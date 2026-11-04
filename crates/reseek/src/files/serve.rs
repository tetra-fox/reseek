//! A local file as a response: its type from its extension, one byte range when asked, an
//! `ETag` from its size and time so an unchanged file is answered 304, and the headers that
//! keep a browser from running what it downloads on this site: only sound, raster pictures and
//! plain text are shown, and every file is sandboxed.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use api_types::files::ContentDisposition;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::{debug, warn};

use super::ServeSpeed;
use crate::api::error::ApiError;
use crate::safety::{Os, file_name_safely};
use crate::services::ContentRequest;

/// How much of a file one read takes.
const CHUNK: usize = 256 * 1024;

/// The characters a `filename*` value keeps as they are: letters, digits and these.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'!')
  .remove(b'#')
  .remove(b'$')
  .remove(b'&')
  .remove(b'+')
  .remove(b'-')
  .remove(b'.')
  .remove(b'^')
  .remove(b'_')
  .remove(b'`')
  .remove(b'|')
  .remove(b'~');

/// Picture types a browser draws from pixels, which hold nothing it would run.
const RASTER: &[&str] = &[
  "image/apng",
  "image/avif",
  "image/bmp",
  "image/gif",
  "image/jpeg",
  "image/png",
  "image/webp",
];

/// Whether a file of type `essence` may be shown on this site rather than saved: sound, a raster
/// picture or plain text. Anything else, markup above all, could carry script that runs as this
/// site.
fn shown_inline(essence: &str) -> bool {
  essence == "text/plain" || essence.starts_with("audio/") || RASTER.contains(&essence)
}

/// A file to serve.
pub(super) struct Served {
  /// where it is, resolved
  pub path: PathBuf,
  /// its size in bytes
  pub size: u64,
  /// when its contents last changed
  pub modified: Option<SystemTime>,
  /// its type, from its extension unless given
  pub content_type: Option<&'static str>,
}

impl Served {
  /// The file at `path`, read as it is now; `None` when it is not a regular file.
  pub async fn at(path: &Path) -> std::io::Result<Option<Self>> {
    let metadata = tokio::fs::metadata(path).await?;
    if !metadata.is_file() {
      return Ok(None);
    }
    Ok(Some(Self {
      path: path.to_path_buf(),
      size: metadata.len(),
      modified: metadata.modified().ok(),
      content_type: None,
    }))
  }

  /// The file's own name.
  pub fn name(&self) -> String {
    self
      .path
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      // a served file is never a filesystem root, the one path without a name
      .unwrap_or_default()
  }

  /// An entity tag that changes when the size or the modification time does.
  fn etag(&self) -> String {
    format!("\"{}\"", self.digest())
  }

  /// The entity tag of what `kind` makes of the file, which follows the file as [`Self::etag`]
  /// does.
  pub fn etag_of(&self, kind: &str) -> String {
    format!("\"{}-{kind}\"", self.digest())
  }

  fn digest(&self) -> String {
    let nanos = self
      .modified
      .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
      .map_or(0, |d| d.as_nanos());
    let digest = Sha256::digest(format!("{}-{nanos}", self.size));
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
  }
}

/// What a response of bytes says about them, whatever they are read from.
pub(super) struct Entity {
  /// the name it is shown or saved under
  pub name: String,
  pub etag: String,
  /// its type
  pub kind: String,
  /// its length in bytes
  pub size: u64,
}

/// The byte range a `Range` header asks for, as a first and a last byte, inclusive.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Wanted {
  /// the whole file: no header, or one this server does not read
  Whole,
  /// one range
  Range(u64, u64),
  /// no byte of the file, or more than one range
  Unsatisfiable,
}

