//! Putting a report together from what was measured and read, and the names a file's report
//! and images are found by.

use std::time::SystemTime;

use api_types::analysis::{AnalysisWindow, AudioAnalysis, ClaimedFormat, SpectrogramLayout};
use api_types::files::FileRoot;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

use super::engine::Measured;
use super::reading::Reading;
use crate::files::LocalFile;

/// When a file last changed, as stored and hashed.
pub(super) fn modified_text(time: SystemTime) -> String {
  DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Nanos, true)
}

/// The name a file's spectrogram images are kept under: the SHA-256 of its path, size and
/// modification time, so a changed file gets new images.
pub(super) fn image_name(path: &str, size: u64, modified: &str) -> String {
  let mut hash = Sha256::new();
  hash.update(path.as_bytes());
  hash.update([0]);
  hash.update(size.to_le_bytes());
  hash.update(modified.as_bytes());
  hash
    .finalize()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

/// Where the spectrogram of the file `relative` under `root` is served.
pub(super) fn spectrogram_url(root: FileRoot, relative: &str) -> String {
  format!(
    "/api/v0/files/{root}/files/{}/analysis/spectrogram.png",
    URL_SAFE_NO_PAD.encode(relative)
  )
}

/// The report of `file` from what was measured in it and read from that.
pub(super) fn assemble(
  file: &LocalFile,
  measured: &Measured,
  reading: Reading,
  layout: Option<SpectrogramLayout>,
  analysed_at: DateTime<Utc>,
) -> AudioAnalysis {
  let claimed = match &measured.claimed {
    Some(claimed) => ClaimedFormat {
      codec: claimed.codec.clone(),
      sample_rate: claimed.sample_rate,
      bit_depth: claimed.bit_depth,
      channels: claimed.channels,
      bitrate_kbps: measured.bitrate_kbps,
    },
    None => ClaimedFormat {
      codec: "unknown".to_string(),
      sample_rate: 0,
      bit_depth: None,
      channels: 0,
      bitrate_kbps: None,
    },
  };
  let windows = measured
    .windows
    .iter()
    .map(|w| AnalysisWindow {
      t: w.t,
      cutoff_hz: w.cutoff_hz.map(|hz| hz.round() as u32),
      shelf_db: w.shelf_db.map(|db| (db * 10.0).round() / 10.0),
      rms_dbfs: (w.rms_dbfs * 10.0).round() / 10.0,
    })
    .collect();
  AudioAnalysis {
    path: file.path.to_string_lossy().into_owned(),
    size: file.size,
    analysed_at,
    duration_seconds: measured.duration,
    claimed,
    verdict: reading.verdict,
    confidence: (reading.confidence * 100.0).round() / 100.0,
    flags: reading.flags,
    encoder_guess: reading.encoder_guess,
    summary: reading.summary,
    measures: reading.measures,
    windows: Some(windows),
    spectrogram_url: spectrogram_url(file.root, &file.relative),
    spectrogram: layout,
    judgement: None,
    limitations: reading.limitations,
  }
}
