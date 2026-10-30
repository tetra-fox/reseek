//! The audio analysis options: how many run at once, how long one may take, and how a long
//! file is sampled.

use std::time::Duration;

use super::Report;
use super::fields::{at_least, duration_at_least, range};
use crate::config::{Analysis, MAX_ANALYSIS_WORKERS};

/// The fewest windows a sampled file is analyzed at: fewer cannot give a reading.
const MIN_SAMPLED_WINDOWS: u32 = 20;
/// The most windows a sampled file is analyzed at: each window's spectrogram and waveform are
/// kept on disk and held while the file is analyzed.
const MAX_SAMPLED_WINDOWS: u32 = 2000;

pub(super) fn check(analysis: &Analysis, report: &mut Report) {
  report.check(
    "analysis.workers",
    range(
      "analysis.workers",
      analysis.workers,
      1,
      MAX_ANALYSIS_WORKERS,
    ),
  );
  report.check(
    "analysis.timeout",
    duration_at_least("analysis.timeout", analysis.timeout, Duration::from_secs(1)),
  );
  report.check(
    "analysis.max_full_minutes",
    at_least("analysis.max_full_minutes", analysis.max_full_minutes, 1),
  );
  report.check(
    "analysis.sampled_windows",
    range(
      "analysis.sampled_windows",
      analysis.sampled_windows,
      MIN_SAMPLED_WINDOWS,
      MAX_SAMPLED_WINDOWS,
    ),
  );
  for extension in &analysis.extensions {
    if extension.is_empty() || extension.contains(['.', '/', '\\']) {
      report.add(
        "analysis.extensions",
        format!("'{extension}' must be a file extension without the leading dot."),
      );
    }
  }
}
