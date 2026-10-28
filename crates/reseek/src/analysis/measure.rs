//! What the analyser measures in each one-second window: its loudness, its spectrum averaged
//! over short overlapping frames, where that spectrum stops (the cutoff) and how steeply, gaps
//! in the band just below the cutoff, a step inside a full band, and how strongly the noise
//! repeats at a lossy codec's frame length. Running totals over the windows give the
//! file-wide figures: DC offset, clipping, how alike the channels are and the quietest level.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use super::thresholds::{
  CUTOFF_RANGE_DB, FULL_BAND, HOLE_DEPTH_DB, HOLES_FROM_HZ, HOLES_TO_HZ, INNER_SHELF_FROM_HZ,
  SHELF_GAP_HZ, SHELF_SPAN_HZ, SILENCE_DBFS,
};

/// The length of one spectrum frame in samples.
pub(super) const FFT_SIZE: usize = 4096;
/// How far one frame starts after the one before: half a frame.
const HOP: usize = FFT_SIZE / 2;
/// How many frequency rows a spectrogram column has.
pub(super) const ROWS: usize = 512;
/// The lowest frequency a log-scale spectrogram shows.
pub(super) const LOG_MIN_HZ: f64 = 20.0;
/// How wide the band is that a spectrum is smoothed over, either side of a bin.
const SMOOTH_HZ: f64 = 50.0;
/// The lowest level a spectrum is measured down to, in dB; anything quieter reads as this.
const FLOOR_DB: f64 = -200.0;
/// The quietest a spectrogram cell can be stored as, in dB.
pub(super) const COLUMN_FLOOR_DB: f64 = -160.0;
/// The length of the blocks a window's noise envelope is measured in, in samples.
const ENVELOPE_BLOCK: usize = 64;
/// Lossy codecs' frame lengths in samples, as envelope blocks: 1024 (AAC), 1152 (MP3) and 2048
/// (Vorbis long blocks).
const CODEC_PERIODS: [usize; 3] = [
  1024 / ENVELOPE_BLOCK,
  1152 / ENVELOPE_BLOCK,
  2048 / ENVELOPE_BLOCK,
];
/// The length of the blocks the quietest level is measured in, in samples.
const LEVEL_BLOCK: usize = 1024;

/// How a spectrogram's rows map to frequencies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scale {
  /// octaves take the same height
  Log,
  /// every hertz takes the same height
  Linear,
}

impl Scale {
  /// Where `hz` falls on the axis from `min` to `max`, from 0 at the bottom to 1 at the top.
  pub fn position(self, hz: f64, min: f64, max: f64) -> f64 {
    match self {
      Scale::Linear => (hz - min) / (max - min),
      Scale::Log => (hz.max(min) / min).ln() / (max / min).ln(),
    }
  }

  /// The frequency at `position` on the axis from `min` to `max`.
  pub fn frequency(self, position: f64, min: f64, max: f64) -> f64 {
    match self {
      Scale::Linear => min + position * (max - min),
      Scale::Log => min * (max / min).powf(position),
    }
  }

  /// The frequency at the bottom of the axis for a file whose band ends at `nyquist`.
  pub fn min_hz(self) -> f64 {
    match self {
      Scale::Linear => 0.0,
      Scale::Log => LOG_MIN_HZ,
    }
  }
}

/// What one window measured.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Window {
  /// where the window starts, in seconds
  pub t: f64,
  /// its loudness over every analysed channel, in dBFS
  pub rms_dbfs: f64,
  /// the highest frequency within the measured range of the loudest band, in Hz; `None` for a
  /// near-silent window or one too short for a spectrum
  pub cutoff_hz: Option<f64>,
  /// how much quieter the band just above the cutoff is than the band just below, in dB;
  /// `None` without a cutoff or when the cutoff is too near the top of the band to tell
  pub shelf_db: Option<f64>,
  /// how many bins in the upper band below the cutoff are empty per populated bin
  pub holes: Option<f64>,
  /// the steepest step inside a band that reaches the top: its frequency and depth in dB
  pub inner_shelf: Option<(f64, f64)>,
  /// how strongly the noise repeats at a lossy codec's frame length, from 0 to 1
  pub frame_score: Option<f64>,
  /// the power above half the band, and in the whole band
  pub energy: (f64, f64),
  /// the spectrogram column: each row's level, top row first, scaled into a byte from
  /// [`COLUMN_FLOOR_DB`] to 0 dB
  pub column: Vec<u8>,
}

