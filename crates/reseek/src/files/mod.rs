//! The files service: the only part of the daemon that touches the downloads, incomplete and
//! log directories for an API caller. It lists and deletes there, serves a file or a zipped
//! directory to the browser, makes the tokens that let a link load one file, and prunes files
//! kept past their retention.
//!
//! Every path is checked against its root after links are resolved, so a link inside a root
//! that points out of it leads nowhere. Files and archives are sent no faster than
//! `web.files.serve_speed_limit` allows over every response together. A file a download is writing is held in [`Writing`]
//! and is neither served, deleted nor pruned until the download lets go of it.

mod archive;
mod delete;
mod link;
mod listing;
mod local;
mod logs;
pub(crate) mod mode;
mod placing;
mod prune;
mod roots;
mod serve;
mod writing;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_types::auth::Principal;
use api_types::files::{FileRoot, FileToken, FilesystemDirectory, FilesystemFile};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use cascade::Watcher;
use chrono::Utc;
use futures_util::stream;
use rusqlite::OptionalExtension;
use soulseek_client::TokenBucket;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, warn};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::auth::Auth;
use crate::config::Options;
use crate::db::Database;
use crate::paths::Paths;
use crate::safety::{self, Os};
use crate::services::{ContentRequest, FileService, FileTarget};
use crate::sync::lock;
use delete::Kind;
pub use local::{LocalFile, LocalPath};
pub use placing::{create_file, move_file};
use roots::{Refused, locate};
use serve::Served;
pub use writing::{WriteGuard, Writing};

/// How many pieces of an archive wait to be sent before its writer waits.
const ARCHIVE_BACKLOG: usize = 8;

/// The speed limit every served file and archive shares.
#[derive(Clone)]
pub(crate) struct ServeSpeed {
  bucket: Arc<TokenBucket>,
  /// ends a wait for the limit when the daemon stops
  shutdown: CancellationToken,
}

impl ServeSpeed {
  /// How many of the `wanted` bytes may be read and sent now, waiting until some may.
  async fn allowed(&self, wanted: usize) -> usize {
    let wanted = u32::try_from(wanted).unwrap_or(u32::MAX);
    match self.bucket.get(wanted, &self.shutdown).await {
      Ok(granted) => granted as usize,
      // the daemon stops; what is left goes without a limit
      Err(_) => wanted as usize,
    }
  }
}

/// The files service.
pub struct Files {
  config: Watcher<Options>,
  /// `transfers.db`, where a download's file is found
  transfers: Database,
  downloads: PathBuf,
  incomplete: PathBuf,
  logs: PathBuf,
  auth: Arc<Auth>,
  writing: Writing,
  /// the principals an archive is being sent to now
  archives: Arc<Mutex<HashSet<String>>>,
  /// the speed files are served at
  speed: ServeSpeed,
}

impl Files {
  /// The service over the directories of `paths`, finding downloads' files in `transfers`,
  /// pruning them by `retention.files` and following the speed files are served at until
  /// `shutdown` is canceled.
  pub fn start(
    config: Watcher<Options>,
    transfers: Database,
    paths: &Paths,
    auth: Arc<Auth>,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let limit = config.current().values.web.files.serve_speed_limit;
    let speed = ServeSpeed {
      // the client's bucket takes 0 as no limit
      bucket: Arc::new(TokenBucket::for_speed(limit.unwrap_or(0))),
      shutdown: shutdown.clone(),
    };
    tokio::spawn(follow_speed(
      config.clone(),
      speed.bucket.clone(),
      shutdown.clone(),
    ));
    let files = Arc::new(Self {
      config,
      transfers,
      downloads: paths.downloads.clone(),
      incomplete: paths.incomplete.clone(),
      logs: paths.logs.clone(),
      auth,
      writing: Writing::default(),
      archives: Arc::default(),
      speed,
    });
    prune::spawn(files.clone(), shutdown);
    files
  }

  /// Marks the file at `path`, which a download is writing, so it is not served, deleted or
  /// pruned until the guard is dropped.
  pub fn writing(&self, path: &Path) -> WriteGuard {
    let mut spellings = vec![path.to_path_buf()];
    // a parent that does not resolve has only the spelling given
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
      && let Ok(resolved) = std::fs::canonicalize(parent)
    {
      spellings.push(resolved.join(name));
    }
    self.writing.hold(spellings)
  }

