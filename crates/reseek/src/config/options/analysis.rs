//! Audio analysis of downloaded files.

use std::time::Duration;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use crate::config::enums::SpectrogramScale;

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
  /// Number of files analyzed at once: `auto`, half of the processors and at least one, or a
  /// number from 1 to the number of processors.
  pub workers: Workers,
  /// Maximum analysis time per file. An analysis that runs longer stops and reports its
  /// partial results.
  #[serde(with = "crate::config::duration")]
  pub timeout: Duration,
  /// Maximum length, in minutes, of a file analyzed in full. Longer files are sampled.
  pub max_full_minutes: u32,
  /// Number of evenly spaced one-second windows analyzed in a sampled file.
  pub sampled_windows: u32,
  /// Disk space for the spectrograms and waveforms of analyzed files and for files uploaded for
  /// analysis. Uploads are removed first, oldest first, except the two newest; then the least
  /// recently viewed spectrograms and waveforms.
  #[cascade(kind = "size")]
  pub cache_max_bytes: bytesize::ByteSize,
  /// Largest file that can be uploaded from a browser for analysis.
  #[cascade(kind = "size")]
  pub upload_max_bytes: bytesize::ByteSize,
  /// Spectrogram display.
  pub spectrogram: Spectrogram,
}

impl Default for Analysis {
  fn default() -> Self {
    Self {
      on_download: false,
      extensions: ["flac", "wav", "aiff", "alac", "m4a", "ape", "wv", "mp3"]
        .map(str::to_string)
        .to_vec(),
      workers: Workers::Auto,
      timeout: Duration::from_secs(120),
      max_full_minutes: 15,
      sampled_windows: 240,
      cache_max_bytes: bytesize::ByteSize::mb(500),
      upload_max_bytes: bytesize::ByteSize::gb(2),
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

/// How many files are analyzed at once: half of the processors, or a count the user set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Workers {
  /// half of the processors, at least one
  #[default]
  Auto,
  /// this many
  Count(u32),
}

impl Workers {
  /// The count `auto` stands for on this machine: half of its processors, at least one.
  pub fn auto() -> u32 {
    let half = processors() / 2;
    half.max(1)
  }

  /// How many files are analyzed at once under this setting.
  pub fn count(self) -> u32 {
    match self {
      Self::Auto => Self::auto(),
      Self::Count(count) => count,
    }
  }
}

/// The processors this process may run on, as the most workers that can be set.
pub fn processors() -> u32 {
  std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX))
}

impl Serialize for Workers {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    match self {
      Self::Auto => serializer.serialize_str("auto"),
      Self::Count(count) => serializer.serialize_u32(*count),
    }
  }
}

impl<'de> Deserialize<'de> for Workers {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
      type Value = Workers;

      fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("`auto` or a whole number")
      }

      fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Workers, E> {
        u32::try_from(value)
          .map(Workers::Count)
          .map_err(|_| E::custom(format!("{value} is too large")))
      }

      fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Workers, E> {
        u64::try_from(value)
          .map_err(|_| E::custom(format!("{value} is not `auto` or a whole number")))
          .and_then(|value| self.visit_u64(value))
      }

      // the settings form sends what was typed, so a number can come as text
      fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Workers, E> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("auto") {
          return Ok(Workers::Auto);
        }
        value
          .parse()
          .map(Workers::Count)
          .map_err(|_| E::custom(format!("`{value}` is not `auto` or a whole number")))
      }
    }

    deserializer.deserialize_any(Visitor)
  }
}

impl Cascade for Workers {
  fn shape() -> cascade::Shape {
    cascade::Shape::leaf(cascade::Kind::String)
  }
}

#[cfg(test)]
mod tests {
  use super::Workers;

  #[test]
  fn workers_read_auto_or_a_count() {
    let read = |toml: &str| {
      #[derive(serde::Deserialize)]
      struct Holder {
        workers: Workers,
      }
      toml::from_str::<Holder>(toml).map(|holder| holder.workers)
    };
    assert_eq!(read("workers = \"auto\"").unwrap(), Workers::Auto);
    assert_eq!(read("workers = \"AUTO\"").unwrap(), Workers::Auto);
    assert_eq!(read("workers = 3").unwrap(), Workers::Count(3));
    assert_eq!(read("workers = \" 2 \"").unwrap(), Workers::Count(2));
    assert!(read("workers = -1").is_err());
    assert!(read("workers = \"many\"").is_err());
    assert!(Workers::auto() >= 1);
  }
}