impl Window {
  /// Whether the window is too quiet to measure a spectrum in.
  pub fn silent(&self) -> bool {
    self.rms_dbfs < SILENCE_DBFS
  }
}

/// The running figures over every analysed sample.
#[derive(Clone, Debug, Default)]
pub(super) struct Totals {
  samples: u64,
  sum: f64,
  clipped: u64,
  /// frames of two channels, with the sums correlation and the side energy need
  pairs: u64,
  left: f64,
  right: f64,
  left_squared: f64,
  right_squared: f64,
  product: f64,
  /// the count of level blocks per whole dB from -160 dBFS
  levels: Vec<u64>,
}

impl Totals {
  /// The mean sample value, relative to full scale.
  pub fn dc_offset(&self) -> Option<f64> {
    (self.samples > 0).then(|| self.sum / self.samples as f64)
  }

  /// The percentage of samples at full scale.
  pub fn clipped_pct(&self) -> Option<f64> {
    (self.samples > 0).then(|| 100.0 * self.clipped as f64 / self.samples as f64)
  }

  /// The correlation between the two channels, from -1 to 1; `None` for mono or when either
  /// channel is constant.
  pub fn stereo_correlation(&self) -> Option<f64> {
    if self.pairs == 0 {
      return None;
    }
    let n = self.pairs as f64;
    let covariance = self.product - self.left * self.right / n;
    let left = self.left_squared - self.left * self.left / n;
    let right = self.right_squared - self.right * self.right / n;
    let spread = (left * right).sqrt();
    (spread > 0.0).then(|| (covariance / spread).clamp(-1.0, 1.0))
  }

  /// The side channel's energy relative to the mid channel's, in dB, at least -120 dB.
  pub fn side_energy_db(&self) -> Option<f64> {
    if self.pairs == 0 {
      return None;
    }
    let mid = self.left_squared + self.right_squared + 2.0 * self.product;
    let side = self.left_squared + self.right_squared - 2.0 * self.product;
    (mid > 0.0).then(|| (10.0 * (side.max(0.0) / mid).log10()).max(-120.0))
  }

  /// The level of the quietest passages, in dBFS: the second percentile of the loudness of
  /// short blocks, leaving out blocks of digital silence.
  pub fn noise_floor_dbfs(&self) -> Option<f64> {
    let total: u64 = self.levels.iter().sum();
    if total == 0 {
      return None;
    }
    let wanted = total.div_ceil(50);
    let mut seen = 0;
    for (index, count) in self.levels.iter().enumerate() {
      seen += count;
      if seen >= wanted {
        return Some(COLUMN_FLOOR_DB + index as f64);
      }
    }
    None
  }
}

/// The spectrum analyser for one sample rate: the frame transform, the window function, and
/// where each spectrogram row's bins are.
pub(super) struct Analyser {
  fft: Arc<dyn RealToComplex<f32>>,
  hann: Vec<f32>,
  input: Vec<f32>,
  output: Vec<Complex<f32>>,
  scratch: Vec<Complex<f32>>,
  rate: f64,
  /// the width of one bin in Hz
  bin_hz: f64,
  /// for each spectrogram row, top first, the bins it averages
  rows: Vec<(usize, usize)>,
  /// scratch spectra: per frame, the power of every bin over the channels
  frames: Vec<Vec<f64>>,
}

impl Analyser {
  /// An analyser for audio at `rate` samples per second, drawing columns on `scale`.
  pub fn new(rate: u32, scale: Scale) -> Self {
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let hann = (0..FFT_SIZE)
      .map(|i| {
        let phase = 2.0 * std::f64::consts::PI * i as f64 / FFT_SIZE as f64;
        (0.5 - 0.5 * phase.cos()) as f32
      })
      .collect();
    let rate = f64::from(rate);
    let bin_hz = rate / FFT_SIZE as f64;
    let nyquist = rate / 2.0;
    let bins = FFT_SIZE / 2 + 1;
    let min = scale.min_hz();
    let rows = (0..ROWS)
      .map(|row| {
        let top = 1.0 - row as f64 / ROWS as f64;
        let bottom = 1.0 - (row + 1) as f64 / ROWS as f64;
        let low = scale.frequency(bottom, min, nyquist) / bin_hz;
        let high = scale.frequency(top, min, nyquist) / bin_hz;
        let first = (low.ceil() as usize).min(bins - 1);
        let last = (high.floor() as usize).min(bins - 1);
        if first <= last {
          (first, last)
        } else {
          // a row narrower than a bin takes the bin nearest its middle
          let nearest = (((low + high) / 2.0).round() as usize).min(bins - 1);
          (nearest, nearest)
        }
      })
      .collect();
    Self {
      input: fft.make_input_vec(),
      output: fft.make_output_vec(),
      scratch: vec![Complex::default(); fft.get_scratch_len()],
      fft,
      hann,
      rate,
      bin_hz,
      rows,
      frames: Vec::new(),
    }
  }

