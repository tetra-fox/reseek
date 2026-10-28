//! The analyser's readings on generated audio: honest lossless audio, MP3 transcodes, an
//! upsample, padded bits, mixed sources and honest band-limited audio, which must never read
//! as a transcode or as mixed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use api_types::analysis::{AnalysisFlag, AnalysisVerdict};
use mp3lame_encoder::Bitrate;
use tokio_util::sync::CancellationToken;

use super::engine::{self, Settings};
use super::fixtures::{self, Audio, lo_fi, music, music_for, pink, tone};
use super::measure::Scale;
use super::reading::{self, Reading};

fn settings() -> Settings {
  Settings {
    max_full_seconds: 900.0,
    sampled_windows: 240,
    scale: Scale::Log,
    deadline: Instant::now() + Duration::from_secs(600),
  }
}

fn analysed(path: &Path) -> Reading {
  let measured = engine::run(path, &settings(), &CancellationToken::new()).unwrap();
  reading::read(&measured)
}

fn flac(dir: &Path, name: &str, audio: &Audio, bits: u32) -> PathBuf {
  let path = dir.join(name);
  fixtures::write_flac(audio, bits, false, &path);
  path
}

/// The reading must not call honest audio fake.
fn not_fake(reading: &Reading) {
  assert!(
    !matches!(
      reading.verdict,
      AnalysisVerdict::LikelyTranscode | AnalysisVerdict::MixedSources
    ),
    "{reading:#?}"
  );
}

#[test]
fn lossless_audio_reads_consistent() {
  let dir = tempfile::tempdir().unwrap();
  let reading = analysed(&flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16));
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  assert!(reading.confidence >= 0.7, "{reading:#?}");
  assert!(reading.flags.is_empty());
  assert_eq!(reading.encoder_guess, None);
  assert_eq!(reading.cutoff_hz, None);
  assert_eq!(reading.measures.bit_depth_effective, Some(16));
}

#[test]
fn mp3_transcodes_read_as_transcodes_of_their_bitrate() {
  let dir = tempfile::tempdir().unwrap();
  let source = music(1, 44_100, pink);
  for (bitrate, profile) in [
    (Bitrate::Kbps128, "mp3 ~128 kbps"),
    (Bitrate::Kbps192, "mp3 ~192 kbps"),
  ] {
    let through = fixtures::through_mp3(&source, bitrate, dir.path());
    let reading = analysed(&flac(dir.path(), "t.flac", &through, 16));
    assert_eq!(
      reading.verdict,
      AnalysisVerdict::LikelyTranscode,
      "{reading:#?}"
    );
    assert!(reading.confidence >= 0.6, "{reading:#?}");
    let guess = reading.encoder_guess.as_ref().unwrap();
    assert_eq!(guess.profile, profile, "{reading:#?}");
    assert!(reading.cutoff_hz.is_some());
  }
}

#[test]
fn a_320_kbps_transcode_is_not_read_as_consistent() {
  let dir = tempfile::tempdir().unwrap();
  let through = fixtures::through_mp3(&music(1, 44_100, pink), Bitrate::Kbps320, dir.path());
  let reading = analysed(&flac(dir.path(), "t.flac", &through, 16));
  assert_ne!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  assert!(reading.confidence <= 0.75, "{reading:#?}");
}

#[test]
fn an_mp3_is_judged_against_its_own_bitrate() {
  let dir = tempfile::tempdir().unwrap();
  let source = music(1, 44_100, pink);
  let honest = dir.path().join("honest.mp3");
  fixtures::write_mp3(&source, Bitrate::Kbps128, &honest);
  let reading = analysed(&honest);
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  // a 128 kbps MP3 encoded again at 320 kbps keeps the lower cutoff
  let through = fixtures::through_mp3(&source, Bitrate::Kbps128, dir.path());
  let inflated = dir.path().join("inflated.mp3");
  fixtures::write_mp3(&through, Bitrate::Kbps320, &inflated);
  let reading = analysed(&inflated);
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::LikelyTranscode,
    "{reading:#?}"
  );
}

#[test]
fn an_upsample_and_padded_bits_are_flagged_beside_the_verdict() {
  let dir = tempfile::tempdir().unwrap();
  let source = music(1, 44_100, pink);
  let reading = analysed(&flac(dir.path(), "up.flac", &source.resampled(96_000), 24));
  assert_eq!(reading.flags, [AnalysisFlag::Upsampled], "{reading:#?}");
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  assert!(reading.measures.upsample_ratio.unwrap() < 0.001);

  let padded = dir.path().join("padded.flac");
  fixtures::write_flac(&source, 24, true, &padded);
  let reading = analysed(&padded);
  assert_eq!(reading.flags, [AnalysisFlag::BitPadded], "{reading:#?}");
  assert_eq!(reading.measures.bit_depth_effective, Some(16));
}

#[test]
fn a_mono_source_copied_to_stereo_shows_in_the_measures() {
  let dir = tempfile::tempdir().unwrap();
  let mono = music(1, 44_100, pink).mono_to_stereo();
  let reading = analysed(&flac(dir.path(), "m.flac", &mono, 16));
  assert!(reading.measures.stereo_correlation.unwrap() > 0.9999);
  assert_eq!(reading.measures.side_energy_db, Some(-120.0));
  assert!(reading.summary.contains("mono"), "{}", reading.summary);
}