  /// The path under the downloads directory of the file the succeeded download `id` was moved
  /// to, `/`-separated; `username`, when given, must be the download's peer. 404 when there is
  /// no such download, it has no file, or its file is no longer under the downloads directory.
  async fn download_file(&self, username: Option<&str>, id: Uuid) -> Result<String, ApiError> {
    let found = self
      .transfers
      .read(move |conn| {
        conn
          .query_row(
            "SELECT Username, LocalPath FROM Transfers WHERE Id = ?1 AND Direction = 'Download'",
            [id.to_string()],
            |row| {
              Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
              ))
            },
          )
          .optional()
      })
      .await
      .map_err(ApiError::internal)?;
    let missing = || ApiError::not_found(format!("No file for the download {id}"));
    let Some((peer, local)) = found else {
      return Err(missing());
    };
    if username.is_some_and(|username| peer.as_deref() != Some(username)) {
      return Err(missing());
    }
    let local = PathBuf::from(local.ok_or_else(missing)?);
    let relative = local.strip_prefix(&self.downloads).map_err(|_| {
      debug!(path = %local.display(), "a download's file outside the downloads directory");
      missing()
    })?;
    let segments: Option<Vec<&str>> = relative
      .components()
      .map(|c| c.as_os_str().to_str())
      .collect();
    Ok(segments.ok_or_else(missing)?.join("/"))
  }

  fn root(&self, root: FileRoot) -> &Path {
    match root {
      FileRoot::Downloads => &self.downloads,
      FileRoot::Incomplete => &self.incomplete,
    }
  }
}

/// Runs blocking filesystem work off the runtime.
async fn blocking<T: Send + 'static>(
  work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
  tokio::task::spawn_blocking(work)
    .await
    .map_err(|e| ApiError::internal(format!("a file task did not finish: {e}")))?
}

/// The answer to `error` met reading `path`, which a message calls `what`: 404 when it is not
/// there, 403 when the system refuses it, else a 500 with the error logged.
fn unreadable(what: &str, path: &Path, error: std::io::Error) -> ApiError {
  if roots::missing(&error) {
    ApiError::not_found(format!("The {what} '{}' does not exist", path.display()))
  } else if error.kind() == std::io::ErrorKind::PermissionDenied {
    ApiError::forbidden(format!(
      "Access to {what} '{}' was denied: {error}",
      path.display()
    ))
  } else {
    ApiError::internal(format!(
      "Could not read the {what} '{}': {error}",
      path.display()
    ))
  }
}

/// Finds the file `relative` under `root` to serve.
fn served_path(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
  let shown = root.join(relative);
  let missing = || ApiError::not_found(format!("The file '{}' does not exist", shown.display()));
  match locate(root, relative) {
    Ok(located) if located.exists => Ok(located.resolved),
    Ok(_) | Err(Refused::RootMissing) => Err(missing()),
    Err(Refused::Outside) => {
      warn!(path = %shown.display(), "serving a file outside the application's directories refused");
      Err(
        ApiError::forbidden("Only files in application-controlled directories can be served")
          .logged(),
      )
    }
    Err(Refused::Denied(e) | Refused::Io(e)) => Err(unreadable("file", &shown, e)),
  }
}

/// A name for an archive's `Content-Disposition`: the base name made safe, without the quotes
/// and controls a quoted header value can not hold.
fn archive_name(name: &str) -> String {
  safety::file_name_safely(name, true, Os::CURRENT)
    .unwrap_or_else(|| "archive".to_string())
    .chars()
    .map(|c| {
      if c == '"' || c == '\\' || c.is_control() {
        '_'
      } else {
        c
      }
    })
    .collect()
}

/// Lets one archive at a time go to a principal: the slot is free again when dropped.
struct ArchiveSlot {
  archives: Arc<Mutex<HashSet<String>>>,
  subject: String,
}

impl Drop for ArchiveSlot {
  fn drop(&mut self) {
    lock(&self.archives).remove(&self.subject);
  }
}

#[async_trait]
impl FileService for Files {
  async fn list(
    &self,
    root: FileRoot,
    path: &str,
    recursive: bool,
  ) -> Result<FilesystemDirectory, ApiError> {
    let dir = self.root(root).to_path_buf();
    let path = path.to_string();
    blocking(move || listing::list(&dir, &path, recursive)).await
  }