/// The range `header` asks for of a file of `size` bytes. A header in another unit or
/// malformed is ignored and the whole file sent; more than one range is not served.
pub(super) fn wanted(header: Option<&str>, size: u64) -> Wanted {
  let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
    return Wanted::Whole;
  };
  if spec.contains(',') {
    return Wanted::Unsatisfiable;
  }
  let Some((first, last)) = spec.trim().split_once('-') else {
    return Wanted::Whole;
  };
  let number = |text: &str| text.trim().parse::<u64>().ok();
  match (first.trim().is_empty(), last.trim().is_empty()) {
    // the last `n` bytes
    (true, false) => match number(last) {
      Some(0) => Wanted::Unsatisfiable,
      Some(_) if size == 0 => Wanted::Unsatisfiable,
      Some(n) => Wanted::Range(size.saturating_sub(n), size - 1),
      None => Wanted::Whole,
    },
    // from `first` to the end
    (false, true) => match number(first) {
      Some(start) if start < size => Wanted::Range(start, size - 1),
      Some(_) => Wanted::Unsatisfiable,
      None => Wanted::Whole,
    },
    (false, false) => match (number(first), number(last)) {
      (Some(start), Some(end)) if start <= end && start < size => {
        Wanted::Range(start, end.min(size - 1))
      }
      (Some(start), Some(end)) if start <= end => Wanted::Unsatisfiable,
      _ => Wanted::Whole,
    },
    (true, true) => Wanted::Whole,
  }
}

/// Whether `If-None-Match` names `etag`, weak tags matching their strong form.
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
  let Some(value) = headers
    .get(header::IF_NONE_MATCH)
    .and_then(|v| v.to_str().ok())
  else {
    return false;
  };
  value
    .split(',')
    .map(|tag| tag.trim().trim_start_matches("W/"))
    .any(|tag| tag == "*" || tag == etag)
}

/// Whether a range may be sent under `If-Range`: when there is none, or it names `etag`. A weak
/// tag or a date never matches, since only a strong tag says the bytes are the same.
fn range_applies(headers: &HeaderMap, etag: &str) -> bool {
  headers
    .get(header::IF_RANGE)
    .is_none_or(|value| value.to_str().is_ok_and(|tag| tag.trim() == etag))
}

/// The `Content-Disposition` for a file named `name`: the asked disposition for a type that may
/// be shown, attachment for any other, with the name made safe and percent-encoded.
fn disposition(request: &ContentRequest, own_name: &str, content_type: &str) -> String {
  let essence = crate::api::extract::essence(content_type);
  let kind = if shown_inline(&essence) {
    request.disposition
  } else {
    ContentDisposition::Attachment
  };
  let kind = match kind {
    ContentDisposition::Inline => "inline",
    ContentDisposition::Attachment => "attachment",
  };
  let name = request
    .name
    .as_deref()
    .and_then(|name| file_name_safely(name, true, Os::CURRENT))
    .or_else(|| file_name_safely(own_name, true, Os::CURRENT))
    .unwrap_or_else(|| "download".to_string());
  format!(
    "{kind}; filename*=UTF-8''{}",
    utf8_percent_encode(&name, ATTR_CHAR)
  )
}

/// The type of a file named `name` from its extension, text as UTF-8.
fn content_type(name: &str) -> String {
  let guessed = mime_guess::from_path(name).first_or_octet_stream();
  if guessed.type_() == mime_guess::mime::TEXT {
    format!("{}; charset=utf-8", guessed.essence_str())
  } else {
    guessed.essence_str().to_string()
  }
}

/// `file` as the response `request` asks for, sent no faster than `speed` allows.
pub(super) async fn respond(
  file: Served,
  request: &ContentRequest,
  speed: &ServeSpeed,
) -> Result<Response, ApiError> {
  let name = file.name();
  let entity = Entity {
    etag: file.etag(),
    kind: match file.content_type {
      Some(kind) => kind.to_string(),
      None => content_type(&name),
    },
    size: file.size,
    name,
  };
  match ranged(&entity, request) {
    Ranged::Answered(response) => Ok(response),
    Ranged::Bytes(part) => {
      let bytes = body(&file.path, part.start, part.length, speed.clone()).await?;
      Ok(part.with(bytes))
    }
  }
}

/// What a request for an entity comes to.
pub(super) enum Ranged {
  /// the response, nothing of the entity read: unchanged, refused or only its headers
  Answered(Response),
  /// a part to read and send
  Bytes(Part),
}

/// The part of an entity a response sends.
pub(super) struct Part {
  status: StatusCode,
  headers: HeaderMap,
  /// where it starts
  pub start: u64,
  /// its length in bytes
  pub length: u64,
}

