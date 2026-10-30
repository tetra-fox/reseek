//! The hint on a search result whose size does not fit its duration and declared format, such
//! as a "320 kbps" MP3 whose size works out to 128 kbps. It is a reason to look twice, never a
//! verdict: tags, cover art, silence, mono recordings and unusual encoders all move the
//! numbers, so nothing is hidden or refused for it.

use api_types::search::{File, SuspectHint, SuspectReason};

/// Files shorter than this are not judged.
const SHORTEST_SECONDS: u32 = 30;
/// A lossy file below this share of its declared bit rate is flagged.
const LOSSY_SHARE: f64 = 0.75;
/// An uncompressed file below this share of one channel of PCM is flagged.
const PCM_SHARE: f64 = 0.9;
/// A compressed lossless file below this many kbit/s is flagged.
const LOSSLESS_FLOOR: u32 = 320;
/// The sample rate and bit depth assumed for an uncompressed file that does not say.
const CD: (u32, u32) = (44_100, 16);

const LOSSY: &[&str] = &["mp3", "ogg", "opus", "m4a", "aac", "wma"];
const UNCOMPRESSED: &[&str] = &["wav", "aif", "aiff"];
const LOSSLESS: &[&str] = &["flac", "ape", "wv", "alac"];

/// The hint for `file` of the format `format`, the extension of its name, or `None` when it
/// fits or there is too little to tell.
pub fn hint(file: &File, format: &str) -> Option<SuspectHint> {
  let length = file.length.filter(|&seconds| seconds >= SHORTEST_SECONDS)?;
  if file.size == 0 {
    return None;
  }
  let implied = file.size.saturating_mul(8) / u64::from(length) / 1000;
  let implied_kbps = u32::try_from(implied).unwrap_or(u32::MAX);
  let flagged = |reason, expected_kbps| SuspectHint {
    reason,
    implied_kbps,
    expected_kbps,
  };
  if LOSSY.contains(&format) {
    let declared = file.bit_rate?;
    return (f64::from(implied_kbps) < LOSSY_SHARE * f64::from(declared))
      .then(|| flagged(SuspectReason::BelowDeclaredBitrate, declared));
  }
  if UNCOMPRESSED.contains(&format) {
    let rate = file.sample_rate.unwrap_or(CD.0);
    let depth = file.bit_depth.unwrap_or(CD.1);
    let expected = u64::from(rate) * u64::from(depth) / 1000;
    let expected = u32::try_from(expected).unwrap_or(u32::MAX);
    return (f64::from(implied_kbps) < PCM_SHARE * f64::from(expected))
      .then(|| flagged(SuspectReason::LossySizedLossless, expected));
  }
  if LOSSLESS.contains(&format) {
    return (implied_kbps < LOSSLESS_FLOOR)
      .then(|| flagged(SuspectReason::LossySizedLossless, LOSSLESS_FLOOR));
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  fn file(size: u64, length: Option<u32>) -> File {
    File {
      code: 1,
      filename: "@@a\\Album\\01".into(),
      size,
      extension: String::new(),
      bit_rate: None,
      length,
      is_variable_bit_rate: None,
      sample_rate: None,
      bit_depth: None,
      is_locked: false,
      suspect: None,
      attribute_count: 0,
      attributes: Vec::new(),
    }
  }

  #[test]
  fn a_lossy_file_well_below_its_declared_bit_rate_is_flagged() {
    // 240 s at 128 kbit/s is 3 840 000 bytes
    let mut mp3 = file(3_840_000, Some(240));
    mp3.bit_rate = Some(320);
    let hint = hint(&mp3, "mp3").unwrap();
    assert_eq!(hint.reason, SuspectReason::BelowDeclaredBitrate);
    assert_eq!((hint.implied_kbps, hint.expected_kbps), (128, 320));
    mp3.bit_rate = Some(160);
    assert_eq!(
      super::hint(&mp3, "mp3"),
      None,
      "128 is more than 0.75 of 160"
    );
    mp3.size = 30_000_000;
    mp3.bit_rate = Some(320);
    assert_eq!(
      super::hint(&mp3, "mp3"),
      None,
      "a bigger file is never flagged"
    );
    mp3.bit_rate = None;
    mp3.size = 1000;
    assert_eq!(
      super::hint(&mp3, "mp3"),
      None,
      "no declared bit rate, nothing to compare"
    );
  }

  #[test]
  fn lossless_files_are_measured_against_pcm_or_the_lossless_floor() {
    // 1411 kbit/s stereo CD audio would be 42 336 000 bytes for 240 s; one channel passes
    let mono = file(21_168_000, Some(240));
    assert_eq!(
      hint(&mono, "wav"),
      None,
      "one channel of 44.1 kHz 16 bit is enough"
    );
    let small = file(9_600_000, Some(240));
    let flagged = hint(&small, "wav").unwrap();
    assert_eq!(flagged.reason, SuspectReason::LossySizedLossless);
    assert_eq!((flagged.implied_kbps, flagged.expected_kbps), (320, 705));
    let mut hires = file(21_168_000, Some(240));
    hires.sample_rate = Some(96_000);
    hires.bit_depth = Some(24);
    assert_eq!(hint(&hires, "aiff").unwrap().expected_kbps, 2304);

    // 250 kbit/s for a FLAC is what a lossy file would weigh
    let flac = file(7_500_000, Some(240));
    let flagged = hint(&flac, "flac").unwrap();
    assert_eq!((flagged.implied_kbps, flagged.expected_kbps), (250, 320));
    let fine = file(25_000_000, Some(240));
    assert_eq!(hint(&fine, "flac"), None);
  }

  #[test]
  fn short_or_empty_or_unknown_files_are_not_judged() {
    assert_eq!(hint(&file(1000, Some(29)), "flac"), None);
    assert_eq!(hint(&file(1000, None), "flac"), None);
    assert_eq!(hint(&file(0, Some(240)), "flac"), None);
    assert_eq!(hint(&file(1000, Some(240)), "zip"), None);
    assert_eq!(hint(&file(1000, Some(240)), ""), None);
  }
}