  /// Measures the window starting at `t` seconds, one sample vector per analysed channel, and
  /// adds its samples to `totals`.
  pub fn measure(&mut self, t: f64, channels: &[Vec<f32>], totals: &mut Totals) -> Window {
    let length = channels[0].len();
    add_totals(channels, totals);
    let squares: f64 = channels
      .iter()
      .flat_map(|c| c.iter())
      .map(|s| f64::from(*s) * f64::from(*s))
      .sum();
    let count = (length * channels.len()).max(1) as f64;
    let rms_dbfs = (10.0 * (squares / count).log10()).max(FLOOR_DB);
    let frame_score = frame_score(channels);
    let spectrum = self.spectrum(channels);
    let column = self.column(spectrum.as_deref());
    let mut window = Window {
      t,
      rms_dbfs,
      cutoff_hz: None,
      shelf_db: None,
      holes: None,
      inner_shelf: None,
      frame_score: None,
      energy: (0.0, 0.0),
      column,
    };
    let Some(power) = spectrum else {
      return window;
    };
    let half = (self.rate / 4.0 / self.bin_hz).ceil() as usize;
    window.energy = (power[half..].iter().sum(), power[1..].iter().sum());
    if window.silent() {
      return window;
    }
    let smoothed = smooth_db(&power, (SMOOTH_HZ / self.bin_hz).round() as usize);
    let peak = smoothed[1..].iter().copied().fold(FLOOR_DB, f64::max);
    let top = smoothed
      .iter()
      .rposition(|level| *level >= peak - CUTOFF_RANGE_DB)
      .unwrap_or(0);
    let cutoff = top as f64 * self.bin_hz;
    window.cutoff_hz = Some(cutoff);
    window.shelf_db = self.step(&smoothed, cutoff);
    window.holes = self.holes(cutoff);
    window.frame_score = frame_score;
    if cutoff >= FULL_BAND * self.rate / 2.0 {
      window.inner_shelf = self.inner_shelf(&smoothed, cutoff);
    }
    window
  }

  /// The power of every bin averaged over the window's frames and summed over its channels,
  /// relative to a full-scale sine; `None` for a window shorter than one frame. The power of
  /// each frame is kept in `frames` for [`Analyser::holes`].
  fn spectrum(&mut self, channels: &[Vec<f32>]) -> Option<Vec<f64>> {
    let length = channels[0].len();
    if length < FFT_SIZE {
      self.frames.clear();
      return None;
    }
    let count = (length - FFT_SIZE) / HOP + 1;
    let bins = FFT_SIZE / 2 + 1;
    // a full-scale sine's bin reads 1 under a Hann window
    let norm = (FFT_SIZE as f64 / 4.0).powi(2);
    self.frames.resize_with(count, Vec::new);
    let mut total = vec![0.0; bins];
    for (index, frame) in self.frames.iter_mut().enumerate() {
      frame.clear();
      frame.resize(bins, 0.0);
      let start = index * HOP;
      for channel in channels {
        let samples = &channel[start..start + FFT_SIZE];
        for ((input, sample), weight) in self.input.iter_mut().zip(samples).zip(&self.hann) {
          *input = sample * weight;
        }
        self
          .fft
          .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
          .expect("the buffers are the ones the plan made");
        for (power, value) in frame.iter_mut().zip(&self.output) {
          *power += f64::from(value.norm_sqr()) / norm;
        }
      }
      for (sum, power) in total.iter_mut().zip(frame.iter()) {
        *sum += power;
      }
    }
    for sum in &mut total {
      *sum /= count as f64;
    }
    Some(total)
  }

