//! The findings reported beside the reading, independent of the cutoff: a file made from
//! audio at a lower sample rate, samples that use fewer bits than they claim, and two channels
//! that hold the same audio.

use api_types::analysis::{AnalysisFlag, AnalysisMeasures};

use super::decode::Claimed;
use super::measure::Window;
use super::thresholds::{CLUSTER_HZ, LOWER_RATE_EDGES, MONO_CORRELATION, MONO_SIDE_DB, profile_at};

/// What was found beside the reading.
pub(in crate::analysis) struct Findings {
  /// the flags
  pub flags: Vec<AnalysisFlag>,
  /// a sentence for the summary per finding
  pub notes: Vec<String>,
  /// the top of the band of a lower rate the content stops at with a cliff, in Hz; flagged
  /// `upsampled` only where no encoder cuts at the same frequency
  pub source_edge: Option<f64>,
}

/// The findings in a file claiming `claimed`, from its non-silent `windows`, which cut off at
/// `median`, and its `measures`.
pub(in crate::analysis) fn findings(
  claimed: &Claimed,
  windows: &[&Window],
  median: Option<f64>,
  measures: &AnalysisMeasures,
) -> Findings {
  let mut found = Findings {
    flags: Vec::new(),
    notes: Vec::new(),
    source_edge: None,
  };
  let nyquist = f64::from(claimed.sample_rate) / 2.0;
  found.source_edge = median.and_then(|m| source_edge(windows, m, nyquist));
  // where an encoder cuts at the same edge the cliff proves nothing on its own, and the
  // reading names both
  if let Some(edge) = found.source_edge.filter(|edge| profile_at(*edge).is_none()) {
    found.flags.push(AnalysisFlag::Upsampled);
    found.notes.push(format!(
      "The content stops sharply at {:.2} kHz, the upper limit of a lower sample rate. The \
       audio was probably recorded at that rate.",
      edge / 1000.0
    ));
  }
  if let (Some(claim), Some(used)) = (claimed.bit_depth, measures.bit_depth_effective)
    && claim > 16
    && used <= 16
  {
    found.flags.push(AnalysisFlag::BitPadded);
    found.notes.push(format!(
      "The samples use only {used} of their {claim} bits."
    ));
  }
  if measures
    .stereo_correlation
    .is_some_and(|c| c > MONO_CORRELATION)
    && measures.side_energy_db.is_some_and(|s| s <= MONO_SIDE_DB)
  {
    found
      .notes
      .push("The two channels are identical. The source was probably mono.".to_string());
  }
  found
}

/// The band edge of a rate lower than the file's (whose band ends at `nyquist`) the content
/// stops at with a cliff: the median cutoff is at the edge and most windows near it have a
/// sharp shelf there. A natural roll-off, however little it leaves above, has no cliff.
fn source_edge(windows: &[&Window], median: f64, nyquist: f64) -> Option<f64> {
  let edge = LOWER_RATE_EDGES
    .iter()
    .copied()
    .filter(|edge| *edge < nyquist - CLUSTER_HZ)
    .find(|edge| (median - edge).abs() <= CLUSTER_HZ)?;
  let near: Vec<&&Window> = windows
    .iter()
    .filter(|w| w.cutoff_hz.is_some_and(|c| (c - edge).abs() <= CLUSTER_HZ))
    .collect();
  let sharp = near.iter().filter(|w| w.sharp()).count();
  (near.len() * 2 > windows.len() && sharp * 2 > near.len()).then_some(edge)
}
