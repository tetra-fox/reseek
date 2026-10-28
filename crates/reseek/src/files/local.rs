//! Finding the one local file a call is about, for work that reads it in place: under the
//! downloads or incomplete directory after links are resolved, a regular file, and not one a
//! download is writing.

use std::path::PathBuf;
use std::time::SystemTime;

use api_types::files::FileRoot;
use rusqlite::OptionalExtension;
use tracing::debug;
use uuid::Uuid;

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

impl Files {
  /// The file `target` names, ready to be read: 404 when there is none, 403 when its path
  /// leads out of its directory, 409 when a download is writing it.
  pub async fn local_file(&self, target: &FileTarget) -> Result<LocalFile, ApiError> {
    let (root, relative, transfer_id) = match target {
      FileTarget::File { root, path } => (*root, path.clone(), None),
      FileTarget::Download { username, id } => (
        FileRoot::Downloads,
        self.download_file(Some(username), *id).await?,
        Some(*id),
      ),
    };
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
        "the file is being written by a download",
        Some("a file in the incomplete directory can be read once its download ends".to_string()),
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