  /// The spectrogram column of a window's spectrum; the quietest value for a window too short
  /// for one.
  fn column(&self, spectrum: Option<&[f64]>) -> Vec<u8> {
    let Some(power) = spectrum else {
      return vec![0; ROWS];
    };
    self
      .rows
      .iter()
      .map(|&(first, last)| {
        let mean = power[first..=last].iter().sum::<f64>() / (last - first + 1) as f64;
        column_byte(db(mean))
      })
      .collect()
  }

  /// The bins from `low` to `high` Hz, clamped to the spectrum.
  fn bins(&self, low: f64, high: f64) -> std::ops::RangeInclusive<usize> {
    let last = FFT_SIZE / 2;
    let first = ((low / self.bin_hz).ceil().max(1.0) as usize).min(last);
    let end = ((high / self.bin_hz).floor().max(0.0) as usize).min(last);
    first..=end
  }

  /// The mean level of `smoothed` between `low` and `high` Hz.
  fn level(&self, smoothed: &[f64], low: f64, high: f64) -> Option<f64> {
    let range = self.bins(low, high);
    let count = range.clone().count();
    (count > 0).then(|| smoothed[range].iter().sum::<f64>() / count as f64)
  }

  /// The drop in level across `hz`: the band below it less the band above it, each
  /// [`SHELF_SPAN_HZ`] wide and [`SHELF_GAP_HZ`] away; `None` when the band above passes the
  /// top of the spectrum.
  fn step(&self, smoothed: &[f64], hz: f64) -> Option<f64> {
    let nyquist = self.rate / 2.0;
    if hz + SHELF_GAP_HZ + SHELF_SPAN_HZ > nyquist || hz < SHELF_GAP_HZ + SHELF_SPAN_HZ {
      return None;
    }
    let below = self.level(
      smoothed,
      hz - SHELF_GAP_HZ - SHELF_SPAN_HZ,
      hz - SHELF_GAP_HZ,
    )?;
    let above = self.level(
      smoothed,
      hz + SHELF_GAP_HZ,
      hz + SHELF_GAP_HZ + SHELF_SPAN_HZ,
    )?;
    Some(below - above)
  }

  /// The steepest step in the upper band of a window whose content reaches `cutoff` near the
  /// top: where it is and how deep, searched every 100 Hz.
  fn inner_shelf(&self, smoothed: &[f64], cutoff: f64) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    let mut hz = INNER_SHELF_FROM_HZ;
    while hz + SHELF_GAP_HZ + SHELF_SPAN_HZ <= cutoff {
      if let Some(drop) = self.step(smoothed, hz)
        && best.is_none_or(|(_, deepest)| drop > deepest)
      {
        best = Some((hz, drop));
      }
      hz += 100.0;
    }
    best
  }

  /// How many bins between 16 and 20 kHz, below `cutoff`, are much quieter than the bins
  /// around them, per bin that is not, averaged over the window's frames. `None` when less
  /// than a kilohertz of that band is below the cutoff.
  fn holes(&self, cutoff: f64) -> Option<f64> {
    let high = HOLES_TO_HZ.min(cutoff - SHELF_GAP_HZ * 2.0);
    if high - HOLES_FROM_HZ < 1000.0 || self.frames.is_empty() {
      return None;
    }
    let range = self.bins(HOLES_FROM_HZ, high);
    let radius = (SMOOTH_HZ * 2.0 / self.bin_hz).round() as usize;
    let mut ratios = 0.0;
    for frame in &self.frames {
      let smoothed = smooth_db(frame, radius);
      let (mut holes, mut populated) = (0u32, 0u32);
      for bin in range.clone() {
        if db(frame[bin]) < smoothed[bin] - HOLE_DEPTH_DB {
          holes += 1;
        } else {
          populated += 1;
        }
      }
      ratios += f64::from(holes) / f64::from(populated.max(1));
    }
    Some(ratios / self.frames.len() as f64)
  }
}

/// A power as dB, no lower than [`FLOOR_DB`].
fn db(power: f64) -> f64 {
  if power > 0.0 {
    (10.0 * power.log10()).max(FLOOR_DB)
  } else {
    FLOOR_DB
  }
}

/// A level in dB as a spectrogram byte: [`COLUMN_FLOOR_DB`] and below is 0, 0 dB and above
/// is 255.
pub(super) fn column_byte(level: f64) -> u8 {
  let scaled = (level - COLUMN_FLOOR_DB) / -COLUMN_FLOOR_DB * 255.0;
  scaled.round().clamp(0.0, 255.0) as u8
}

