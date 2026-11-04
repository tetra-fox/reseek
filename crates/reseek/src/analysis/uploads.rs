//! Files uploaded from a browser for one analysis, kept apart from everything a user owns: each
//! lives in a folder of its own under the analysis directory with its spectrogram and waveform,
//! its report is never listed, it takes no judgment and raises no event. They share
//! `analysis.cache_max_bytes` with the pictures of real files and go first when the cache is
//! full, except the two newest, which the Analysis page can be showing side by side. A restart
//! removes them all.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use api_types::analysis::AnalysisUpload;
use api_types::files::FileRoot;
use axum::body::{Body, HttpBody};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Analysis, store};
use crate::api::error::ApiError;
use crate::safety::{Os, creatable, file_name_safely};
use crate::sync::lock;

/// The folder under the analysis directory that holds the uploads.
const FOLDER: &str = "uploads";
/// The name an upload has until all of it has arrived.
const PARTIAL: &str = "upload.part";
/// How many of the newest uploads stay whatever they take, one for each slot of the page.
const ALWAYS_KEPT: usize = 2;

/// Where uploads are held under the data directory.
pub fn uploads_dir(data: &Path) -> PathBuf {
  data.join("analysis").join(FOLDER)
}

/// The name an upload's pictures are kept under, in its own folder so they go with it. The
/// upload's path is `<id>/<name>`; `hash` is the name a report's pictures take elsewhere.
pub(super) fn kept_name(relative: &str, hash: &str) -> String {
  let id = relative.split('/').next().unwrap_or_default();
  format!("{FOLDER}/{id}/{hash}")
}

/// One upload the cache holds.
#[derive(Debug, PartialEq, Eq)]
struct Held {
  /// its path under the temporary root
  relative: String,
  bytes: u64,
  /// what its spectrogram and waveform take
  pictures: u64,
}

/// The uploads held, least recently used first. They go with a restart, so nothing but this
/// knows them.
#[derive(Debug, Default)]
pub(super) struct Uploads {
  held: Vec<Held>,
}

impl Uploads {
  fn add(&mut self, relative: String, bytes: u64) {
    self.held.push(Held {
      relative,
      bytes,
      pictures: 0,
    });
  }

  /// Marks the upload at `relative` as the most recently used.
  pub fn touch(&mut self, relative: &str) {
    if let Some(at) = self.held.iter().position(|h| h.relative == relative) {
      let held = self.held.remove(at);
      self.held.push(held);
    }
  }

  /// Records what the upload's spectrogram and waveform take.
  pub fn drawn(&mut self, relative: &str, pictures: u64) {
    if let Some(held) = self.held.iter_mut().find(|h| h.relative == relative) {
      held.pictures = pictures;
    }
  }

  /// What the uploads take with their pictures.
  pub fn bytes(&self) -> u64 {
    self.held.iter().map(|h| h.bytes + h.pictures).sum()
  }

  /// Takes the oldest uploads off, the newest two left, until they and `others` bytes fit
  /// `limit`; returns the paths of those taken.
  pub fn over(&mut self, limit: u64, others: u64) -> Vec<String> {
    let mut total = self.bytes() + others;
    let mut gone = Vec::new();
    while total > limit && self.held.len() > ALWAYS_KEPT {
      let held = self.held.remove(0);
      total -= held.bytes + held.pictures;
      gone.push(held.relative);
    }
    gone
  }
}

/// Removes an upload's folder when dropped, so a transfer cut short leaves nothing behind.
struct Partial(Option<PathBuf>);

impl Partial {
  /// The upload arrived whole: its folder stays.
  fn keep(mut self) {
    self.0 = None;
  }
}

impl Drop for Partial {
  fn drop(&mut self) {
    let Some(dir) = self.0.take() else {
      return;
    };
    // a request is dropped from the runtime, which must not wait for the disk
    tokio::task::spawn_blocking(move || remove_folder(&dir));
  }
}

/// Removes an upload's folder, which may be gone already.
fn remove_folder(dir: &Path) {
  if let Err(e) = std::fs::remove_dir_all(dir)
    && e.kind() != std::io::ErrorKind::NotFound
  {
    warn!(path = %dir.display(), error = %e, "an uploaded file not removed");
  }
}

