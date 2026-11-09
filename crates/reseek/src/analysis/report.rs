//! Putting a report together from what was measured and read, and the names a file's report
//! and its spectrogram and waveform are found by.

use std::time::SystemTime;

use api_types::analysis::{
  AnalysisWindow, AudioAnalysis, ClaimedFormat, GuideKind, GuideLine, SpectrogramLayout,
  SpectrogramScale,
};
use api_types::files::FileRoot;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

use super::analyzer::engine::Measured;
use super::analyzer::picture::{BINS, COLUMNS_PER_SECOND, PEAKS_PER_SECOND};
use super::analyzer::reading::Reading;
use super::analyzer::thresholds::guides;
use super::tiles::{TILE_SECONDS, tile_count};
use crate::files::LocalFile;

/// When a file last changed, as stored and hashed.
pub(super) fn modified_text(time: SystemTime) -> String {
  DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Nanos, true)
}

/// The name a file's spectrogram and waveform are kept under: the SHA-256 of its path, size and
/// modification time, so a changed file gets a new one.
pub(super) fn kept_name(path: &str, size: u64, modified: &str) -> String {
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

/// Where the overview of the spectrogram of the file `relative` under `root` is served.
pub(super) fn spectrogram_url(root: FileRoot, relative: &str) -> String {
  format!(
    "/api/v0/files/{root}/files/{}/analysis/spectrogram",
    URL_SAFE_NO_PAD.encode(relative)
  )
}

impl From<crate::config::enums::SpectrogramScale> for SpectrogramScale {
  fn from(scale: crate::config::enums::SpectrogramScale) -> Self {
    match scale {
      crate::config::enums::SpectrogramScale::Linear => Self::Linear,
      crate::config::enums::SpectrogramScale::Log => Self::Log,
      crate::config::enums::SpectrogramScale::Mel => Self::Mel,
    }
  }
}

/// How the spectrogram of audio `duration` seconds long whose band ends at `max_hz` is served,
/// with a guide line at each common encoder cutoff below the top of the band, one at the top,
/// and one at the analyzer's cutoff when it found one.
pub(super) fn layout(
  duration: f64,
  max_hz: f64,
  sampled: bool,
  cutoff_hz: Option<f64>,
  scale: SpectrogramScale,
) -> SpectrogramLayout {
  let mut guides: Vec<GuideLine> = guides()
    .filter(|(hz, _)| *hz < max_hz)
    .map(|(hz, label)| GuideLine {
      hz,
      label: label.to_string(),
      kind: GuideKind::Encoder,
    })
    .collect();
  guides.push(GuideLine {
    hz: max_hz,
    label: "file limit".to_string(),
    kind: GuideKind::FileLimit,
  });
  if let Some(cutoff) = cutoff_hz {
    guides.push(GuideLine {
      hz: cutoff,
      label: "analyzer's cutoff".to_string(),
      kind: GuideKind::Cutoff,
    });
  }
  guides.sort_by(|a, b| a.hz.total_cmp(&b.hz));
  SpectrogramLayout {
    scale,
    max_hz,
    duration_seconds: duration,
    sampled,
    guides,
    bins: BINS as u32,
    columns_per_second: COLUMNS_PER_SECOND as u32,
    peaks_per_second: PEAKS_PER_SECOND as u32,
    tile_seconds: TILE_SECONDS as u32,
    // a tile is asked for by a u32, so a length past the last one numbered has no more
    tiles: u32::try_from(tile_count(duration)).unwrap_or(u32::MAX),
  }
}

/// The report of `file` from what was measured in it and read from that.
pub(super) fn assemble(
  file: &LocalFile,
  measured: &Measured,
  reading: Reading,
  layout: Option<SpectrogramLayout>,
  analyzed_at: DateTime<Utc>,
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
    root: file.root,
    relative_path: file.relative.clone(),
    transfer_id: file.transfer_id,
    size: file.size,
    analyzed_at,
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
    judgment: None,
    limitations: reading.limitations,
    stale: false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_encoder_guides_sit_at_the_cutoffs_the_analyzer_names() {
    let sent = layout(60.0, 22_050.0, false, None, SpectrogramScale::Linear);
    let encoders: Vec<(f64, &str)> = sent
      .guides
      .iter()
      .filter(|guide| guide.kind == GuideKind::Encoder)
      .map(|guide| (guide.hz, guide.label.as_str()))
      .collect();
    assert_eq!(
      encoders,
      [
        (16_000.0, "MP3 128k"),
        (18_600.0, "MP3 192k"),
        (19_530.0, "MP3 224-256k"),
        (20_000.0, "MP3 320k / AAC"),
      ]
    );
  }
}