  async fn delete_directory(&self, root: FileRoot, path: &str) -> Result<(), ApiError> {
    let dir = self.root(root).to_path_buf();
    let (path, writing) = (path.to_string(), self.writing.clone());
    blocking(move || delete::delete(&dir, &path, Kind::Directory, &writing)).await
  }

  async fn delete_file(&self, root: FileRoot, path: &str) -> Result<(), ApiError> {
    let dir = self.root(root).to_path_buf();
    let (path, writing) = (path.to_string(), self.writing.clone());
    blocking(move || delete::delete(&dir, &path, Kind::File, &writing)).await
  }

  async fn content(
    &self,
    target: FileTarget,
    request: ContentRequest,
  ) -> Result<Response, ApiError> {
    let link = match (&request.principal, request.link.as_deref()) {
      (Some(_), _) => None,
      (None, Some(token)) => Some(token),
      (None, None) => return Err(ApiError::unauthorized("Authentication required")),
    };
    let (root, path) = match target {
      FileTarget::File { root, path } => (root, path),
      FileTarget::Download { username, id } => (
        FileRoot::Downloads,
        self.download_file(Some(&username), id).await?,
      ),
    };
    if let Some(token) = link {
      let claimed = link::claimed(&root.to_string(), &path);
      match link::check(&self.auth.secret, token, &claimed, Utc::now()) {
        Ok(subject) => debug!(subject, "file served by its link token"),
        Err(reason) => {
          return Err(ApiError::unauthorized(format!(
            "The link token is not valid: {reason}"
          )));
        }
      }
    }
    let dir = self.root(root).to_path_buf();
    let relative = path.clone();
    let resolved = blocking(move || served_path(&dir, &relative)).await?;
    let lexical = self.root(root).join(&path);
    if root == FileRoot::Incomplete && self.writing.holds(&[&lexical, &resolved]) {
      return Err(ApiError::conflict(
        "The file is being written by a download",
        Some("Files in the incomplete directory are served once their download ends".to_string()),
      ));
    }
    let served = Served::at(&resolved)
      .await
      .map_err(|e| unreadable("file", &lexical, e))?
      .ok_or_else(|| ApiError::not_found(format!("'{}' is not a file", lexical.display())))?;
    serve::respond(served, &request, &self.speed).await
  }