impl Analysis {
  /// Takes `body`, a file named `name`, into the cache, no more of it than
  /// `analysis.upload_max_bytes`.
  pub(super) async fn receive(
    &self,
    by: &str,
    name: &str,
    body: Body,
  ) -> Result<AnalysisUpload, ApiError> {
    let limit = self.config.current().values.analysis.upload_max_bytes;
    let too_large = || {
      ApiError::too_large(format!(
        "The file is larger than the upload limit of {limit}"
      ))
    };
    if body.size_hint().lower() > limit.as_u64() {
      return Err(too_large());
    }
    // named as the system creates it, so the rename after the last byte cannot refuse it
    let name = file_name_safely(name, true, Os::CURRENT)
      .filter(|name| !name.is_empty())
      .map(|name| creatable(&name, Os::CURRENT))
      .ok_or_else(|| ApiError::bad_request("The file has no name"))?;
    let id = Uuid::new_v4();
    let dir = self.uploads.join(id.to_string());
    tokio::fs::create_dir_all(&dir)
      .await
      .map_err(ApiError::internal)?;
    let partial = Partial(Some(dir.clone()));
    let part = dir.join(PARTIAL);
    let mut file = tokio::fs::File::create(&part)
      .await
      .map_err(ApiError::internal)?;
    let mut size = 0;
    let mut stream = body.into_data_stream();
    while let Some(piece) = stream.next().await {
      let piece = piece.map_err(|e| {
        debug!(error = %e, "an upload stopped before its end");
        ApiError::bad_request("The upload stopped before its end")
      })?;
      size += piece.len() as u64;
      if size > limit.as_u64() {
        return Err(too_large());
      }
      file.write_all(&piece).await.map_err(ApiError::internal)?;
    }
    file.flush().await.map_err(ApiError::internal)?;
    drop(file);
    tokio::fs::rename(&part, dir.join(&name))
      .await
      .map_err(ApiError::internal)?;
    partial.keep();
    let path = format!("{id}/{name}");
    lock(&self.held).add(path.clone(), size);
    info!(by, bytes = size, "a file uploaded for analysis");
    self.evict().await;
    Ok(AnalysisUpload {
      root: FileRoot::Temporary,
      path,
      size,
    })
  }

  /// Takes the oldest uploads out of the cache until it fits `limit` with `others` bytes of other
  /// files, and gives what the uploads left take.
  pub(super) async fn evict_uploads(&self, limit: u64, others: u64) -> u64 {
    let (gone, left) = {
      let mut held = lock(&self.held);
      let gone = held.over(limit, others);
      (gone, held.bytes())
    };
    if gone.is_empty() {
      return left;
    }
    debug!(files = gone.len(), limit, "uploads over the limit removed");
    for relative in &gone {
      if let Err(e) = store::forget_temporary(&self.db, Some(relative)).await {
        warn!(error = %e, "the report of a removed upload not removed");
      }
    }
    let folders: HashSet<PathBuf> = gone
      .iter()
      .filter_map(|relative| relative.split('/').next())
      .map(|id| self.uploads.join(id))
      .collect();
    let removed =
      tokio::task::spawn_blocking(move || folders.iter().for_each(|f| remove_folder(f)));
    if let Err(e) = removed.await {
      warn!(error = %e, "removing uploads failed");
    }
    left
  }

  /// Removes every upload and its report, as nothing can name one after a restart.
  pub(super) async fn clear_uploads(&self) -> Result<(), crate::db::DbError> {
    let dir = self.uploads.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || remove_folder(&dir)).await {
      warn!(error = %e, "removing the uploads of the last run failed");
    }
    store::forget_temporary(&self.db, None).await
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn held(uploads: &Uploads) -> Vec<&str> {
    uploads.held.iter().map(|h| h.relative.as_str()).collect()
  }

  #[test]
  fn uploads_go_oldest_first_and_the_two_newest_stay_over_the_limit() {
    let mut uploads = Uploads::default();
    for name in ["a", "b", "c", "d"] {
      uploads.add(name.to_string(), 100);
    }
    uploads.drawn("d", 10);
    assert_eq!(uploads.bytes(), 410);
    assert_eq!(uploads.over(215, 0), ["a", "b"]);
    assert_eq!(held(&uploads), ["c", "d"]);
    assert!(uploads.over(0, 0).is_empty(), "two stay whatever they take");
  }

  #[test]
  fn a_viewed_upload_counts_as_new_and_other_files_take_room() {
    let mut uploads = Uploads::default();
    for name in ["a", "b", "c"] {
      uploads.add(name.to_string(), 100);
    }
    uploads.touch("a");
    assert_eq!(uploads.over(300, 50), ["b"]);
    assert_eq!(held(&uploads), ["c", "a"]);
  }
}
