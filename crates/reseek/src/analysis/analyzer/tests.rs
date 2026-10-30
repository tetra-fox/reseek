//! The analyzer's readings on generated audio: honest lossless audio, MP3 transcodes, an
//! upsample, padded bits, mixed sources and honest band-limited audio, which must never read
//! as a transcode or as mixed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use api_types::analysis::{AnalysisFlag, AnalysisVerdict};
use mp3lame_encoder::Bitrate;
use tokio_util::sync::CancellationToken;

use super::engine::{self, Settings};
use super::fixtures::{self, Audio, lo_fi, music, music_for, pink, tone};
use super::reading::{self, Reading};

fn settings() -> Settings {
  Settings {
    max_full_seconds: 900.0,
    sampled_windows: 240,
    deadline: Instant::now() + Duration::from_secs(600),
    progress: None,
  }
}

fn analyzed(path: &Path) -> Reading {
  explained(path).0
}

/// The reading of the file at `path`, with what it saw for an assertion to print.
fn explained(path: &Path) -> (Reading, String) {
  let measured = engine::run(path, &settings(), &CancellationToken::new()).unwrap();
  let reading = reading::read(&measured);
  let why = engine::explain(&measured, &reading);
  (reading, why)
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
  let reading = analyzed(&flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16));
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  assert!(reading.confidence >= 0.7, "{reading:#?}");
  assert!(reading.flags.is_empty());
  assert_eq!(reading.encoder_guess, None);
  assert_eq!(reading.cutoff_hz, None);
  assert_eq!(reading.measures.bit_depth_effective, Some(16));
}

/// An MP3 at `bitrate` written out as a FLAC reads as a transcode of that bitrate.
fn reads_as_a_transcode_of(bitrate: Bitrate, profile: &str) {
  let dir = tempfile::tempdir().unwrap();
  let through = fixtures::through_mp3(&music(1, 44_100, pink), bitrate, dir.path());
  let reading = analyzed(&flac(dir.path(), "t.flac", &through, 16));
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

#[test]
fn a_128_kbps_transcode_reads_as_a_transcode_of_its_bitrate() {
  reads_as_a_transcode_of(Bitrate::Kbps128, "mp3 ~128 kbps");
}

#[test]
fn a_192_kbps_transcode_reads_as_a_transcode_of_its_bitrate() {
  reads_as_a_transcode_of(Bitrate::Kbps192, "mp3 ~192 kbps");
}

#[test]
fn a_320_kbps_transcode_is_not_read_as_consistent() {
  let dir = tempfile::tempdir().unwrap();
  let through = fixtures::through_mp3(&music(1, 44_100, pink), Bitrate::Kbps320, dir.path());
  let reading = analyzed(&flac(dir.path(), "t.flac", &through, 16));
  assert_ne!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  assert!(reading.confidence <= 0.75, "{reading:#?}");
}

#[test]
fn an_honest_mp3_is_judged_against_its_own_bitrate() {
  let dir = tempfile::tempdir().unwrap();
  let honest = dir.path().join("honest.mp3");
  fixtures::write_mp3(&music(1, 44_100, pink), Bitrate::Kbps128, &honest);
  let reading = analyzed(&honest);
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
}

/// Content that stops at 16 kHz, where older encoders and 32 kHz sources stop too, as an MP3 at
/// `bitrate`, reads as `verdict`.
fn a_16_khz_cut_as_an_mp3_reads(bitrate: Bitrate, verdict: AnalysisVerdict) {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("cut16.mp3");
  fixtures::write_mp3(
    &music(1, 44_100, fixtures::cut_at(16_000.0)),
    bitrate,
    &path,
  );
  let (reading, why) = explained(&path);
  assert_eq!(reading.verdict, verdict, "{}: {why}", bitrate as u32);
}

#[test]
fn a_16_khz_cut_is_a_128_kbps_mp3s_own_cutoff() {
  a_16_khz_cut_as_an_mp3_reads(Bitrate::Kbps128, AnalysisVerdict::Consistent);
}

#[test]
fn a_16_khz_cut_in_a_320_kbps_mp3_fits_either_explanation() {
  a_16_khz_cut_as_an_mp3_reads(Bitrate::Kbps320, AnalysisVerdict::Inconclusive);
}

#[test]
fn a_cover_larger_than_the_audio_does_not_raise_an_mp3s_bitrate() {
  let dir = tempfile::tempdir().unwrap();
  let honest = dir.path().join("honest.mp3");
  fixtures::write_mp3(&music(1, 44_100, pink), Bitrate::Kbps128, &honest);
  fixtures::add_cover(&honest, 2_000_000);
  let measured = engine::run(&honest, &settings(), &CancellationToken::new()).unwrap();
  let kbps = measured.bitrate_kbps.unwrap();
  assert!((120..=136).contains(&kbps), "{kbps} kbps");
  let reading = reading::read(&measured);
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
}

#[test]
fn a_128_kbps_mp3_encoded_again_at_320_kbps_keeps_the_lower_cutoff() {
  let dir = tempfile::tempdir().unwrap();
  let through = fixtures::through_mp3(&music(1, 44_100, pink), Bitrate::Kbps128, dir.path());
  let inflated = dir.path().join("inflated.mp3");
  fixtures::write_mp3(&through, Bitrate::Kbps320, &inflated);
  let reading = analyzed(&inflated);
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::LikelyTranscode,
    "{reading:#?}"
  );
}

