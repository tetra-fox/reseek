//! What follows a download whose file arrived: the file is moved from the incomplete
//! directory to its destination under the downloads directory and the record learns where it
//! is, then `DownloadFileComplete` is raised, and `DownloadDirectoryComplete` too when no other
//! download from the same remote directory of the same user is left. A move that fails keeps
//! the download succeeded with the move's error, and raises `DownloadFileFailed`.

use std::path::{Path, PathBuf};

use api_types::events::{
  DownloadDirectoryComplete, DownloadFailureStage, DownloadFileComplete, DownloadFileFailed, Event,
};
use api_types::transfers::TransferDirection;
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, error, info, warn};

use super::Downloads;
use super::batches::{self, BatchRow};
use super::destination::{SearchFound, Source, destination_directory, subdirectory};
use crate::config::enums::DestinationExists;
use crate::files::mode::to_unix_file_mode;
use crate::files::move_file;
use crate::safety::Os;
use crate::transfers::listing::remote_directory;
use crate::transfers::{Record, direction_text};

/// The search a batch was made from, by its id, which the searches engine keeps; none is
/// found until it keeps them.
fn search_of(_id: uuid::Uuid) -> Option<SearchFound> {
  None
}

impl Downloads {
  /// Moves the arrived file of `record` from `incomplete` to its destination and raises the
  /// events that follow.
  pub(super) async fn complete(&self, mut record: Record, incomplete: PathBuf) {
    let options = self.config.current().values.clone();
    let destination = &options.transfers.download.destination;
    // validated when the options were loaded
    let mode = destination
      .permissions
      .mode
      .as_deref()
      .and_then(|text| to_unix_file_mode(text).ok());
    let overwrite = destination.exists == DestinationExists::Overwrite;
    let directory = match self
      .destination(&record, &incomplete, destination.subdirectory.as_deref())
      .await
    {
      Ok(directory) => directory,
      Err(reason) => return self.move_failed(&mut record, &incomplete, reason).await,
    };
    let source = incomplete.clone();
    let target = directory.clone();
    let moved = tokio::task::spawn_blocking(move || move_file(&source, &target, mode, overwrite))
      .await
      .map_err(|e| format!("the move did not finish: {e}"))
      .and_then(|moved| moved.map_err(|e| e.to_string()));
    let path = match moved {
      Ok(path) => path,
      Err(reason) => return self.move_failed(&mut record, &incomplete, reason).await,
    };
    record.local_path = Some(path.display().to_string());
    self.save(&record, false).await;
    info!(path = %path.display(), "download moved into the downloads directory");
    if options.relay.enabled {
      // the relay controller learns of finished downloads once the relay runs
      debug!(path = %path.display(), "relay controller not told of the download; the relay is not running");
    }
    let ended_at = record.ended_at.unwrap_or_else(Utc::now);
    self.events.raise_at(
      Event::DownloadFileComplete(DownloadFileComplete {
        version: 0,
        local_filename: path.display().to_string(),
        remote_filename: record.filename.clone(),
        transfer: record.transfer(),
      }),
      ended_at,
    );
    self.directory_complete(&record, &directory).await;
  }

  /// The directory under the downloads directory the file of `record` goes to.
  async fn destination(
    &self,
    record: &Record,
    incomplete: &Path,
    pattern: Option<&str>,
  ) -> Result<PathBuf, String> {
    let batch: Option<BatchRow> = match record.batch_id {
      Some(id) => self
        .db
        .read(move |conn| batches::get(conn, id))
        .await
        .map_err(|e| format!("the batch was not read: {e}"))?,
      None => None,
    };
    let search = batch.as_ref().and_then(|b| b.search_id).and_then(search_of);
    let source = Source {
      username: &record.username,
      filename: &record.filename,
      batch: batch.as_ref(),
      search: search.as_ref(),
    };
    let relative = subdirectory(pattern, &source, Os::CURRENT)?;
    let name = incomplete
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .ok_or_else(|| format!("the incomplete file {} has no name", incomplete.display()))?;
    let (directory, shortened) = destination_directory(
      &self.downloads,
      &relative,
      &record.username,
      &name,
      Os::CURRENT,
    )?;
    if shortened {
      warn!(directory = %directory.display(), "destination path too long; the file goes under the peer's directory");
    }
    Ok(directory)
  }

  /// Keeps the download of `record` succeeded with `reason` as its error, since every byte
  /// arrived, and raises `DownloadFileFailed` for the move.
  async fn move_failed(&self, record: &mut Record, incomplete: &Path, reason: String) {
    error!(path = %incomplete.display(), reason, "downloaded file not moved into the downloads directory");
    record.exception = Some(reason.clone());
    self.save(record, false).await;
    self.events.raise_at(
      Event::DownloadFileFailed(DownloadFileFailed {
        version: 0,
        stage: DownloadFailureStage::Move,
        reason,
        local_filename: Some(incomplete.display().to_string()),
        remote_filename: record.filename.clone(),
        transfer: record.transfer(),
      }),
      record.ended_at.unwrap_or_else(Utc::now),
    );
  }

  /// Raises `DownloadDirectoryComplete` for the remote directory of `record` when no download
  /// from it by the same user is still to finish, queued here included. Asked in the user's
  /// turn, so an enqueue of more files from it cannot slip between.
  async fn directory_complete(&self, record: &Record, local: &Path) {
    let _turn = self.turn(&record.username).await;
    let directory = remote_directory(&record.filename).to_string();
    let username = record.username.clone();
    let pending = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(
          "SELECT Filename FROM Transfers WHERE Username = ?1 AND Direction = ?2 AND Removed = 0 \
           AND (State & 16) = 0",
        )?;
        let filenames = statement
          .query_map(
            params![username, direction_text(TransferDirection::Download)],
            |row| row.get::<_, Option<String>>(0),
          )?
          .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(filenames)
      })
      .await;
    let pending = match pending {
      Ok(filenames) => filenames
        .iter()
        .flatten()
        .any(|filename| remote_directory(filename) == directory),
      Err(e) => return warn!(error = %e, "directory completion not checked"),
    };
    if pending {
      return debug!(directory, "files of the remote directory still to download");
    }
    info!(directory, "remote directory downloaded");
    self.events.raise_at(
      Event::DownloadDirectoryComplete(DownloadDirectoryComplete {
        version: 0,
        local_directory_name: local.display().to_string(),
        remote_directory_name: directory,
        username: Some(record.username.clone()),
      }),
      record.ended_at.unwrap_or_else(Utc::now),
    );
  }
}
