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
  /// File extensions, without the dot, of the files the UI offers to analyse and queues when
  /// a whole folder is analysed.
  pub extensions: Vec<String>,
  /// How many files are analysed at the same time, at most 4.
  pub workers: u32,
  /// How long one file may take; an analysis that runs longer stops and reports what it
  /// measured so far.
  #[serde(with = "humantime_serde")]
  pub timeout: Duration,
  /// Files up to this many minutes long are analysed in full; longer ones are sampled.
  pub max_full_minutes: u32,
  /// How many one-second windows, spread evenly, a sampled file is analysed at.
  pub sampled_windows: u32,
  /// How much disk the spectrograms and waveforms of analyzed files may take; the least recently
  /// viewed go first.
  #[cascade(kind = "size")]
  pub cache_max_bytes: bytesize::ByteSize,
  /// How the spectrogram is shown.
  pub spectrogram: Spectrogram,
}

impl Default for Analysis {
  fn default() -> Self {
    Self {
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

/// How the spectrogram is shown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Spectrogram {
  /// The frequency axis the browser shows the spectrogram on until its user picks another:
  /// `log`, `linear` or `mel`.
  pub scale: SpectrogramScale,
}
