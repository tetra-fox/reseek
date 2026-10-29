//! The figures kept over every analyzed sample rather than per window: the mean, how many
//! samples are at full scale, the sums the correlation of two channels and their side energy
//! need, and a histogram of short blocks' loudness for the quietest level.

/// The length of the blocks the quietest level is measured in, in samples.
const LEVEL_BLOCK: usize = 1024;
/// The quietest block level the histogram counts, in dBFS; one bucket per whole dB up to 0.
const LEVELS_FROM_DB: f64 = -160.0;

/// The running figures over every analyzed sample.
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
        return Some(LEVELS_FROM_DB + index as f64);
      }
    }
    None
  }

  /// Adds a window's samples, one vector per channel.
  pub fn add(&mut self, channels: &[Vec<f32>]) {
    if self.levels.is_empty() {
      self.levels = vec![0; -LEVELS_FROM_DB as usize + 1];
    }
    for channel in channels {
      for sample in channel {
        let value = f64::from(*sample);
        self.samples += 1;
        self.sum += value;
        if value.abs() >= 0.9999 {
          self.clipped += 1;
        }
      }
    }
    if let [left, right] = channels {
      for (l, r) in left.iter().zip(right) {
        let (l, r) = (f64::from(*l), f64::from(*r));
        self.pairs += 1;
        self.left += l;
        self.right += r;
        self.left_squared += l * l;
        self.right_squared += r * r;
        self.product += l * r;
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
      let index = (level - LEVELS_FROM_DB).floor().clamp(0.0, -LEVELS_FROM_DB) as usize;
      self.levels[index] += 1;
    }
  }
}