  async fn archive(
    &self,
    principal: &Principal,
    root: FileRoot,
    path: &str,
    name: Option<String>,
  ) -> Result<Response, ApiError> {
    let dir = self.root(root).to_path_buf();
    let relative = path.to_string();
    let (located_name, members) = blocking(move || {
      let shown = dir.join(&relative);
      let missing = || {
        ApiError::not_found(format!(
          "The directory '{}' does not exist",
          shown.display()
        ))
      };
      let located = match locate(&dir, &relative) {
        Ok(located) if located.exists && located.resolved.is_dir() => located,
        Ok(_) | Err(Refused::RootMissing) => return Err(missing()),
        Err(Refused::Outside) => {
          warn!(path = %shown.display(), "archiving outside the application's directories refused");
          return Err(
            ApiError::forbidden("Only application-controlled directories can be archived").logged(),
          );
        }
        Err(Refused::Denied(e)) | Err(Refused::Io(e)) => {
          return Err(unreadable("directory", &shown, e));
        }
      };
      let members =
        archive::members(&located.resolved).map_err(|e| unreadable("directory", &shown, e))?;
      Ok((located.name(), members))
    })
    .await?;
    // the incomplete directory serves only files no download is writing
    let members = if root == FileRoot::Incomplete {
      let lexical = self.root(root).join(path);
      members
        .into_iter()
        .filter(|member| {
          let written = self
            .writing
            .holds(&[&member.path, &lexical.join(&member.name)]);
          if written {
            debug!(file = %member.path.display(), "file a download writes left out of an archive");
          }
          !written
        })
        .collect()
    } else {
      members
    };
    let total: u64 = members.iter().map(|m| m.size).sum();
    let limit = self
      .config
      .current()
      .values
      .web
      .files
      .archive_max_bytes
      .as_u64();
    if total > limit {
      return Err(ApiError::too_large(format!(
        "The folder holds {total} bytes, more than web.files.archive_max_bytes ({limit})"
      )));
    }
    let slot = {
      let mut archives = lock(&self.archives);
      if !archives.insert(principal.subject.clone()) {
        return Err(ApiError::too_many("An archive is already being sent"));
      }
      ArchiveSlot {
        archives: self.archives.clone(),
        subject: principal.subject.clone(),
      }
    };
    debug!(files = members.len(), bytes = total, "sending an archive");
    let (sender, receiver) = mpsc::channel(ARCHIVE_BACKLOG);
    let speed = self.speed.clone();
    tokio::spawn(
      async move {
        archive::send(members, sender, &speed).await;
        drop(slot);
      }
      .instrument(tracing::Span::current()),
    );
    let body = Body::from_stream(stream::unfold(receiver, |mut receiver| async move {
      receiver.recv().await.map(|piece| (piece, receiver))
    }));
    let file_name = format!(
      "{}.zip",
      archive_name(name.as_deref().unwrap_or(&located_name))
    );
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
      header::CONTENT_TYPE,
      HeaderValue::from_static("application/zip"),
    );
    headers.insert(
      header::CACHE_CONTROL,
      HeaderValue::from_static("private, no-cache"),
    );
    headers.insert(
      header::X_CONTENT_TYPE_OPTIONS,
      HeaderValue::from_static("nosniff"),
    );
    match HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\"")) {
      Ok(value) => {
        headers.insert(header::CONTENT_DISPOSITION, value);
      }
      Err(e) => debug!(error = %e, "archive name not sent"),
    }
    Ok(response)
  }

  async fn download_link_token(
    &self,
    principal: &Principal,
    id: Uuid,
  ) -> Result<FileToken, ApiError> {
    let relative = self.download_file(None, id).await?;
    self
      .link_token(principal, FileRoot::Downloads, &relative)
      .await
  }

  async fn link_token(
    &self,
    principal: &Principal,
    root: FileRoot,
    relative: &str,
  ) -> Result<FileToken, ApiError> {
    let relative = relative.to_string();
    let dir = self.root(root).to_path_buf();
    let checked = relative.clone();
    let is_file = blocking(move || served_path(&dir, &checked).map(|path| path.is_file())).await?;
    if !is_file {
      return Err(ApiError::not_found(format!(
        "'{}' is not a file",
        self.root(root).join(&relative).display()
      )));
    }
    let (token, expires_at) = link::issue(
      &self.auth.secret,
      &principal.subject,
      &link::claimed(&root.to_string(), &relative),
      Utc::now(),
    );
    debug!(
      subject = principal.subject,
      ?root,
      path = relative,
      "file link token made"
    );
    Ok(FileToken { token, expires_at })
  }

  async fn log_files(&self) -> Result<Vec<FilesystemFile>, ApiError> {
    let dir = self.logs.clone();
    blocking(move || logs::files(&dir)).await
  }

  async fn log_file(&self, name: &str, request: ContentRequest) -> Result<Response, ApiError> {
    let dir = self.logs.clone();
    let name = name.to_string();
    let resolved = blocking(move || {
      let combined = safety::combine_safely(&dir.to_string_lossy(), &[&name], Os::CURRENT)
        .map_err(ApiError::bad_request)?;
      debug!(path = combined, "log file asked for");
      served_path(&dir, &name)
    })
    .await?;
    let mut served = Served::at(&resolved)
      .await
      .map_err(|e| unreadable("log file", &resolved, e))?
      .ok_or_else(|| ApiError::not_found("The log file is not a regular file"))?;
    served.content_type = Some("text/plain; charset=utf-8");
    serve::respond(served, &request, &self.speed).await
  }
}

/// Takes every change of `web.files.serve_speed_limit` into `bucket` until `shutdown`.
async fn follow_speed(
  config: Watcher<Options>,
  bucket: Arc<TokenBucket>,
  shutdown: CancellationToken,
) {
  let mut changes = config.changes();
  loop {
    let change = tokio::select! {
      () = shutdown.cancelled() => return,
      change = changes.recv() => change,
    };
    let limit = match change {
      Ok(change) => change.current.values.web.files.serve_speed_limit,
      Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
        config.current().values.web.files.serve_speed_limit
      }
      Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
    };
    bucket.set_speed(limit.unwrap_or(0));
    debug!(limit, "speed files are served at");
  }
}