#[test]
fn an_upsample_is_flagged_beside_the_verdict() {
  let dir = tempfile::tempdir().unwrap();
  let source = music(1, 44_100, pink);
  let reading = analyzed(&flac(dir.path(), "up.flac", &source.resampled(96_000), 24));
  assert_eq!(reading.flags, [AnalysisFlag::Upsampled], "{reading:#?}");
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
  assert!(reading.measures.upsample_ratio.unwrap() < 0.001);
}

#[test]
fn padded_bits_are_flagged_beside_the_verdict() {
  let dir = tempfile::tempdir().unwrap();
  let padded = dir.path().join("padded.flac");
  fixtures::write_flac(&music(1, 44_100, pink), 24, true, &padded);
  let reading = analyzed(&padded);
  assert_eq!(reading.flags, [AnalysisFlag::BitPadded], "{reading:#?}");
  assert_eq!(reading.measures.bit_depth_effective, Some(16));
}

#[test]
fn a_mono_source_copied_to_stereo_shows_in_the_measures() {
  let dir = tempfile::tempdir().unwrap();
  let mono = music(1, 44_100, pink).mono_to_stereo();
  let reading = analyzed(&flac(dir.path(), "m.flac", &mono, 16));
  assert!(reading.measures.stereo_correlation.unwrap() > 0.9999);
  assert_eq!(reading.measures.side_energy_db, Some(-120.0));
  assert!(reading.summary.contains("mono"), "{}", reading.summary);
}

