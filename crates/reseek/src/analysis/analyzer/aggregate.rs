//! The measures of a whole file, gathered from its windows and running totals: the cutoff
//! histogram and median, percentiles of the per-window measures, and the file-wide figures.

use std::collections::BTreeMap;

use api_types::analysis::AnalysisMeasures;

use super::engine::Measured;
use super::measure::Window;

/// The width of a cutoff histogram bucket, in Hz.
const HISTOGRAM_HZ: f64 = 500.0;

/// The aggregate measures over the non-silent `windows`.
pub(in crate::analysis) fn measures(
  measured: &Measured,
  windows: &[&Window],
  upsample_ratio: Option<f64>,
) -> AnalysisMeasures {
  let mut histogram: BTreeMap<u32, u32> = BTreeMap::new();
  for cutoff in windows.iter().filter_map(|w| w.cutoff_hz) {
    let bucket = ((cutoff / HISTOGRAM_HZ).round() * HISTOGRAM_HZ) as u32;
    *histogram.entry(bucket).or_default() += 1;
  }
  let cutoffs: Vec<f64> = windows.iter().filter_map(|w| w.cutoff_hz).collect();
  let shelves: Vec<f64> = windows.iter().filter_map(|w| w.shelf_db).collect();
  let holes: Vec<f64> = windows.iter().filter_map(|w| w.holes).collect();
  let scores: Vec<f64> = windows.iter().filter_map(|w| w.frame_score).collect();
  let claimed_bits = measured.claimed.as_ref().and_then(|c| c.bit_depth);
  AnalysisMeasures {
    cutoff_hz_histogram: histogram.into_iter().map(|(hz, n)| [hz, n]).collect(),
    cutoff_median_hz: (!cutoffs.is_empty()).then(|| middle(&cutoffs).round() as u32),
    shelf_sharpness_db_p95: percentile(&shelves, 0.95),
    sfb21_holes: percentile(&holes, 0.5),
    frame_artifact_score: percentile(&scores, 0.5),
    upsample_ratio,
    bit_depth_effective: measured
      .bits_used
      .map(|used| claimed_bits.map_or(used, |claimed| used.min(claimed))),
    noise_floor_dbfs: measured.totals.noise_floor_dbfs(),
    stereo_correlation: measured.totals.stereo_correlation(),
    side_energy_db: measured.totals.side_energy_db(),
    dc_offset: measured.totals.dc_offset(),
    clipped_samples_pct: measured.totals.clipped_pct(),
  }
}

/// The share of the energy of `windows` above half the band.
pub(in crate::analysis) fn upsample_ratio(windows: &[&Window]) -> f64 {
  let (high, total) = windows
    .iter()
    .fold((0.0, 0.0), |(h, t), w| (h + w.energy.0, t + w.energy.1));
  if total > 0.0 { high / total } else { 0.0 }
}

/// The median of `values`, 0 for none.
pub(in crate::analysis) fn middle(values: &[f64]) -> f64 {
  percentile(values, 0.5).unwrap_or(0.0)
}

/// The value `fraction` of the way through `values` in order, by nearest rank.
pub(in crate::analysis) fn percentile(values: &[f64], fraction: f64) -> Option<f64> {
  if values.is_empty() {
    return None;
  }
  let mut sorted = values.to_vec();
  sorted.sort_by(f64::total_cmp);
  let rank = ((fraction * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
  Some(sorted[rank - 1])
}