#[test]
fn a_lossy_layer_or_a_lossy_part_reads_as_mixed_sources() {
  let dir = tempfile::tempdir().unwrap();
  let stem = fixtures::through_mp3(&music(3, 44_100, pink), Bitrate::Kbps128, dir.path());
  // a full-band bed 25 dB under a stem from a 128 kbps MP3, all the way through
  let layered = stem.clone().mixed(&music(4, 44_100, pink), 0.056);
  let reading = analysed(&flac(dir.path(), "layered.flac", &layered, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::MixedSources,
    "{reading:#?}"
  );
  assert!(reading.summary.contains("step"), "{}", reading.summary);
  // the first twelve seconds from the MP3, the rest lossless
  let intro = stem.spliced(&music(1, 44_100, pink), 12);
  let reading = analysed(&flac(dir.path(), "intro.flac", &intro, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::MixedSources,
    "{reading:#?}"
  );
  assert!(
    reading.summary.starts_with("65% of the time is full-band"),
    "{}",
    reading.summary
  );
}

#[test]
fn honest_band_limited_audio_is_not_read_as_fake() {
  let dir = tempfile::tempdir().unwrap();
  let reading = analysed(&flac(dir.path(), "lofi.flac", &music(5, 44_100, lo_fi), 16));
  not_fake(&reading);
  assert!(reading.confidence <= 0.5, "{reading:#?}");
  let reading = analysed(&flac(
    dir.path(),
    "tone.flac",
    &tone(1000.0, fixtures::SECONDS),
    16,
  ));
  not_fake(&reading);
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{reading:#?}"
  );
}

#[test]
fn too_little_audio_is_inconclusive() {
  let dir = tempfile::tempdir().unwrap();
  let reading = analysed(&flac(
    dir.path(),
    "short.flac",
    &music_for(12, 1, 44_100, pink),
    16,
  ));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{reading:#?}"
  );
  assert!(reading.confidence <= 0.5);
  assert!(
    reading.summary.starts_with("Only 12 s"),
    "{}",
    reading.summary
  );
}

#[test]
fn a_long_file_is_sampled_at_windows_spread_over_it() {
  let dir = tempfile::tempdir().unwrap();
  let path = flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16);
  let settings = Settings {
    max_full_seconds: 10.0,
    sampled_windows: 20,
    ..settings()
  };
  let measured = engine::run(&path, &settings, &CancellationToken::new()).unwrap();
  assert!(measured.sampled);
  assert_eq!(measured.windows.len(), 20);
  let starts: Vec<f64> = measured.windows.iter().map(|w| w.t).collect();
  assert_eq!(starts.first(), Some(&0.0));
  assert_eq!(starts.last(), Some(&32.0), "the last whole second");
  let reading = reading::read(&measured);
  assert!(
    reading
      .limitations
      .iter()
      .any(|l| l.contains("20 one-second windows")),
    "{reading:#?}"
  );
}

#[test]
fn a_cancelled_analysis_stops_and_one_out_of_time_reports_what_it_has() {
  let dir = tempfile::tempdir().unwrap();
  let path = flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16);
  let cancelled = CancellationToken::new();
  cancelled.cancel();
  assert_eq!(
    engine::run(&path, &settings(), &cancelled).unwrap_err(),
    engine::Cancelled
  );
  let late = Settings {
    deadline: Instant::now(),
    ..settings()
  };
  let measured = engine::run(&path, &late, &CancellationToken::new()).unwrap();
  assert!(measured.windows.is_empty());
  let reading = reading::read(&measured);
  assert_eq!(reading.verdict, AnalysisVerdict::Inconclusive);
  assert!(
    reading
      .limitations
      .iter()
      .any(|l| l.contains("ran out of time")),
    "{reading:#?}"
  );
}

proptest::proptest! {
  #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

  /// Arbitrary bytes, and a real file's header followed by arbitrary bytes, are read without a
  /// panic, as a reading of some kind.
  #[test]
  fn any_bytes_are_read_without_panicking(
    bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096),
    header in 0usize..4,
  ) {
    let headers: [&[u8]; 4] = [
      b"",
      b"fLaC\x00\x00\x00\x22\x10\x00\x10\x00",
      b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x02\x00\x44\xac\x00\x00",
      b"ID3\x04\x00\x00\x00\x00\x00\x00\xff\xfb\x90\x64",
    ];
    let mut input = headers[header].to_vec();
    input.extend_from_slice(&bytes);
    let measured = engine::read_bytes(input, &settings(), &CancellationToken::new()).unwrap();
    reading::read(&measured);
  }
}

#[test]
fn a_cut_where_the_highest_bitrates_cut_reads_alike_at_44_1_and_48_khz() {
  let dir = tempfile::tempdir().unwrap();
  for rate in [44_100, 48_000] {
    for hz in [20_000.0, 20_500.0] {
      let audio = music(1, rate, fixtures::cut_at(hz));
      let reading = analysed(&flac(dir.path(), "cut.flac", &audio, 16));
      assert_eq!(
        reading.verdict,
        AnalysisVerdict::Inconclusive,
        "{rate} {hz}: {reading:#?}"
      );
      assert!(reading.encoder_guess.is_some(), "{rate} {hz}");
    }
    // a cut where a lower bitrate cuts is still a transcode at either rate
    let audio = music(1, rate, fixtures::cut_at(17_000.0));
    let reading = analysed(&flac(dir.path(), "cut.flac", &audio, 16));
    assert_eq!(
      reading.verdict,
      AnalysisVerdict::LikelyTranscode,
      "{rate}: {reading:#?}"
    );
  }
}