#[test]
fn a_lossy_layer_under_a_full_band_bed_reads_as_mixed_sources() {
  let dir = tempfile::tempdir().unwrap();
  let stem = fixtures::through_mp3(&music(3, 44_100, pink), Bitrate::Kbps128, dir.path());
  // a full-band bed 23 dB under a stem from a 128 kbps MP3, all the way through
  let layered = stem.mixed(&music(4, 44_100, pink), 0.071);
  let reading = analyzed(&flac(dir.path(), "layered.flac", &layered, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::MixedSources,
    "{reading:#?}"
  );
  assert!(reading.summary.contains("step"), "{}", reading.summary);
}

#[test]
fn a_lossy_part_before_lossless_audio_reads_as_mixed_sources() {
  let dir = tempfile::tempdir().unwrap();
  let stem = fixtures::through_mp3(&music(3, 44_100, pink), Bitrate::Kbps128, dir.path());
  // the first twelve seconds from the MP3, the rest lossless
  let intro = stem.spliced(&music(1, 44_100, pink), 12);
  let reading = analyzed(&flac(dir.path(), "intro.flac", &intro, 16));
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

/// A 320 kbps stem under a bed at `gain`: a step where the highest bitrates cut, which a
/// recording filtered at 20 kHz with something above it also shows.
fn a_320_kbps_stem_under_a_bed_reads_as_that_cut(gain: f32) {
  let dir = tempfile::tempdir().unwrap();
  let stem = fixtures::through_mp3(&music(3, 44_100, pink), Bitrate::Kbps320, dir.path());
  let layered = stem.mixed(&music(4, 44_100, pink), gain);
  let (reading, why) = explained(&flac(dir.path(), "layered320.flac", &layered, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{gain}: {why}"
  );
  let guess = reading.encoder_guess.as_ref().unwrap();
  assert_eq!(
    (guess.profile.as_str(), guess.confidence),
    ("mp3 ~320 kbps or aac", 0.4)
  );
  assert!(
    !reading.summary.contains("no encoder shelf"),
    "{}",
    reading.summary
  );
}

#[test]
fn a_320_kbps_stem_under_a_bed_23_db_down_reads_as_that_cut() {
  a_320_kbps_stem_under_a_bed_reads_as_that_cut(0.071);
}

#[test]
fn a_320_kbps_stem_under_a_bed_15_db_down_reads_as_that_cut() {
  a_320_kbps_stem_under_a_bed_reads_as_that_cut(0.18);
}

#[test]
fn honest_lo_fi_audio_is_not_read_as_fake() {
  let dir = tempfile::tempdir().unwrap();
  let reading = analyzed(&flac(dir.path(), "lofi.flac", &music(5, 44_100, lo_fi), 16));
  not_fake(&reading);
  assert!(reading.confidence <= 0.5, "{reading:#?}");
}

#[test]
fn a_pure_tone_is_inconclusive_not_fake() {
  let dir = tempfile::tempdir().unwrap();
  let reading = analyzed(&flac(
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

/// A synth line at `hz` low-passed at 20 kHz: a line spectrum, with gaps between its harmonics,
/// which stops where the highest bitrates cut.
fn a_synth_line_filtered_at_20_khz_reads_as_that_cut(hz: f64) {
  let dir = tempfile::tempdir().unwrap();
  let saw = fixtures::harmonics(hz, 20_000.0, 1.0);
  let reading = analyzed(&flac(dir.path(), "saw.flac", &saw, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{hz}: {reading:#?}"
  );
  assert!(reading.encoder_guess.is_some(), "{hz}");
}

#[test]
fn a_110_hz_synth_line_filtered_at_20_khz_reads_as_that_cut() {
  a_synth_line_filtered_at_20_khz_reads_as_that_cut(110.0);
}

#[test]
fn a_220_hz_synth_line_filtered_at_20_khz_reads_as_that_cut() {
  a_synth_line_filtered_at_20_khz_reads_as_that_cut(220.0);
}

#[test]
fn a_clipped_pulse_filtered_at_20_khz_reads_as_that_cut() {
  let dir = tempfile::tempdir().unwrap();
  // a pulse of equal harmonics every 100 Hz, which clips when written: its clipping puts lines
  // above the 20 kHz cut, as a recording filtered there with something above it
  let pulse = fixtures::harmonics(100.0, 20_000.0, 0.0);
  let reading = analyzed(&flac(dir.path(), "pulse.flac", &pulse, 16));
  assert!(
    reading.measures.clipped_samples_pct.unwrap() > 0.0,
    "{reading:#?}"
  );
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{reading:#?}"
  );
}

/// White noise at `dbfs`, a stand-in for tape hiss.
fn hiss(dbfs: f64) -> Audio {
  let mut floor = music(9, 44_100, |_| 1.0);
  floor.set_level(dbfs);
  floor
}

/// The reading of `audio` written as a 16-bit FLAC, which must be `verdict`.
fn reads(dir: &Path, name: &str, audio: &Audio, verdict: AnalysisVerdict) -> Reading {
  let (reading, why) = explained(&flac(dir, "floor.flac", audio, 16));
  assert_eq!(reading.verdict, verdict, "{name}: {why}");
  reading
}

/// A recording filtered at 20 kHz with hiss `db` under full scale over it stops where the highest
/// bitrates cut, which is all the analyzer can say.
fn hiss_over_a_20_khz_cut_neither_hides_nor_fakes_it(db: f64) {
  let dir = tempfile::tempdir().unwrap();
  let hissy = music(1, 44_100, fixtures::cut_at(20_000.0)).mixed(&hiss(db), 1.0);
  let reading = reads(
    dir.path(),
    &format!("hiss {db}"),
    &hissy,
    AnalysisVerdict::Inconclusive,
  );
  assert!(reading.encoder_guess.is_some());
}

#[test]
fn hiss_55_db_down_over_a_20_khz_cut_neither_hides_nor_fakes_it() {
  hiss_over_a_20_khz_cut_neither_hides_nor_fakes_it(-55.0);
}

#[test]
fn hiss_60_db_down_over_a_20_khz_cut_neither_hides_nor_fakes_it() {
  hiss_over_a_20_khz_cut_neither_hides_nor_fakes_it(-60.0);
}

/// A recording filtered at 20 kHz at `level` dBFS, so quiet that 16-bit dither is its floor,
/// still reads as that cut.
fn a_quiet_20_khz_cut_reads_as_that_cut(level: f64) {
  let dir = tempfile::tempdir().unwrap();
  let mut quiet = music(1, 44_100, fixtures::cut_at(20_000.0));
  quiet.set_level(level);
  reads(
    dir.path(),
    &format!("quiet {level}"),
    &quiet,
    AnalysisVerdict::Inconclusive,
  );
}

#[test]
fn a_20_khz_cut_50_db_down_reads_as_that_cut() {
  a_quiet_20_khz_cut_reads_as_that_cut(-50.0);
}

#[test]
fn a_20_khz_cut_55_db_down_reads_as_that_cut() {
  a_quiet_20_khz_cut_reads_as_that_cut(-55.0);
}

#[test]
fn a_128_kbps_transcode_under_hiss_is_still_a_transcode() {
  let dir = tempfile::tempdir().unwrap();
  let through = fixtures::through_mp3(&music(1, 44_100, pink), Bitrate::Kbps128, dir.path());
  let hissy = through.mixed(&hiss(-56.0), 1.0);
  reads(
    dir.path(),
    "transcode under hiss",
    &hissy,
    AnalysisVerdict::LikelyTranscode,
  );
}

#[test]
fn a_128_kbps_stem_over_a_bed_too_quiet_to_count_is_still_a_transcode() {
  let dir = tempfile::tempdir().unwrap();
  // a full-band bed 30 dB down
  let stem = fixtures::through_mp3(&music(3, 44_100, pink), Bitrate::Kbps128, dir.path());
  let bedded = stem.mixed(&music(4, 44_100, pink), 0.0316);
  reads(
    dir.path(),
    "stem over a quiet bed",
    &bedded,
    AnalysisVerdict::LikelyTranscode,
  );
}

#[test]
fn too_little_audio_is_inconclusive() {
  let dir = tempfile::tempdir().unwrap();
  let reading = analyzed(&flac(
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
    reading.summary.starts_with("Only 12.0 s"),
    "{}",
    reading.summary
  );
}

#[test]
fn a_long_file_is_sampled_at_windows_spread_over_it() {
  let dir = tempfile::tempdir().unwrap();
  let path = flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16);
  let progress = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
  let settings = Settings {
    max_full_seconds: 10.0,
    sampled_windows: 20,
    progress: Some(progress.clone()),
    ..settings()
  };
  let measured = engine::run(&path, &settings, &CancellationToken::new()).unwrap();
  assert!(measured.sampled);
  assert_eq!(measured.windows.len(), 20);
  assert_eq!(progress.load(std::sync::atomic::Ordering::Relaxed), 1000);
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
fn a_canceled_analysis_stops_and_one_out_of_time_reports_what_it_has() {
  let dir = tempfile::tempdir().unwrap();
  let path = flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16);
  let canceled = CancellationToken::new();
  canceled.cancel();
  assert_eq!(
    engine::run(&path, &settings(), &canceled).unwrap_err(),
    engine::Canceled
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

/// A cut at `hz`, where the highest bitrates cut, reads as that cut at the sample `rate`.
fn a_cut_where_the_highest_bitrates_cut_reads_as_that_cut(rate: u32, hz: f64) {
  let dir = tempfile::tempdir().unwrap();
  let audio = music(1, rate, fixtures::cut_at(hz));
  let reading = analyzed(&flac(dir.path(), "cut.flac", &audio, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{rate} {hz}: {reading:#?}"
  );
  assert!(reading.encoder_guess.is_some(), "{rate} {hz}");
}

#[test]
fn a_20_khz_cut_at_44_1_khz_reads_as_that_cut() {
  a_cut_where_the_highest_bitrates_cut_reads_as_that_cut(44_100, 20_000.0);
}

#[test]
fn a_20_5_khz_cut_at_44_1_khz_reads_as_that_cut() {
  a_cut_where_the_highest_bitrates_cut_reads_as_that_cut(44_100, 20_500.0);
}

#[test]
fn a_20_khz_cut_at_48_khz_reads_as_that_cut() {
  a_cut_where_the_highest_bitrates_cut_reads_as_that_cut(48_000, 20_000.0);
}

#[test]
fn a_20_5_khz_cut_at_48_khz_reads_as_that_cut() {
  a_cut_where_the_highest_bitrates_cut_reads_as_that_cut(48_000, 20_500.0);
}

/// A cut where a lower bitrate cuts is still a transcode at the sample `rate`.
fn a_17_khz_cut_reads_as_a_transcode(rate: u32) {
  let dir = tempfile::tempdir().unwrap();
  let audio = music(1, rate, fixtures::cut_at(17_000.0));
  let reading = analyzed(&flac(dir.path(), "cut.flac", &audio, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::LikelyTranscode,
    "{rate}: {reading:#?}"
  );
}

#[test]
fn a_17_khz_cut_at_44_1_khz_reads_as_a_transcode() {
  a_17_khz_cut_reads_as_a_transcode(44_100);
}

#[test]
fn a_17_khz_cut_at_48_khz_reads_as_a_transcode() {
  a_17_khz_cut_reads_as_a_transcode(48_000);
}

#[test]
fn a_file_without_a_stated_length_is_read_only_so_far() {
  let dir = tempfile::tempdir().unwrap();
  let path = flac(dir.path(), "a.flac", &music(1, 44_100, pink), 16);
  // the stream information's total sample count is the low 36 bits of bytes 21 to 25
  let mut bytes = std::fs::read(&path).unwrap();
  bytes[21] &= 0xf0;
  bytes[22..26].fill(0);
  std::fs::write(&path, &bytes).unwrap();
  let settings = Settings {
    max_full_seconds: 10.0,
    ..settings()
  };
  let measured = engine::run(&path, &settings, &CancellationToken::new()).unwrap();
  assert_eq!(measured.claimed.as_ref().unwrap().frames, None);
  assert!(!measured.sampled);
  assert_eq!(measured.windows.len(), 10);
  assert_eq!(measured.duration, 10.0);
  let reading = reading::read(&measured);
  assert!(
    reading
      .limitations
      .iter()
      .any(|l| l.contains("does not state its length") && l.contains("after 10 seconds")),
    "{reading:#?}"
  );
}

#[test]
fn plain_music_has_no_frame_rhythm_or_gaps_in_the_top_band() {
  let dir = tempfile::tempdir().unwrap();
  let plain = analyzed(&flac(dir.path(), "plain.flac", &music(1, 44_100, pink), 16));
  assert!(
    plain.measures.frame_artifact_score.unwrap() < 0.05,
    "{plain:#?}"
  );
  assert!(plain.measures.sfb21_holes.unwrap() < 0.01, "{plain:#?}");
}

#[test]
fn a_frame_rhythm_is_measured() {
  let dir = tempfile::tempdir().unwrap();
  // a burst at every 1152-sample frame edge
  let rhythm = music(1, 44_100, pink).with_frame_rhythm(1152);
  let reading = analyzed(&flac(dir.path(), "rhythm.flac", &rhythm, 16));
  assert!(
    reading.measures.frame_artifact_score.unwrap() >= 0.2,
    "{reading:#?}"
  );
}

#[test]
fn gaps_in_the_top_band_are_measured() {
  let dir = tempfile::tempdir().unwrap();
  // gaps 40 Hz wide every 80 Hz in 16-20 kHz
  let combed = music(1, 44_100, fixtures::combed);
  let reading = analyzed(&flac(dir.path(), "combed.flac", &combed, 16));
  assert!(reading.measures.sfb21_holes.unwrap() >= 0.1, "{reading:#?}");
}

#[test]
fn hi_res_with_a_natural_roll_off_is_not_flagged_as_upsampled() {
  let dir = tempfile::tempdir().unwrap();
  // falling 24 dB an octave from 8 kHz: almost no energy above 24 kHz, and no cliff anywhere
  let audio = music(1, 96_000, |hz| pink(hz) / (1.0 + (hz / 8_000.0).powi(4)));
  let reading = analyzed(&flac(dir.path(), "hires.flac", &audio, 24));
  assert!(
    reading.measures.upsample_ratio.unwrap() < 0.001,
    "{reading:#?}"
  );
  assert!(reading.flags.is_empty(), "{reading:#?}");
  not_fake(&reading);
}

/// A cutoff at a lower sample rate's band edge, where an MP3 at `profile` cuts too, reads as
/// either explanation and claims neither.
fn a_cutoff_at_a_lower_rates_band_edge_is_not_read_as_a_transcode(
  name: &str,
  audio: &Audio,
  profile: &str,
  rate: &str,
) {
  let dir = tempfile::tempdir().unwrap();
  let (reading, why) = explained(&flac(dir.path(), "up.flac", audio, 24));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{name}: {why}"
  );
  assert!(reading.flags.is_empty(), "{name}: {reading:#?}");
  assert_eq!(
    reading.encoder_guess.as_ref().unwrap().profile,
    profile,
    "{name}"
  );
  assert!(
    reading.summary.contains(rate) && reading.summary.contains(profile),
    "{name}: {}",
    reading.summary
  );
}

#[test]
fn audio_made_at_32_khz_and_upsampled_to_96_khz_is_not_read_as_a_transcode() {
  // it stops at 16 kHz, where older 128 kbps MP3 encoders cut too
  a_cutoff_at_a_lower_rates_band_edge_is_not_read_as_a_transcode(
    "32 to 96",
    &music(1, 32_000, pink).resampled(96_000),
    "mp3 ~128 kbps",
    "32 kHz",
  );
}

#[test]
fn audio_made_at_32_khz_and_upsampled_to_44_1_khz_is_not_read_as_a_transcode() {
  a_cutoff_at_a_lower_rates_band_edge_is_not_read_as_a_transcode(
    "32 to 44.1",
    &music(1, 32_000, pink).resampled(44_100),
    "mp3 ~128 kbps",
    "32 kHz",
  );
}

#[test]
fn audio_made_at_22_05_khz_is_not_read_as_a_transcode() {
  // it stops at 11.025 kHz, where a 64 kbps MP3 cuts
  a_cutoff_at_a_lower_rates_band_edge_is_not_read_as_a_transcode(
    "22.05 to 44.1",
    &music(1, 22_050, pink).resampled(44_100),
    "mp3 ~64 kbps",
    "22.05 kHz",
  );
}

#[test]
fn a_cut_at_16_khz_is_not_read_as_a_transcode() {
  // the old encoders' own cut, which a 32 kHz source shows too
  a_cutoff_at_a_lower_rates_band_edge_is_not_read_as_a_transcode(
    "cut at 16 kHz",
    &music(1, 44_100, fixtures::cut_at(16_000.0)),
    "mp3 ~128 kbps",
    "32 kHz",
  );
}

#[test]
fn quiet_tails_of_honest_dynamic_music_read_consistent() {
  let dir = tempfile::tempdir().unwrap();
  let honest = fixtures::classical(90);
  let reading = analyzed(&flac(dir.path(), "classical.flac", &honest, 16));
  assert_eq!(reading.verdict, AnalysisVerdict::Consistent, "{reading:#?}");
}

#[test]
fn quiet_tails_of_transcoded_dynamic_music_do_not_reach_the_quantization_noise() {
  let dir = tempfile::tempdir().unwrap();
  // its quiet tails would otherwise reach the 16-bit noise and read as full-band windows
  let through = fixtures::through_mp3(&fixtures::classical(90), Bitrate::Kbps192, dir.path());
  let reading = analyzed(&flac(dir.path(), "classical192.flac", &through, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::LikelyTranscode,
    "{reading:#?}"
  );
}

#[test]
fn a_cliff_at_an_old_equipment_rate_reads_as_its_encoder_and_says_so() {
  let dir = tempfile::tempdir().unwrap();
  // made at 37.8 kHz, as PlayStation and CD-i audio was: its cliff at 18.9 kHz is where a
  // 192 kbps MP3 cuts, and the reading cannot tell them apart
  let audio = music(1, 37_800, pink).resampled(44_100);
  let (reading, why) = explained(&flac(dir.path(), "xa.flac", &audio, 16));
  assert_eq!(reading.verdict, AnalysisVerdict::LikelyTranscode, "{why}");
  assert!(
    reading.limitations.iter().any(|l| l.contains("37.8 kHz")),
    "{reading:#?}"
  );
}

#[test]
fn a_reading_needs_thirty_whole_seconds_of_audio() {
  let dir = tempfile::tempdir().unwrap();
  let mut audio = music_for(30, 1, 44_100, pink);
  for channel in &mut audio.channels {
    channel.truncate(44_100 * 299 / 10);
  }
  let reading = analyzed(&flac(dir.path(), "short.flac", &audio, 16));
  assert_eq!(
    reading.verdict,
    AnalysisVerdict::Inconclusive,
    "{reading:#?}"
  );
}
