//! One analysis from file to measures: the file is decoded a window at a time, in full or, for
//! a long file, at windows spread evenly over it, and each window is measured. It runs on a
//! blocking thread, stops promptly when canceled, and stops with what it has when its time
//! is up.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use symphonia::core::formats::probe::Hint;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::decode::{Claimed, Decoder, Filled};
use super::measure::{Analyzer, FFT_SIZE, Window};
use super::totals::Totals;

/// How one analysis runs.
#[derive(Clone, Debug)]
pub(in crate::analysis) struct Settings {
  /// files up to this many seconds long are analyzed in full
  pub max_full_seconds: f64,
  /// how many windows a longer file is analyzed at
  pub sampled_windows: usize,
  /// when the analysis stops with what it has
  pub deadline: Instant,
  /// where the analysis reports how far it is, in thousandths of the file
  pub progress: Option<Arc<AtomicU32>>,
}

impl Settings {
  /// Says the analysis is `done` of the way through, from 0 to 1.
  fn report(&self, done: f64) {
    if let Some(progress) = &self.progress {
      progress.store((done.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }
  }
}

/// What an analysis measured.
#[derive(Debug)]
pub(in crate::analysis) struct Measured {
  /// the format the file claims, `None` when it could not be opened as audio
  pub claimed: Option<Claimed>,
  /// how long the audio is, in seconds
  pub duration: f64,
  /// a lossy file's bitrate in kbit/s, from its decoded packets
  pub bitrate_kbps: Option<u32>,
  /// every analyzed window, in time order
  pub windows: Vec<Window>,
  /// the figures over every analyzed sample
  pub totals: Totals,
  /// the bits per sample the integer samples use
  pub bits_used: Option<u32>,
  /// whether only windows spread over the file were analyzed
  pub sampled: bool,
  /// the channels left out of a file with more than two
  pub channels_left_out: u32,
  /// what made the analysis stop early or skip audio, in words for the report
  pub problems: Vec<String>,
}

/// The analysis was canceled; nothing is reported.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::analysis) struct Canceled;

/// Analyzes the file at `path`.
pub(in crate::analysis) fn run(
  path: &Path,
  settings: &Settings,
  cancel: &CancellationToken,
) -> Result<Measured, Canceled> {
  match Decoder::open(path) {
    Ok(decoder) => measure(decoder, settings, cancel),
    Err(problem) => {
      debug!(path = %path.display(), problem, "audio not opened");
      Ok(unreadable(problem))
    }
  }
}

/// Analyzes audio held in memory, as a file's would be.
pub(in crate::analysis) fn read_bytes(
  bytes: Vec<u8>,
  settings: &Settings,
  cancel: &CancellationToken,
) -> Result<Measured, Canceled> {
  let source = Box::new(std::io::Cursor::new(bytes));
  match Decoder::from_source(source, &Hint::new()) {
    Ok(decoder) => measure(decoder, settings, cancel),
    Err(problem) => Ok(unreadable(problem)),
  }
}

/// What is reported for audio that could not be opened.
pub(in crate::analysis) fn unreadable(problem: String) -> Measured {
  Measured {
    claimed: None,
    duration: 0.0,
    bitrate_kbps: None,
    windows: Vec::new(),
    totals: Totals::default(),
    bits_used: None,
    sampled: false,
    channels_left_out: 0,
    problems: vec![problem],
  }
}

/// Measures the audio `decoder` reads.
pub(in crate::analysis) fn measure(
  mut decoder: Decoder,
  settings: &Settings,
  cancel: &CancellationToken,
) -> Result<Measured, Canceled> {
  let claimed = decoder.claimed.clone();
  let rate = claimed.sample_rate as usize;
  let stated = claimed.frames.map(|frames| frames as f64 / rate as f64);
  let mut analyzer = Analyzer::new(claimed.sample_rate, claimed.bit_depth);
  let mut buffer = vec![Vec::with_capacity(rate); decoder.channels];
  let mut measured = Measured {
    duration: stated.unwrap_or(0.0),
    bitrate_kbps: None,
    channels_left_out: claimed.channels.saturating_sub(decoder.channels as u32),
    claimed: Some(claimed),
    windows: Vec::new(),
    totals: Totals::default(),
    bits_used: None,
    sampled: false,
    problems: Vec::new(),
  };
  let picks = stated
    .filter(|seconds| *seconds > settings.max_full_seconds)
    .map(|seconds| picks(seconds, settings.sampled_windows));
  match picks {
    Some(picks) => {
      measured.sampled = true;
      sampled(
        &mut decoder,
        &mut analyzer,
        &mut buffer,
        &picks,
        settings,
        cancel,
        &mut measured,
      )?;
    }
    None => {
      // reading in full stops at max_full_seconds, whatever length the file states, if any
      let longest = settings.max_full_seconds.ceil() as u64;
      let decoded = whole(
        &mut decoder,
        &mut analyzer,
        &mut buffer,
        Read::Upto(longest),
        settings,
        cancel,
        &mut measured,
      )?;
      if stated.is_none() {
        measured.duration = decoded;
      }
    }
  }
  settings.report(1.0);
  measured.bits_used = decoder.bits_used();
  if measured.claimed.as_ref().is_some_and(|c| !c.lossless) {
    measured.bitrate_kbps = decoder.bitrate_kbps();
  }
  if decoder.skipped > 0 {
    measured.problems.push(format!(
      "{} packets could not be decoded and were skipped",
      decoder.skipped
    ));
  }
  if let Some(stopped) = decoder.stopped.take() {
    measured.problems.push(stopped);
  }
  debug!(
    windows = measured.windows.len(),
    sampled = measured.sampled,
    skipped = decoder.skipped,
    "audio measured"
  );
  Ok(measured)
}

/// The whole seconds `count` windows spread evenly over `seconds` of audio start at.
fn picks(seconds: f64, count: usize) -> BTreeSet<u64> {
  let last = (seconds - 1.0).max(0.0);
  (0..count)
    .map(|i| {
      let at = last * i as f64 / (count.max(2) - 1) as f64;
      at.floor() as u64
    })
    .collect()
}

/// Whether the analysis is to stop: an error when it was canceled, true when its time is up,
/// which is noted once in `measured`.
fn stop(
  settings: &Settings,
  cancel: &CancellationToken,
  measured: &mut Measured,
) -> Result<bool, Canceled> {
  if cancel.is_cancelled() {
    return Err(Canceled);
  }
  if Instant::now() >= settings.deadline {
    debug!(windows = measured.windows.len(), "analysis out of time");
    measured
      .problems
      .push("the analysis ran out of time and covers only the part read until then".to_string());
    return Ok(true);
  }
  Ok(false)
}

/// `seconds` in words, as minutes when they are whole minutes.
fn minutes(seconds: u64) -> String {
  match seconds {
    60 => "a minute".to_string(),
    s if s % 60 == 0 => format!("{} minutes", s / 60),
    s => format!("{s} seconds"),
  }
}

/// Whether a window filled as `filled` is worth measuring: a whole second, or a last part of at
/// least half a second and one spectrum frame.
fn usable(filled: &Filled, rate: usize) -> bool {
  match filled {
    Filled::Full => true,
    Filled::End(length) => *length >= FFT_SIZE.min(rate) && length * 2 >= rate,
  }
}

/// Which windows [`whole`] measures.
enum Read<'a> {
  /// every window, up to this many
  Upto(u64),
  /// the windows starting at these seconds
  Only(&'a BTreeSet<u64>),
}

/// Reads windows one after another from where the decoder is and measures those `read` asks
/// for. Returns the seconds read.
fn whole(
  decoder: &mut Decoder,
  analyzer: &mut Analyzer,
  buffer: &mut [Vec<f32>],
  read: Read<'_>,
  settings: &Settings,
  cancel: &CancellationToken,
  measured: &mut Measured,
) -> Result<f64, Canceled> {
  let rate = decoder.claimed.sample_rate as usize;
  let stated = decoder
    .claimed
    .frames
    .map(|frames| frames as f64 / rate as f64);
  let expected = match read {
    Read::Upto(longest) => stated.map_or(longest as f64, |s| s.min(longest as f64)),
    Read::Only(picks) => picks.last().map_or(1.0, |last| (last + 1) as f64),
  };
  let mut second = 0u64;
  loop {
    settings.report(second as f64 / expected.max(1.0));
    if stop(settings, cancel, measured)? {
      return Ok(second as f64);
    }
    let filled = decoder.fill(buffer, rate);
    if let Read::Upto(longest) = read
      && second >= longest
    {
      if filled == Filled::End(0) {
        return Ok(second as f64);
      }
      debug!(
        seconds = second,
        "reading in full stopped at the longest file read so"
      );
      measured.problems.push(match decoder.claimed.frames {
        None => format!(
          "The file does not state its length, so the analysis stopped after {}.",
          minutes(second)
        ),
        Some(_) => format!(
          "The file runs longer than it states, so the analysis stopped after {}.",
          minutes(second)
        ),
      });
      return Ok(second as f64);
    }
    let length = match filled {
      Filled::Full => rate,
      Filled::End(length) => length,
    };
    let wanted = match read {
      Read::Upto(_) => true,
      Read::Only(picks) => picks.contains(&second),
    };
    if usable(&filled, rate) && wanted {
      let window = analyzer.measure(second as f64, buffer, &mut measured.totals);
      measured.windows.push(window);
    }
    if filled != Filled::Full {
      return Ok(second as f64 + length as f64 / rate as f64);
    }
    second += 1;
  }
}

/// Measures one window at each of `picks`, seeking to each; when the file cannot be sought
/// in, reads it from the start and measures the picked seconds as they come.
fn sampled(
  decoder: &mut Decoder,
  analyzer: &mut Analyzer,
  buffer: &mut [Vec<f32>],
  picks: &BTreeSet<u64>,
  settings: &Settings,
  cancel: &CancellationToken,
  measured: &mut Measured,
) -> Result<(), Canceled> {
  let rate = decoder.claimed.sample_rate as usize;
  for (index, &second) in picks.iter().enumerate() {
    settings.report(index as f64 / picks.len() as f64);
    if stop(settings, cancel, measured)? {
      return Ok(());
    }
    if let Err(problem) = decoder.seek(second as f64) {
      if index > 0 {
        debug!(problem, second, "seeking failed part way, sampling stopped");
        measured.problems.push(problem);
        return Ok(());
      }
      debug!(problem, "the file cannot be sought in, reading it through");
      whole(
        decoder,
        analyzer,
        buffer,
        Read::Only(picks),
        settings,
        cancel,
        measured,
      )?;
      return Ok(());
    }
    if !usable(&decoder.fill(buffer, rate), rate) {
      continue;
    }
    let window = analyzer.measure(second as f64, buffer, &mut measured.totals);
    measured.windows.push(window);
  }
  Ok(())
}

/// What a reading saw, for a failing test to print: the reading, then each window's start,
/// level, cutoff, shelf and steepest inner step.
#[cfg(test)]
pub(in crate::analysis) fn explain(
  measured: &Measured,
  reading: &super::reading::Reading,
) -> String {
  let mut text = format!(
    "{:?} at {:.2}, guess {:?}, flags {:?}\n  {}\n  limitations {:?}\n  median {:?} Hz, \
     bitrate {:?} kbps\n",
    reading.verdict,
    reading.confidence,
    reading.encoder_guess.as_ref().map(|g| &g.profile),
    reading.flags,
    reading.summary,
    reading.limitations,
    reading.measures.cutoff_median_hz,
    measured.bitrate_kbps,
  );
  for window in &measured.windows {
    let step = window
      .inner_shelf
      .map(|s| format!("{:.0} Hz -{:.0} dB clear {:.0}", s.hz, s.drop, s.clearance));
    text += &format!(
      "  t {:>4} {:>6.1} dBFS cutoff {:?} shelf {:?} step {}\n",
      window.t,
      window.rms_dbfs,
      window.cutoff_hz.map(f64::round),
      window.shelf_db.map(f64::round),
      step.unwrap_or_default()
    );
  }
  text
}
