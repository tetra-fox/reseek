//! What one window adds to the spectrogram and the waveform a viewer draws: its spectrum over
//! the whole second, the same spectrum in short columns across the second, and the lowest and
//! highest sample in short buckets. Levels are bytes over linear frequency bins from 0 Hz to
//! half the sample rate, so a viewer can lay them out on any frequency axis.

/// How many frequency bins a spectrum is kept at, from 0 Hz to half the sample rate.
pub(super) const BINS: usize = 512;
/// How many spectrogram columns a second of audio is kept at.
pub(super) const COLUMNS_PER_SECOND: usize = 16;
/// How many waveform buckets a second of audio is kept at.
pub(super) const PEAKS_PER_SECOND: usize = 200;
/// The quietest level a byte holds, in dB relative to a full-scale sine.
pub(super) const FLOOR_DB: f64 = -160.0;
/// The loudest level a byte holds, in dB relative to a full-scale sine.
pub(super) const CEILING_DB: f64 = 0.0;

/// One window as the spectrogram and the waveform show it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shown {
  /// the spectrum over the whole window, [`BINS`] levels lowest first
  pub spectrum: Vec<u8>,
  /// the window's columns one after another, each [`BINS`] levels lowest first; a whole second
  /// has [`COLUMNS_PER_SECOND`]
  pub columns: Vec<u8>,
  /// the lowest and the highest sample of each bucket as two signed bytes; a whole second has
  /// [`PEAKS_PER_SECOND`] buckets
  pub peaks: Vec<u8>,
}

/// A level in dB as a byte: [`FLOOR_DB`] and below is 0, [`CEILING_DB`] and above is 255.
pub(super) fn level_byte(level: f64) -> u8 {
  let scaled = (level - FLOOR_DB) / (CEILING_DB - FLOOR_DB) * 255.0;
  scaled.round().clamp(0.0, 255.0) as u8
}

/// The level in dB of a byte.
pub(super) fn byte_level(byte: u8) -> f64 {
  FLOOR_DB + f64::from(byte) / 255.0 * (CEILING_DB - FLOOR_DB)
}

/// The middle of bin `bin` in Hz, for audio whose band ends at `max_hz`.
pub(super) fn bin_hz(bin: usize, max_hz: f64) -> f64 {
  (bin as f64 + 0.5) * max_hz / BINS as f64
}

/// A power as a byte, no quieter than [`FLOOR_DB`].
fn power_byte(power: f64) -> u8 {
  if power > 0.0 {
    level_byte(10.0 * power.log10())
  } else {
    0
  }
}

/// A power spectrum of a transform's bins, lowest first, as [`BINS`] bytes: each byte is the
/// mean power of the transform bins that fall in it. The bin at half the sample rate is left
/// out, so every byte averages the same number of bins.
pub(super) fn spectrum_bytes(power: &[f64]) -> Vec<u8> {
  let group = (power.len() - 1) / BINS;
  power[..group * BINS]
    .chunks_exact(group)
    .map(|bins| power_byte(bins.iter().sum::<f64>() / group as f64))
    .collect()
}

/// The columns of a window `length` samples long at `rate`, from the power spectra of its
/// frames, each `frame` samples long and `hop` apart. A column is the power mean of the frames
/// whose middle falls in it; one that no frame's middle falls in takes the frame nearest its
/// middle. A window with no frames has silent columns.
pub(super) fn columns(
  frames: &[Vec<f64>],
  length: usize,
  rate: usize,
  frame: usize,
  hop: usize,
) -> Vec<u8> {
  let count = (length * COLUMNS_PER_SECOND).div_ceil(rate).max(1);
  if frames.is_empty() {
    return vec![0; count * BINS];
  }
  let middle = |f: usize| (f * hop + frame / 2) as f64 / rate as f64;
  let column_of =
    |f: usize| ((middle(f) * COLUMNS_PER_SECOND as f64).floor() as usize).min(count - 1);
  let bins = frames[0].len();
  let mut out = Vec::with_capacity(count * BINS);
  let mut sum = vec![0.0; bins];
  for column in 0..count {
    sum.fill(0.0);
    let mut taken = 0;
    for (f, power) in frames.iter().enumerate() {
      if column_of(f) == column {
        for (total, p) in sum.iter_mut().zip(power) {
          *total += p;
        }
        taken += 1;
      }
    }
    if taken == 0 {
      let at = (column as f64 + 0.5) / COLUMNS_PER_SECOND as f64;
      let nearest = (0..frames.len())
        .min_by(|a, b| (middle(*a) - at).abs().total_cmp(&(middle(*b) - at).abs()))
        .unwrap_or(0);
      sum.copy_from_slice(&frames[nearest]);
      taken = 1;
    }
    for total in &mut sum {
      *total /= f64::from(taken);
    }
    out.extend(spectrum_bytes(&sum));
  }
  out
}

