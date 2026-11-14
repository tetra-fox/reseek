//! What follows a download whose file arrived: the file is moved from the incomplete
//! directory to its destination under the downloads directory and the record is written as
//! succeeded with where the file is, then `DownloadFileComplete` is raised, the file is queued
//! for analysis when the options ask for it, and `DownloadDirectoryComplete` is raised too when
//! no other download from the same remote directory of the same user is left. A move that fails
//! keeps the download succeeded with the move's error, and raises `DownloadFileFailed`.

use std::path::{Path, PathBuf};

use api_types::events::{
  DownloadDirectoryComplete, DownloadFailureStage, DownloadFileComplete, DownloadFileFailed, Event,
};
use api_types::transfers::TransferDirection;
use chrono::Utc;
use rusqlite::params;
use tracing::{debug, error, info, warn};

use super::Downloads;
use super::destination::{SearchFound, Source, destination_directory, local_name, subdirectory};
use crate::config::enums::DestinationExists;
use crate::files::mode::to_unix_file_mode;
use crate::files::{WriteGuard, move_file};
use crate::safety::Os;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::listing::remote_directory;
use crate::transfers::{Record, direction_text};

impl Downloads {
  /// Moves the arrived file of `record`, succeeded but not yet written, from `incomplete` to its
  /// destination, writes the record with where the file is, and raises the events that follow.
  /// `writing` marks the incomplete file until the move has ended.
  pub(super) async fn complete(
    &self,
    mut record: Record,
    incomplete: PathBuf,
    writing: WriteGuard,
  ) {
    let options = self.config.current().values.clone();
    let destination = &options.transfers.download.destination;
    // validated when the options were loaded
    let mode = destination
      .permissions
      .mode
      .as_deref()
      .and_then(|text| to_unix_file_mode(text).ok());
    // a sender who sends files unasked names them, so theirs never replace a file
    let overwrite =
      destination.exists == DestinationExists::Overwrite && !self.pushed(&record).await;
    let (directory, name) = match self
      .destination(&record, destination.subdirectory.as_deref())
      .await
    {
      Ok(placed) => placed,
      Err(reason) => return self.move_failed(&mut record, &incomplete, reason).await,
    };
    #[cfg(test)]
    drop(self.move_gate.lock().await);
    let source = incomplete.clone();
    let target = directory.clone();
    let moved =
      tokio::task::spawn_blocking(move || move_file(&source, &target, &name, mode, overwrite))
        .await
        .map_err(|e| format!("the move did not finish: {e}"))
        .and_then(|moved| moved.map_err(|e| e.to_string()));
    drop(writing);
    let path = match moved {
      Ok(path) => path,
      Err(reason) => return self.move_failed(&mut record, &incomplete, reason).await,
    };
    record.local_path = Some(path.display().to_string());
    record.removed = self.removal_pending(record.id);
    self.save(&record, true).await;
    info!(path = %path.display(), "download moved into the downloads directory");
    if let Some(relay) = &self.relay {
      relay.download_completed(&path).await;
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
    // the options read when the completion began, before the move
    self
      .analysis
      .downloaded(
        &options.analysis,
        &record.username,
        record.id,
        record.job_id,
        &path,
      )
      .await;
    self.directory_complete(&record, &directory).await;
  }

  /// The directory under the downloads directory the file of `record` goes to, and its name
  /// there.
  async fn destination(
    &self,
    record: &Record,
    pattern: Option<&str>,
  ) -> Result<(PathBuf, String), String> {
    let search = match record.search_id {
      Some(id) => self.search_of(id).await,
      None => None,
    };
    let source = Source {
      username: &record.username,
      filename: &record.filename,
      job: record.job_id,
      destination: record.destination.as_deref(),
      folder: record.folder.as_deref(),
      search: search.as_ref(),
    };
    let relative = subdirectory(pattern, &source, Os::CURRENT)?;
    let name = local_name(&record.filename, Os::CURRENT)?;
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
    Ok((directory, name))
  }

  /// The search a download was found by, when it is still kept; a search deleted since, or one
  /// that could not be read, is none.
  async fn search_of(&self, id: uuid::Uuid) -> Option<SearchFound> {
    match self.searches.text_of(id).await {
      Ok(text) => text.map(|text| SearchFound { id, text }),
      Err(e) => {
        warn!(search = %id, error = %e, "the download's search not read");
        None
      }
    }
  }

  /// Keeps the download of `record` succeeded with `reason` as its error, since every byte
  /// arrived, and raises `DownloadFileFailed` for the move.
  async fn move_failed(&self, record: &mut Record, incomplete: &Path, reason: String) {
    error!(path = %incomplete.display(), reason, "downloaded file not moved into the downloads directory");
    record.exception = Some(reason.clone());
    record.removed = self.removal_pending(record.id);
    self.save(record, true).await;
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
        let mut statement = conn.prepare(&format!(
          "SELECT Filename FROM Transfers WHERE Username = ?1 AND Direction = ?2 AND Removed = 0 \
           AND {UNFINISHED}"
        ))?;
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
        username: record.username.clone(),
      }),
      record.ended_at.unwrap_or_else(Utc::now),
    );
  }
}