impl Part {
  /// The response that sends `body`, what was read for this part.
  pub fn with(self, body: Body) -> Response {
    (self.status, self.headers, body).into_response()
  }
}

/// What `request` asks of `entity`: unchanged when the caller has it, else its headers and,
/// unless they are all that is wanted, one part of it or the whole.
pub(super) fn ranged(entity: &Entity, request: &ContentRequest) -> Ranged {
  let mut headers = HeaderMap::new();
  let put =
    |headers: &mut HeaderMap, name: header::HeaderName, value: &str| match HeaderValue::from_str(
      value,
    ) {
      Ok(value) => {
        headers.insert(name, value);
      }
      Err(e) => debug!(header = %name, error = %e, "header not sent"),
    };
  put(&mut headers, header::ETAG, &entity.etag);
  put(&mut headers, header::CACHE_CONTROL, "private, no-cache");
  put(&mut headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
  // a file opened in a tab is a page of an origin of its own, which can run nothing here
  put(&mut headers, header::CONTENT_SECURITY_POLICY, "sandbox");
  if not_modified(&request.headers, &entity.etag) {
    return Ranged::Answered((StatusCode::NOT_MODIFIED, headers).into_response());
  }
  put(&mut headers, header::ACCEPT_RANGES, "bytes");
  let range = request
    .headers
    .get(header::RANGE)
    .and_then(|v| v.to_str().ok())
    .filter(|_| range_applies(&request.headers, &entity.etag));
  let size = entity.size;
  let (status, start, length) = match wanted(range, size) {
    Wanted::Whole => (StatusCode::OK, 0, size),
    Wanted::Range(first, last) => {
      put(
        &mut headers,
        header::CONTENT_RANGE,
        &format!("bytes {first}-{last}/{size}"),
      );
      (StatusCode::PARTIAL_CONTENT, first, last - first + 1)
    }
    Wanted::Unsatisfiable => {
      let mut response = ApiError::range_not_satisfiable(format!(
        "The file has {size} bytes. Only one range within them can be requested."
      ))
      .into_response();
      put(
        response.headers_mut(),
        header::CONTENT_RANGE,
        &format!("bytes */{size}"),
      );
      return Ranged::Answered(response);
    }
  };
  put(&mut headers, header::CONTENT_TYPE, &entity.kind);
  put(
    &mut headers,
    header::CONTENT_DISPOSITION,
    &disposition(request, &entity.name, &entity.kind),
  );
  headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
  if request.head {
    return Ranged::Answered((status, headers).into_response());
  }
  Ranged::Bytes(Part {
    status,
    headers,
    start,
    length,
  })
}

/// `length` bytes of the file at `path` from `start`, read as they are sent and no faster
/// than `speed` allows.
pub(super) async fn body(
  path: &Path,
  start: u64,
  length: u64,
  speed: ServeSpeed,
) -> Result<Body, ApiError> {
  let mut opened = tokio::fs::File::open(path)
    .await
    .map_err(|e| match e.kind() {
      std::io::ErrorKind::NotFound => ApiError::not_found("The file no longer exists"),
      _ => ApiError::internal(format!("{} not opened: {e}", path.display())),
    })?;
  opened
    .seek(std::io::SeekFrom::Start(start))
    .await
    .map_err(ApiError::internal)?;
  let shown = path.display().to_string();
  let chunks = stream::unfold(
    (opened, length, shown, speed),
    |(mut file, left, shown, speed)| async move {
      if left == 0 {
        return None;
      }
      let want = CHUNK.min(usize::try_from(left).unwrap_or(CHUNK));
      let mut buffer = vec![0; speed.allowed(want).await];
      match file.read(&mut buffer).await {
        // the file got shorter while it was sent; the length promised cannot be kept
        Ok(0) => {
          warn!(
            path = shown,
            missing = left,
            "a file being served got shorter"
          );
          Some((
            Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
            (file, 0, shown, speed),
          ))
        }
        Ok(read) => {
          buffer.truncate(read);
          Some((
            Ok(Bytes::from(buffer)),
            (file, left - read as u64, shown, speed),
          ))
        }
        Err(e) => {
          warn!(path = shown, error = %e, "a file being served could not be read");
          Some((Err(e), (file, 0, shown, speed)))
        }
      }
    },
  );
  Ok(Body::from_stream(chunks))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn one_range_is_read_and_anything_else_is_whole_or_refused() {
    assert_eq!(wanted(None, 100), Wanted::Whole);
    assert_eq!(wanted(Some("bytes=0-9"), 100), Wanted::Range(0, 9));
    assert_eq!(wanted(Some("bytes=90-200"), 100), Wanted::Range(90, 99));
    assert_eq!(wanted(Some("bytes=10-"), 100), Wanted::Range(10, 99));
    assert_eq!(wanted(Some("bytes=-10"), 100), Wanted::Range(90, 99));
    assert_eq!(wanted(Some("bytes=-500"), 100), Wanted::Range(0, 99));
    assert_eq!(wanted(Some("bytes=100-"), 100), Wanted::Unsatisfiable);
    assert_eq!(wanted(Some("bytes=150-160"), 100), Wanted::Unsatisfiable);
    assert_eq!(wanted(Some("bytes=-0"), 100), Wanted::Unsatisfiable);
    assert_eq!(wanted(Some("bytes=0-1,5-6"), 100), Wanted::Unsatisfiable);
    assert_eq!(wanted(Some("bytes=0-0"), 0), Wanted::Unsatisfiable);
    assert_eq!(wanted(Some("bytes=-1"), 0), Wanted::Unsatisfiable);
    assert_eq!(wanted(Some("items=0-1"), 100), Wanted::Whole);
    assert_eq!(wanted(Some("bytes=x-y"), 100), Wanted::Whole);
    assert_eq!(wanted(Some("bytes=9-3"), 100), Wanted::Whole);
  }

  #[test]
  fn only_sound_pictures_and_plain_text_are_shown_inline() {
    let request = |disposition, name: Option<&str>| ContentRequest {
      headers: HeaderMap::new(),
      head: false,
      disposition,
      name: name.map(str::to_string),
      principal: None,
      link: None,
    };
    let inline = request(ContentDisposition::Inline, None);
    assert_eq!(
      disposition(&inline, "01 - Track.flac", "audio/flac"),
      "inline; filename*=UTF-8''01%20-%20Track.flac"
    );
    assert_eq!(
      disposition(&inline, "page.html", "text/html; charset=utf-8"),
      "attachment; filename*=UTF-8''page.html"
    );
    for (name, kind) in [
      ("x.svg", "image/svg+xml"),
      ("feed.xml", "text/xml; charset=utf-8"),
      ("style.xsl", "application/xslt+xml"),
      ("doc.pdf", "application/pdf"),
    ] {
      assert!(
        disposition(&inline, name, kind).starts_with("attachment"),
        "{name} is shown inline"
      );
    }
    for (name, kind) in [
      ("cover.jpg", "image/jpeg"),
      ("notes.txt", "text/plain; charset=utf-8"),
      ("01.wav", "audio/wav"),
    ] {
      assert!(
        disposition(&inline, name, kind).starts_with("inline"),
        "{name} is saved"
      );
    }
    let renamed = request(ContentDisposition::Attachment, Some("../caf\u{e9}.flac"));
    assert_eq!(
      disposition(&renamed, "x.flac", "audio/flac"),
      "attachment; filename*=UTF-8''caf%C3%A9.flac",
      "only the base name, encoded"
    );
    assert_eq!(content_type("notes.txt"), "text/plain; charset=utf-8");
    assert_eq!(content_type("noextension"), "application/octet-stream");
  }

  #[test]
  fn etags_match_with_or_without_the_weak_mark() {
    let mut headers = HeaderMap::new();
    headers.insert(
      header::IF_NONE_MATCH,
      "\"other\", W/\"abc\"".parse().unwrap(),
    );
    assert!(not_modified(&headers, "\"abc\""));
    assert!(!not_modified(&headers, "\"abd\""));
    assert!(!not_modified(&HeaderMap::new(), "\"abc\""));
  }
}
