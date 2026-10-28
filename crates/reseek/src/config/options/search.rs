//! Options for the searches made from the UI.

use cascade::Cascade;
use serde::{Deserialize, Serialize};

/// Searches made from the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Search {
  /// Named result filters the search page offers, each a filter string such as `type:flac`.
  /// The list replaces the defaults as a whole, so a default taken off it stays off.
  pub filter_presets: Vec<FilterPreset>,
}

/// A result filter saved under a name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct FilterPreset {
  /// What the presets menu calls it.
  pub name: String,
  /// The filter it puts in the filter box.
  pub filter: String,
}

impl FilterPreset {
  fn new(name: &str, filter: &str) -> Self {
    Self {
      name: name.to_string(),
      filter: filter.to_string(),
    }
  }
}

impl Default for Search {
  fn default() -> Self {
    Self {
      filter_presets: vec![
        FilterPreset::new("Lossless", "type:flac|alac|ape|wav"),
        FilterPreset::new("FLAC", "type:flac"),
        FilterPreset::new("24-bit", "minbd:24"),
        FilterPreset::new("MP3 320", "type:mp3 bitrate:320"),
        FilterPreset::new("AAC/M4A", "type:aac|m4a"),
        FilterPreset::new("Opus/Vorbis", "type:opus|ogg"),
        FilterPreset::new("No small files", "size:1m"),
      ],
    }
  }
}