/// A sample from -1 to 1 as a signed byte from -127 to 127.
fn sample_byte(sample: f32) -> u8 {
  (sample * 127.0).round().clamp(-127.0, 127.0) as i8 as u8
}

/// The lowest and the highest sample over every channel in each bucket of a window at `rate`,
/// as two signed bytes a bucket.
pub(super) fn peaks(channels: &[Vec<f32>], rate: usize) -> Vec<u8> {
  let length = channels[0].len();
  let count = (length * PEAKS_PER_SECOND).div_ceil(rate);
  (0..count)
    .flat_map(|bucket| {
      let from = bucket * rate / PEAKS_PER_SECOND;
      let to = ((bucket + 1) * rate / PEAKS_PER_SECOND).min(length);
      let (low, high) = channels
        .iter()
        .flat_map(|channel| &channel[from.min(to)..to])
        .fold((f32::MAX, f32::MIN), |(low, high), s| {
          (low.min(*s), high.max(*s))
        });
      if low > high {
        [0, 0]
      } else {
        [sample_byte(low), sample_byte(high)]
      }
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn levels_map_to_bytes_over_the_stated_range() {
    assert_eq!(level_byte(-200.0), 0);
    assert_eq!(level_byte(FLOOR_DB), 0);
    assert_eq!(level_byte(-80.0), 128);
    assert_eq!(level_byte(0.0), 255);
    assert_eq!(level_byte(6.0), 255);
    assert_eq!(byte_level(0), FLOOR_DB);
    assert_eq!(byte_level(255), CEILING_DB);
    assert!((byte_level(level_byte(-37.3)) - -37.3).abs() < 0.32);
  }

  #[test]
  fn a_spectrum_averages_four_transform_bins_into_each_byte() {
    // 2049 bins as a 4096-sample transform gives; a full-scale sine's bin reads 1
    let mut power = vec![0.0; 2049];
    power[400] = 4.0;
    power[2048] = 1.0;
    let bytes = spectrum_bytes(&power);
    assert_eq!(bytes.len(), BINS);
    assert_eq!(bytes[100], 255, "4 over 4 bins is 0 dB");
    assert_eq!(bytes[99], 0);
    assert_eq!(bytes[511], 0, "the bin at half the rate is left out");
    // the middle of bin 100 at 44.1 kHz
    assert!((bin_hz(100, 22_050.0) - 4328.17).abs() < 0.01);
  }

  #[test]
  fn columns_take_the_frames_whose_middle_falls_in_them() {
    // a second at 44.1 kHz holds 20 frames of 4096 samples 2048 apart, their middles from
    // 46 ms to 929 ms
    let frames: Vec<Vec<f64>> = (0..20)
      .map(|f| {
        let mut power = vec![0.0; 2049];
        power[f * 4] = 4.0;
        power
      })
      .collect();
    let bytes = columns(&frames, 44_100, 44_100, 4096, 2048);
    assert_eq!(bytes.len(), COLUMNS_PER_SECOND * BINS);
    let column = |c: usize| &bytes[c * BINS..(c + 1) * BINS];
    // column 0 is the first 62.5 ms: frame 0 (46 ms) only
    assert_eq!(column(0)[0], 255);
    assert_eq!(column(0)[1], 0);
    // column 1 (62.5-125 ms) holds frame 1 (93 ms), column 2 frames 2 and 3 (139 and 186 ms)
    // at half the power each
    assert_eq!(column(1)[1], 255);
    assert_eq!(column(2)[2], level_byte(-3.0103));
    assert_eq!(column(2)[3], level_byte(-3.0103));
    // the last column (937.5 ms on) holds no frame's middle and takes the nearest, frame 19
    assert_eq!(column(15)[19], 255);
    // a short last window has as many columns as it covers
    assert_eq!(
      columns(&frames[..8], 22_050, 44_100, 4096, 2048).len(),
      8 * BINS
    );
    assert_eq!(columns(&[], 44_100, 44_100, 4096, 2048), vec![0; 16 * BINS]);
  }

  #[test]
  fn peaks_are_the_lowest_and_highest_sample_of_each_bucket() {
    let mut left = vec![0.0f32; 44_100];
    let mut right = vec![0.0f32; 44_100];
    left[10] = 0.5;
    right[300] = -1.5;
    left[44_099] = 1.0;
    let peaks = peaks(&[left, right], 44_100);
    assert_eq!(peaks.len(), PEAKS_PER_SECOND * 2);
    let bucket = |b: usize| [peaks[b * 2] as i8, peaks[b * 2 + 1] as i8];
    // a bucket is 220.5 samples: 0-219, 220-440, ...
    assert_eq!(bucket(0), [0, 64]);
    assert_eq!(bucket(1), [-127, 0], "clipped at full scale");
    assert_eq!(bucket(199), [0, 127]);
    let short = super::peaks(&[vec![0.25; 1000]], 44_100);
    assert_eq!(
      short.len(),
      5 * 2,
      "a short last window has as many buckets as it covers"
    );
  }
}
