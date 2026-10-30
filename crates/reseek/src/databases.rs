//! The database files as the Data tab shows them: their sizes on disk, and rebuilding one
//! without its free pages.

use std::path::Path;

use api_types::application::DatabaseFile;
use async_trait::async_trait;
use tracing::{info, warn};

use crate::api::error::ApiError;
use crate::db::Database;
use crate::services::ApplicationService;

/// Every database the daemon opened.
pub struct Databases {
  list: Vec<Database>,
}

impl Databases {
  /// The databases of `list`; those in memory are left out of the listing.
  pub fn new(list: Vec<Database>) -> Self {
    Self { list }
  }

  /// Lets SQLite update its statistics in every database before the daemon stops.
  pub async fn close(&self) {
    for db in &self.list {
      db.close().await;
    }
  }

  /// The database whose file is `<name>.db`.
  fn named(&self, name: &str) -> Option<&Database> {
    self
      .list
      .iter()
      .find(|db| db.path().and_then(stem).as_deref() == Some(name))
  }
}

/// A database file's name without `.db`.
fn stem(path: &Path) -> Option<String> {
  path
    .file_name()?
    .to_str()?
    .strip_suffix(".db")
    .map(str::to_string)
}

/// The size of the file at `path`, 0 when there is none or it cannot be read.
async fn size(path: &Path) -> u64 {
  match tokio::fs::metadata(path).await {
    Ok(metadata) => metadata.len(),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
    Err(e) => {
      warn!(file = %path.display(), error = %e, "database file size not read");
      0
    }
  }
}

#[async_trait]
impl ApplicationService for Databases {
  async fn databases(&self) -> Result<Vec<DatabaseFile>, ApiError> {
    let mut files = Vec::new();
    for path in self.list.iter().filter_map(Database::path) {
      let Some(name) = stem(path) else {
        continue;
      };
      let mut wal = path.as_os_str().to_owned();
      wal.push("-wal");
      files.push(DatabaseFile {
        name,
        path: path.display().to_string(),
        size_bytes: size(path).await,
        wal_size_bytes: size(Path::new(&wal)).await,
      });
    }
    Ok(files)
  }

  async fn vacuum(&self, name: &str) -> Result<(), ApiError> {
    let db = self
      .named(name)
      .ok_or_else(|| ApiError::not_found(format!("No database named {name}")))?;
    db.vacuum().await.map_err(|e| {
      warn!(database = name, error = %e, "database not vacuumed");
      ApiError::internal(e).logged()
    })?;
    info!(database = name, "database vacuumed");
    Ok(())
  }
}