/// The level in dB of a spectrogram byte.
pub(super) fn column_db(byte: u8) -> f64 {
  COLUMN_FLOOR_DB + f64::from(byte) / 255.0 * -COLUMN_FLOOR_DB
}

/// `power` averaged over `radius` bins either side of each bin, in dB.
fn smooth_db(power: &[f64], radius: usize) -> Vec<f64> {
  let mut prefix = Vec::with_capacity(power.len() + 1);
  prefix.push(0.0);
  for value in power {
    prefix.push(prefix.last().copied().unwrap_or(0.0) + value);
  }
  (0..power.len())
    .map(|bin| {
      let first = bin.saturating_sub(radius);
      let last = (bin + radius).min(power.len() - 1);
      db((prefix[last + 1] - prefix[first]) / (last - first + 1) as f64)
    })
    .collect()
}

/// How strongly the energy of a window's high-frequency noise repeats every 1024, 1152 or
/// 2048 samples beyond what the neighbouring periods show, from 0 to 1. A lossy codec
/// quantises each frame on its own, which can leave that rhythm behind after decoding.
fn frame_score(channels: &[Vec<f32>]) -> Option<f64> {
  let length = channels[0].len();
  let blocks = length / ENVELOPE_BLOCK;
  let longest = CODEC_PERIODS.iter().max().copied().unwrap_or(0) + 3;
  if blocks < longest * 4 {
    return None;
  }
  let mut envelope = vec![0.0f64; blocks];
  for channel in channels {
    for (block, energy) in envelope.iter_mut().enumerate() {
      let start = (block * ENVELOPE_BLOCK).max(1);
      let end = (block + 1) * ENVELOPE_BLOCK;
      *energy += (start..end)
        .map(|i| {
          let difference = f64::from(channel[i]) - f64::from(channel[i - 1]);
          difference * difference
        })
        .sum::<f64>();
    }
  }
  let mean = envelope.iter().sum::<f64>() / blocks as f64;
  for energy in &mut envelope {
    *energy -= mean;
  }
  let lag = |k: usize| -> f64 { (0..blocks - k).map(|i| envelope[i] * envelope[i + k]).sum() };
  let zero = lag(0);
  if zero <= 0.0 {
    return Some(0.0);
  }
  let correlation = |k: usize| lag(k) / zero;
  // a rhythm at the period stands above every lag near it; an envelope that swings faster,
  // as a steady tone's does against the blocks, has neighbours as high
  let excess = CODEC_PERIODS
    .iter()
    .map(|&period| {
      let around = (period - 3..=period + 3)
        .filter(|k| *k != period)
        .map(correlation)
        .fold(f64::MIN, f64::max);
      correlation(period) - around
    })
    .fold(0.0, f64::max);
  Some(excess.clamp(0.0, 1.0))
}

/// Adds a window's samples to the running figures.
fn add_totals(channels: &[Vec<f32>], totals: &mut Totals) {
  if totals.levels.is_empty() {
    totals.levels = vec![0; -COLUMN_FLOOR_DB as usize + 1];
  }
  for channel in channels {
    for sample in channel {
      let value = f64::from(*sample);
      totals.samples += 1;
      totals.sum += value;
      if value.abs() >= 0.9999 {
        totals.clipped += 1;
      }
    }
  }
  if let [left, right] = channels {
    for (l, r) in left.iter().zip(right) {
      let (l, r) = (f64::from(*l), f64::from(*r));
      totals.pairs += 1;
      totals.left += l;
      totals.right += r;
      totals.left_squared += l * l;
      totals.right_squared += r * r;
      totals.product += l * r;
    }
  }
  for start in (0..channels[0].len()).step_by(LEVEL_BLOCK) {
    let end = (start + LEVEL_BLOCK).min(channels[0].len());
    let squares: f64 = channels
      .iter()
      .flat_map(|c| &c[start..end])
      .map(|s| f64::from(*s) * f64::from(*s))
      .sum();
    if squares == 0.0 {
      continue;
    }
    let level = 10.0 * (squares / ((end - start) * channels.len()) as f64).log10();
    let index = (level - COLUMN_FLOOR_DB)
      .floor()
      .clamp(0.0, -COLUMN_FLOOR_DB) as usize;
    totals.levels[index] += 1;
  }
}
