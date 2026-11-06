//! Keeping the analysis cache within its limits: the files uploaded for analysis within
//! `analysis.upload_cache_max_bytes`, and the spectrogram and waveform files of real files within
//! `analysis.cache_max_bytes`, the least recently viewed going first.

use tracing::{debug, error, warn};

use super::{Analysis, kept, store};

impl Analysis {
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
