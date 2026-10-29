//! Audio analysis of downloaded files.

use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::SpectrogramScale;

/// The most analyses that may run at the same time.
pub const MAX_ANALYSIS_WORKERS: u32 = 4;

/// Audio analysis of downloaded files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Analysis {
  /// Analyze each finished download whose file type is listed in Extensions.
  #[cascade(env = "RESEEK_ANALYSIS_ON_DOWNLOAD", cli = "--analysis-on-download")]
  pub on_download: bool,
  /// File extensions, without the dot, that the UI offers to analyze and that are queued when a
  /// whole folder or a finished download is analyzed.
  pub extensions: Vec<String>,
  /// Number of files analyzed at once. Maximum: 4.
  pub workers: u32,
  /// Maximum analysis time per file. An analysis that runs longer stops and reports its
  /// partial results.
  #[serde(with = "humantime_serde")]
  pub timeout: Duration,
  /// Maximum length, in minutes, of a file analyzed in full. Longer files are sampled.
  pub max_full_minutes: u32,
  /// Number of evenly spaced one-second windows analyzed in a sampled file.
  pub sampled_windows: u32,
  /// Disk space for the spectrograms and waveforms of analyzed files. The least recently
  /// viewed are removed first.
  #[cascade(kind = "size")]
  pub cache_max_bytes: bytesize::ByteSize,
  /// Spectrogram display.
  pub spectrogram: Spectrogram,
}

impl Default for Analysis {
  fn default() -> Self {
    Self {
      on_download: false,
      extensions: ["flac", "wav", "aiff", "alac", "m4a", "ape", "wv"]
        .map(str::to_string)
        .to_vec(),
      workers: 1,
      timeout: Duration::from_secs(120),
      max_full_minutes: 15,
      sampled_windows: 240,
      cache_max_bytes: bytesize::ByteSize::mb(500),
      spectrogram: Spectrogram::default(),
    }
  }
}

/// Spectrogram display.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Spectrogram {
  /// Default frequency scale of the spectrogram: `log`, `linear` or `mel`. A scale picked in
  /// the browser takes precedence.
  pub scale: SpectrogramScale,
}
