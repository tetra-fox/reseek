//! The files service: the only part of the daemon that touches the downloads, incomplete and
//! log directories for an API caller. It lists and deletes there, serves a file or a zipped
//! directory to the browser, makes the tokens that let a link load one file, and prunes files
//! kept past their retention.
//!
//! Every path is checked against its root after links are resolved, so a link inside a root
//! that points out of it leads nowhere. A file a download is writing is held in [`Writing`]
//! and is neither served, deleted nor pruned until the download lets go of it.

mod archive;
mod delete;
mod link;
mod listing;
mod logs;
pub(crate) mod mode;
mod prune;
mod roots;
mod serve;
mod writing;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use api_types::auth::Principal;
use api_types::files::{FileRoot, FileToken, FilesystemDirectory, FilesystemFile};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use cascade::Watcher;
use chrono::Utc;
use futures_util::stream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, warn};

use crate::api::error::ApiError;
use crate::auth::Auth;
use crate::config::Options;
use crate::paths::Paths;
use crate::safety::{self, Os};
use crate::services::{ContentRequest, FileService, FileTarget};
use delete::Kind;
use roots::{Refused, locate};
use serve::Served;
pub use writing::{WriteGuard, Writing};

/// How many pieces of an archive wait to be sent before its writer waits.
const ARCHIVE_BACKLOG: usize = 8;

/// The files service.
pub struct Files {
  config: Watcher<Options>,
  downloads: PathBuf,
  incomplete: PathBuf,
  logs: PathBuf,
  auth: Arc<Auth>,
  writing: Writing,
  /// the principals an archive is being sent to now
  archives: Arc<Mutex<HashSet<String>>>,
}

impl Files {
  /// The service over the directories of `paths`, pruning them by `retention.files` until
  /// `shutdown` is cancelled.
  pub fn start(
    config: Watcher<Options>,
    paths: &Paths,
    auth: Arc<Auth>,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let files = Arc::new(Self {
      config,
      downloads: paths.downloads.clone(),
      incomplete: paths.incomplete.clone(),
      logs: paths.logs.clone(),
      auth,
      writing: Writing::default(),
      archives: Arc::default(),
    });
    prune::spawn(files.clone(), shutdown);
    files
  }

  /// Marks the file at `path`, which a download is writing, so it is not served, deleted or
  /// pruned until the guard is dropped.
  pub fn writing(&self, path: &Path) -> WriteGuard {
    let mut spellings = vec![path.to_path_buf()];
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
      && let Ok(resolved) = std::fs::canonicalize(parent)
    {
      spellings.push(resolved.join(name));
    }
    self.writing.hold(spellings)
  }

  fn root(&self, root: FileRoot) -> &Path {
    match root {
      FileRoot::Downloads => &self.downloads,
      FileRoot::Incomplete => &self.incomplete,
    }
  }
}

/// A root's name as the routes write it.
fn root_name(root: FileRoot) -> &'static str {
  match root {
    FileRoot::Downloads => "downloads",
    FileRoot::Incomplete => "incomplete",
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
      "the {what} '{}' could not be read: {error}",
      path.display()
    ))
  }
}

/// The answer for a call that needs a download's file, which the downloads engine gives.
fn download_files() -> ApiError {
  ApiError::unavailable(Some(
    "the files of downloads not implemented yet".to_string(),
  ))
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
    self
      .archives
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .remove(&self.subject);
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
    let FileTarget::File { root, path } = target else {
      return Err(download_files());
    };
    if request.principal.is_none() {
      let token = request
        .link
        .as_deref()
        .ok_or_else(|| ApiError::unauthorized("authentication required"))?;
      let claimed = link::claimed(root_name(root), &path);
      match link::check(&self.auth.secret, token, &claimed, Utc::now()) {
        Ok(subject) => debug!(subject, "file served by its link token"),
        Err(reason) => {
          return Err(ApiError::unauthorized(format!(
            "the link token is not valid: {reason}"
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
        "the file is being written by a download",
        Some("files in the incomplete directory are served once their download ends".to_string()),
      ));
    }
    let served = Served::at(&resolved)
      .await
      .map_err(|e| unreadable("file", &lexical, e))?
      .ok_or_else(|| ApiError::not_found(format!("'{}' is not a file", lexical.display())))?;
    serve::respond(served, &request).await
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
        "the folder holds {total} bytes, more than web.files.archive_max_bytes ({limit})"
      )));
    }
    let slot = {
      let mut archives = self.archives.lock().unwrap_or_else(PoisonError::into_inner);
      if !archives.insert(principal.subject.clone()) {
        return Err(ApiError::too_many(
          "an archive is being sent to you already",
        ));
      }
      ArchiveSlot {
        archives: self.archives.clone(),
        subject: principal.subject.clone(),
      }
    };
    debug!(files = members.len(), bytes = total, "sending an archive");
    let (sender, receiver) = mpsc::channel(ARCHIVE_BACKLOG);
    tokio::spawn(
      async move {
        archive::send(members, sender).await;
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
    _principal: &Principal,
    _id: uuid::Uuid,
  ) -> Result<FileToken, ApiError> {
    Err(download_files())
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
    let resolved = blocking(move || served_path(&dir, &checked)).await?;
    if !resolved.is_file() {
      return Err(ApiError::not_found(format!(
        "'{}' is not a file",
        self.root(root).join(&relative).display()
      )));
    }
    let (token, expires_at) = link::issue(
      &self.auth.secret,
      &principal.subject,
      &link::claimed(root_name(root), &relative),
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
      .ok_or_else(|| ApiError::not_found("the log file is not a file"))?;
    served.content_type = Some("text/plain; charset=utf-8");
    serve::respond(served, &request).await
  }
}
