//! Finding the one local file a call is about: for work that reads it in place, under the
//! downloads or incomplete directory after links are resolved, a regular file, and not one a
//! download is writing; for what is kept about a file, its path under the directory whether
//! or not the file is still there.

use std::path::PathBuf;
use std::time::SystemTime;

use api_types::files::FileRoot;
use rusqlite::OptionalExtension;
use tracing::{debug, warn};
use uuid::Uuid;

use super::roots::{Refused, locate};
use super::{Files, blocking, served_path, unreadable};
use crate::api::error::ApiError;
use crate::services::FileTarget;

/// A local file found for reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalFile {
  /// the directory it is under
  pub root: FileRoot,
  /// its path under the root, `/`-separated
  pub relative: String,
  /// the root joined with the relative path, as the file is known by
  pub path: PathBuf,
  /// the path resolved through links, which is what is read
  pub resolved: PathBuf,
  /// the download the file belongs to, when it is one's
  pub transfer_id: Option<Uuid>,
  /// its size in bytes
  pub size: u64,
  /// when it was last changed
  pub modified: SystemTime,
}

/// A local file a call names, which may be gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPath {
  /// the directory it is under
  pub root: FileRoot,
  /// its path under the root, `/`-separated
  pub relative: String,
  /// the root joined with the relative path, as the file is known by
  pub path: PathBuf,
  /// its size and when it last changed, `None` when it is no longer there
  pub found: Option<(u64, SystemTime)>,
}

impl Files {
  /// The root and the path under it that `target` names, with the download it belongs to
  /// when it was named by one.
  async fn named(&self, target: &FileTarget) -> Result<(FileRoot, String, Option<Uuid>), ApiError> {
    Ok(match target {
      FileTarget::File { root, path } => (*root, path.clone(), None),
      FileTarget::Download { username, id } => (
        FileRoot::Downloads,
        self.download_file(Some(username), *id).await?,
        Some(*id),
      ),
    })
  }

  /// The path `target` names, whether or not the file is still there: 403 when it leads out
  /// of its directory, 404 when it names a directory.
  pub async fn local_path(&self, target: &FileTarget) -> Result<LocalPath, ApiError> {
    let (root, relative, _) = self.named(target).await?;
    let dir = self.root(root).to_path_buf();
    let path = dir.join(&relative);
    let checked = relative.clone();
    let shown = path.clone();
    let found = blocking(move || {
      let resolved = match locate(&dir, &checked) {
        Ok(located) if located.exists => located.resolved,
        Ok(_) | Err(Refused::RootMissing) => return Ok(None),
        Err(Refused::Outside) => {
          warn!(path = %shown.display(), "a path outside the application's directories refused");
          return Err(
            ApiError::forbidden("Only files in application-controlled directories can be read")
              .logged(),
          );
        }
        Err(Refused::Denied(e) | Refused::Io(e)) => return Err(unreadable("file", &shown, e)),
      };
      let metadata = std::fs::metadata(&resolved).map_err(|e| unreadable("file", &shown, e))?;
      if !metadata.is_file() {
        return Err(ApiError::not_found(format!(
          "'{}' is not a file",
          shown.display()
        )));
      }
      let modified = metadata.modified().map_err(ApiError::internal)?;
      Ok(Some((metadata.len(), modified)))
    })
    .await?;
    Ok(LocalPath {
      root,
      relative,
      path,
      found,
    })
  }

  /// The file `target` names, ready to be read: 404 when there is none, 403 when its path
  /// leads out of its directory, 409 when a download is writing it.
  pub async fn local_file(&self, target: &FileTarget) -> Result<LocalFile, ApiError> {
    let (root, relative, transfer_id) = self.named(target).await?;
    let dir = self.root(root).to_path_buf();
    let path = dir.join(&relative);
    let checked = relative.clone();
    let shown = path.clone();
    let (resolved, metadata) = blocking(move || {
      let resolved = served_path(&dir, &checked)?;
      let metadata = std::fs::metadata(&resolved).map_err(|e| unreadable("file", &shown, e))?;
      Ok((resolved, metadata))
    })
    .await?;
    if !metadata.is_file() {
      return Err(ApiError::not_found(format!(
        "'{}' is not a file",
        path.display()
      )));
    }
    if root == FileRoot::Incomplete && self.writing.holds(&[&path, &resolved]) {
      return Err(ApiError::conflict(
        "The file is being written by a download",
        Some("A file in the incomplete directory can be read once its download ends".to_string()),
      ));
    }
    let transfer_id = match transfer_id {
      Some(id) => Some(id),
      None if root == FileRoot::Downloads => self.download_at(&path).await?,
      None => None,
    };
    Ok(LocalFile {
      root,
      relative,
      path,
      resolved,
      transfer_id,
      size: metadata.len(),
      modified: metadata.modified().map_err(ApiError::internal)?,
    })
  }

  /// The newest download whose file was moved to `path`.
  async fn download_at(&self, path: &std::path::Path) -> Result<Option<Uuid>, ApiError> {
    let local = path.to_string_lossy().into_owned();
    let found = self
      .transfers
      .read(move |conn| {
        conn
          .query_row(
            "SELECT Id FROM Transfers WHERE LocalPath = ?1 AND Direction = 'Download' \
             ORDER BY RequestedAt DESC LIMIT 1",
            [local],
            |row| row.get::<_, String>(0),
          )
          .optional()
      })
      .await
      .map_err(ApiError::internal)?;
    Ok(found.and_then(|id| {
      Uuid::parse_str(&id)
        .inspect_err(|e| debug!(id, error = %e, "a download's id is not a UUID"))
        .ok()
    }))
  }
}
