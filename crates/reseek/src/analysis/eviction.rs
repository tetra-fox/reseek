//! Keeping the analysis cache within its limits: the files uploaded for analysis within
//! `analysis.upload_cache_max_bytes`, and the spectrogram and waveform files of real files within
//! `analysis.cache_max_bytes`, the least recently viewed going first. Reports of files that are
//! gone are removed by the cleanup.

use tracing::{debug, error, warn};

use super::{Analysis, kept, store};
use crate::db::DbError;

impl Analysis {
  /// Removes the reports of downloaded files that are no longer on disk; returns how many went.
  /// A file whose presence cannot be checked keeps its report.
  pub async fn forget_missing(&self) -> Result<usize, DbError> {
    let analyzed = store::analyzed_files(&self.db).await?;
    let gone = tokio::task::spawn_blocking(move || {
      analyzed
        .into_iter()
        .filter(|(path, _)| matches!(std::path::Path::new(path).try_exists(), Ok(false)))
        .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| DbError::Task(e.to_string()))?;
    if gone.is_empty() {
      return Ok(0);
    }
    let forgotten = store::forget(&self.db, gone).await?;
    debug!(
      reports = forgotten,
      "reports of files no longer on disk removed"
    );
    Ok(forgotten)
  }

  /// Removes the oldest uploads beyond `analysis.upload_cache_max_bytes` and the least recently
  /// viewed spectrogram and waveform files beyond `analysis.cache_max_bytes`.
  pub(super) async fn evict(&self) {
    self.evict_uploads(None).await;
    self.evict_pictures().await;
  }

  /// Removes the least recently viewed spectrogram and waveform files of real files beyond
  /// `analysis.cache_max_bytes`.
  pub(super) async fn evict_pictures(&self) {
    let limit = self
      .config
      .current()
      .values
      .analysis
      .cache_max_bytes
      .as_u64();
    let gone = match store::evict(&self.db, limit).await {
      Ok(gone) => gone,
      Err(e) => {
        error!(error = %e, "spectrograms over the limit not removed");
        return;
      }
    };
    if gone.is_empty() {
      return;
    }
    debug!(
      files = gone.len(),
      limit, "spectrograms over the limit removed"
    );
    let dir = self.dir.clone();
    let removed = tokio::task::spawn_blocking(move || {
      for name in gone {
        let path = kept::path(&dir, &name);
        if let Err(e) = std::fs::remove_file(&path)
          && e.kind() != std::io::ErrorKind::NotFound
        {
          warn!(path = %path.display(), error = %e, "spectrogram not removed");
        }
      }
    })
    .await;
    if let Err(e) = removed {
      error!(error = %e, "removing spectrograms failed");
    }
  }
}
