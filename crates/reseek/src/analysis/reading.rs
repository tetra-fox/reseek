//! The analyser's own reading of its measures: whether the audio is consistent with the format
//! it claims, a likely transcode, a mix of sources, or something it can not tell, with how sure
//! it is, the findings independent of that, one summary, and what made it less reliable. The
//! reading works from the spread over windows, not an average, so a band-limited minority of
//! the time or of the band reads as mixed sources rather than as a transcode.

use std::collections::BTreeMap;

use api_types::analysis::{AnalysisFlag, AnalysisMeasures, AnalysisVerdict, EncoderGuess};

use super::engine::Measured;
use super::flags::findings;
use super::measure::{Step, Window};
use super::thresholds::{
  CLEAR_DB, CLUSTER_HZ, CONFIDENCE_CAP, CONSISTENT_SHARE, FEW_WINDOWS, FULL_BAND,
  HIGH_BITRATE_KBPS, INNER_SHELF_DB, MIN_SECONDS, MIN_WINDOWS, MIXED_SHARE, NATURAL_LIMIT, Profile,
  QUIET_DBFS, SHARP_SHELF_DB, TRANSCODE_SHARE, profile_at,
};

/// The top of the band of a 48 kHz recording; a file whose band goes past it is hi-res.
const HI_RES_TOP_HZ: f64 = 24_000.0;

/// The width of a cutoff histogram bucket, in Hz.
const HISTOGRAM_HZ: f64 = 500.0;

/// What the analyser makes of a file.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Reading {
  /// the reading of the cutoff
  pub verdict: AnalysisVerdict,
  /// how sure it is, from 0 to [`CONFIDENCE_CAP`]
  pub confidence: f64,
  /// findings independent of the cutoff
  pub flags: Vec<AnalysisFlag>,
  /// the encoder setting the cutoff matches
  pub encoder_guess: Option<EncoderGuess>,
  /// what it found, in words
  pub summary: String,
  /// the aggregate measures
  pub measures: AnalysisMeasures,
  /// what made the reading less reliable
  pub limitations: Vec<String>,
  /// the cutoff to mark on the spectrogram, when the content stops short of the top
  pub cutoff_hz: Option<f64>,
}

/// The reading of the cutoff alone, before the findings independent of it are added.
struct Verdict {
  verdict: AnalysisVerdict,
  confidence: f64,
  encoder_guess: Option<EncoderGuess>,
  summary: String,
  limitations: Vec<String>,
  cutoff_hz: Option<f64>,
}

/// A window that is not near-silent, as the reading counts it.
#[derive(Clone, Copy, Debug)]
struct Seen {
  cutoff_hz: Option<f64>,
  shelf_db: Option<f64>,
  inner_shelf: Option<Step>,
  rms_dbfs: f64,
}

impl Seen {
  /// `window` as the reading counts it against a band ending at `top`. A window whose band
  /// reaches the top only through a floor that just clears the cutoff line, above a step at an
  /// encoder's cutoff, stops at that step: hiss or dither over a filtered recording is not
  /// full-band content.
  fn of(window: &Window, top: f64) -> Self {
    let floor = window.inner_shelf.filter(|step| {
      window.cutoff_hz.is_some_and(|c| c >= FULL_BAND * top)
        && step.drop >= INNER_SHELF_DB
        && profile_at(step.hz).is_some()
        && step.clearance < CLEAR_DB
    });
    match floor {
      Some(step) => Self {
        cutoff_hz: Some(step.hz),
        shelf_db: Some(step.drop),
        inner_shelf: None,
        rms_dbfs: window.rms_dbfs,
      },
      None => Self {
        cutoff_hz: window.cutoff_hz,
        shelf_db: window.shelf_db,
        inner_shelf: window.inner_shelf,
        rms_dbfs: window.rms_dbfs,
      },
    }
  }
}

/// The windows that are not near-silent, as the reading counts them.
struct Heard {
  windows: Vec<Seen>,
  /// the top of the band the content is judged against, in Hz
  top: f64,
}

impl Heard {
  fn share(&self, keep: impl Fn(&Seen) -> bool) -> f64 {
    let kept = self.windows.iter().filter(|w| keep(w)).count();
    kept as f64 / self.windows.len().max(1) as f64
  }

  fn full(&self, window: &Seen) -> bool {
    window.cutoff_hz.is_some_and(|c| c >= FULL_BAND * self.top)
  }

  /// A window that stops short of the top at an encoder's cutoff, with a sharp shelf.
  fn limited(&self, window: &Seen) -> Option<&'static Profile> {
    let cutoff = window.cutoff_hz?;
    (!self.full(window) && sharp(window))
      .then(|| profile_at(cutoff))
      .flatten()
  }

  /// A full-band window with a band-limited layer under it at an encoder's cutoff.
  fn layered(&self, window: &Seen) -> Option<f64> {
    let step = window.inner_shelf?;
    (self.full(window) && step.drop >= INNER_SHELF_DB && profile_at(step.hz).is_some())
      .then_some(step.hz)
  }
}

fn sharp(window: &Seen) -> bool {
  window.shelf_db.is_some_and(|s| s >= SHARP_SHELF_DB)
}

/// Reads what `measured` found.
pub(super) fn read(measured: &Measured) -> Reading {
  let Some(claimed) = &measured.claimed else {
    return Reading {
      verdict: AnalysisVerdict::Inconclusive,
      confidence: 0.0,
      flags: Vec::new(),
      encoder_guess: None,
      summary: "The file could not be read as audio.".to_string(),
      measures: measures(measured, &[], None),
      limitations: measured.problems.clone(),
      cutoff_hz: None,
    };
  };
  let nyquist = f64::from(claimed.sample_rate) / 2.0;
  let windows: Vec<&Window> = measured
    .windows
    .iter()
    .filter(|w| w.cutoff_hz.is_some())
    .collect();
  let upsample_ratio = (claimed.sample_rate > 48_000).then(|| upsample_ratio(&windows));
  let measures = measures(measured, &windows, upsample_ratio);
  let median = measures.cutoff_median_hz.map(f64::from);
  let found_beside = findings(claimed, &windows, median, &measures);
  // an upsampled file is judged against the band of the rate it was made at
  let top = found_beside.source_edge.unwrap_or(nyquist);
  let windows: Vec<Seen> = windows.iter().map(|w| Seen::of(w, top)).collect();
  let cutoffs: Vec<f64> = windows.iter().filter_map(|w| w.cutoff_hz).collect();
  let median = (!cutoffs.is_empty()).then(|| middle(&cutoffs));
  let heard = Heard { windows, top };
  let mut limitations = measured.problems.clone();
  if measured.sampled {
    limitations.push(format!(
      "Only {} one-second windows spread over the file were analysed.",
      measured.windows.len()
    ));
  }
  if measured.channels_left_out > 0 {
    limitations.push(format!(
      "Only the first two of {} channels were analysed.",
      claimed.channels
    ));
  }
  let mut found = verdict(&heard, median, claimed.lossless, measured.bitrate_kbps);
  if heard.windows.len() < FEW_WINDOWS {
    limitations.push("Short or mostly quiet audio gives few windows to judge from.".to_string());
  }
  if quiet(&heard) {
    limitations
      .push("The audio is quiet, which makes the upper band harder to measure.".to_string());
  }
  if let Some(edge) = found_beside.source_edge
    && let Some(profile) = profile_at(edge)
    && found.verdict != AnalysisVerdict::MixedSources
  {
    // the edge of the lower rate is also where an encoder cuts: either could be the source
    found = Verdict {
      verdict: AnalysisVerdict::Inconclusive,
      confidence: 0.4,
      encoder_guess: Some(EncoderGuess {
        profile: profile.name.to_string(),
        confidence: 0.4,
      }),
      summary: format!(
        "The content stops at {}, the top of the band a lower sample rate holds and also \
         where an encoder at {} cuts off; the analyser can not tell which made it.",
        khz(edge),
        profile.name
      ),
      limitations: Vec::new(),
      cutoff_hz: Some(edge),
    };
  }
  limitations.extend(found.limitations);
  Reading {
    verdict: found.verdict,
    confidence: found.confidence,
    flags: found_beside.flags,
    encoder_guess: found.encoder_guess,
    summary: std::iter::once(found.summary)
      .chain(found_beside.notes)
      .collect::<Vec<_>>()
      .join(" "),
    measures,
    limitations,
    cutoff_hz: found.cutoff_hz,
  }
}

/// Reads the cutoff of the non-silent windows, whose median cutoff is `median`.
fn verdict(heard: &Heard, median: Option<f64>, lossless: bool, bitrate: Option<u32>) -> Verdict {
  let count = heard.windows.len();
  let seconds = count as f64;
  let mut reading = Verdict {
    verdict: AnalysisVerdict::Inconclusive,
    confidence: 0.3,
    encoder_guess: None,
    summary: String::new(),
    limitations: Vec::new(),
    cutoff_hz: median.filter(|m| *m < FULL_BAND * heard.top),
  };
  let Some(median) = median.filter(|_| count >= MIN_WINDOWS && seconds >= MIN_SECONDS) else {
    reading.confidence = 0.2;
    reading.summary = format!(
      "Only {seconds:.0} s of the audio is above silence; a reading needs at least {MIN_SECONDS:.0} s."
    );
    return reading;
  };
  let clustered = heard.share(|w| {
    w.cutoff_hz
      .is_some_and(|c| (c - median).abs() <= CLUSTER_HZ)
  });
  let sharp_share = heard.share(|w| {
    sharp(w)
      && w
        .cutoff_hz
        .is_some_and(|c| (c - median).abs() <= CLUSTER_HZ)
  }) / clustered.max(f64::EPSILON);
  // a lossy file's own encoder cuts off too: only a cutoff well below its bitrate's counts
  let below_claim =
    |profile: &Profile| lossless || bitrate.is_some_and(|kbps| profile.kbps * 3 / 2 <= kbps);

  // a cutoff where the highest bitrates cut is also where some recordings are filtered, and
  // no measure tells the two apart
  if let Some(profile) = profile_at(median)
    .filter(|p| below_claim(p))
    .filter(|p| p.kbps < HIGH_BITRATE_KBPS)
    && clustered >= TRANSCODE_SHARE
    && sharp_share >= 0.5
  {
    let evidence = 0.6 * ((clustered - TRANSCODE_SHARE) / (1.0 - TRANSCODE_SHARE)).clamp(0.0, 1.0)
      + 0.4 * sharp_share;
    let confidence = 0.6 + 0.38 * evidence;
    reading
      .limitations
      .push("Two or more lossy generations show as the lowest cutoff among them.".to_string());
    reading.verdict = AnalysisVerdict::LikelyTranscode;
    reading.confidence = confidence.min(CONFIDENCE_CAP);
    reading.encoder_guess = Some(EncoderGuess {
      profile: profile.name.to_string(),
      confidence: (clustered * (0.5 + 0.5 * sharp_share)).min(CONFIDENCE_CAP),
    });
    reading.cutoff_hz = Some(median);
    reading.summary = format!(
      "{} of the time stops at {} with a sharp shelf, where an encoder at {} cuts off.",
      percent(clustered),
      khz(median),
      profile.name
    );
    return reading;
  }

  if let Some(profile) = profile_at(median)
    && clustered >= TRANSCODE_SHARE
    && sharp_share >= 0.5
  {
    reading.cutoff_hz = Some(median);
    if !lossless {
      reading.verdict = AnalysisVerdict::Consistent;
      reading.confidence = 0.8;
      reading.summary = format!(
        "{} of the time stops at {}, where an encoder at {} cuts off, as a file of its \
         bitrate does.",
        percent(clustered),
        khz(median),
        profile.name
      );
      return reading;
    }
    // past the transcode rule, this is a sharp cutoff where the highest bitrates cut
    reading.encoder_guess = Some(EncoderGuess {
      profile: profile.name.to_string(),
      confidence: 0.4,
    });
    reading.limitations.push(
      "Some recordings are filtered at 20 kHz when they are made or mastered, which looks \
       the same as a high-bitrate encoder's cutoff."
        .to_string(),
    );
    reading.summary = format!(
      "{} of the time stops at {} with a sharp shelf, where an encoder at {} cuts off; a \
       recording filtered there when it was made looks the same, and the analyser can not \
       tell the two apart.",
      percent(clustered),
      khz(median),
      profile.name
    );
    return reading;
  }

  let full = heard.share(|w| heard.full(w));
  let limited: Vec<f64> = heard
    .windows
    .iter()
    .filter(|w| heard.limited(w).is_some())
    .filter_map(|w| w.cutoff_hz)
    .collect();
  let limited_share = limited.len() as f64 / count as f64;
  if limited_share >= MIXED_SHARE && full >= MIXED_SHARE {
    let at = middle(&limited);
    reading.verdict = AnalysisVerdict::MixedSources;
    reading.confidence = 0.5 + 0.35 * (limited_share.min(full) / 0.5).min(1.0);
    reading.encoder_guess = profile_at(at).map(|p| EncoderGuess {
      profile: p.name.to_string(),
      confidence: 0.5,
    });
    reading.cutoff_hz = Some(at);
    reading.summary = format!(
      "{} of the time is full-band; {} stops at {} with a sharp shelf, as when part of the \
       track comes from a lossy source.",
      percent(full),
      percent(limited_share),
      khz(at)
    );
    return reading;
  }
  let layers: Vec<f64> = heard
    .windows
    .iter()
    .filter_map(|w| heard.layered(w))
    .collect();
  let layered_share = layers.len() as f64 / count as f64;
  if layered_share >= MIXED_SHARE {
    let at = middle(&layers);
    reading.verdict = AnalysisVerdict::MixedSources;
    reading.confidence = 0.5 + 0.35 * layered_share.min(1.0);
    reading.encoder_guess = profile_at(at).map(|p| EncoderGuess {
      profile: p.name.to_string(),
      confidence: 0.5,
    });
    reading.cutoff_hz = Some(at);
    reading.summary = format!(
      "The content reaches the top of the band, but {} of the time shows a sharp step at {} \
       under it, as when a band-limited layer is mixed with full-band material.",
      percent(layered_share),
      khz(at)
    );
    return reading;
  }

  let open = heard.share(|w| heard.full(w) || !sharp(w));
  if open >= CONSISTENT_SHARE {
    if full >= CONSISTENT_SHARE {
      let windows = ((count - MIN_WINDOWS) as f64 / 100.0).min(1.0);
      let mut confidence = 0.7 + 0.25 * windows;
      if quiet(heard) {
        confidence -= 0.1;
      }
      reading.verdict = AnalysisVerdict::Consistent;
      reading.confidence = confidence.min(CONFIDENCE_CAP);
      reading.summary = format!(
        "{} of the time reaches {}, near the top of the {} band, with no encoder's shelf.",
        percent(full),
        khz(median),
        khz(heard.top)
      );
      return reading;
    }
    reading.limitations.push(
      "Lossless recordings of old masters, speech, lo-fi material or vinyl and cassette \
       transfers roll off early, and heavy equalisation can look like a shelf."
        .to_string(),
    );
    if median >= NATURAL_LIMIT * heard.top {
      reading.verdict = AnalysisVerdict::Consistent;
      reading.confidence = 0.7;
      reading.summary = format!(
        "The content rolls off gently from about {}, not at an encoder's sharp cutoff.",
        khz(median)
      );
      return reading;
    }
    reading.confidence = 0.4;
    // a band above what a 48 kHz recording holds is mostly empty in honest hi-res audio too
    let like = if heard.top > HI_RES_TOP_HZ {
      "as many hi-res recordings do, whose microphones and instruments give little above 20 kHz"
    } else {
      "as test tones, speech, old masters and lo-fi recordings do"
    };
    reading.summary = format!(
      "The content stops at about {}, well below the top of the band, without an encoder's \
       sharp shelf, {like}.",
      khz(median)
    );
    return reading;
  }
  reading.summary = if sharp_share >= 0.5 && clustered >= TRANSCODE_SHARE {
    format!(
      "The content stops at {} with a sharp shelf, which is not where common encoders cut off.",
      khz(median)
    )
  } else {
    "The windows do not fit one pattern.".to_string()
  };
  reading
}

/// The aggregate measures over the non-silent `windows`.
fn measures(
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
    frame_artefact_score: percentile(&scores, 0.5),
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
fn upsample_ratio(windows: &[&Window]) -> f64 {
  let (high, total) = windows
    .iter()
    .fold((0.0, 0.0), |(h, t), w| (h + w.energy.0, t + w.energy.1));
  if total > 0.0 { high / total } else { 0.0 }
}

/// Whether the middle window is quiet.
fn quiet(heard: &Heard) -> bool {
  let levels: Vec<f64> = heard.windows.iter().map(|w| w.rms_dbfs).collect();
  percentile(&levels, 0.5).is_some_and(|level| level < QUIET_DBFS)
}

/// The median of `values`, 0 for none.
fn middle(values: &[f64]) -> f64 {
  percentile(values, 0.5).unwrap_or(0.0)
}

/// The value `fraction` of the way through `values` in order, by nearest rank.
fn percentile(values: &[f64], fraction: f64) -> Option<f64> {
  if values.is_empty() {
    return None;
  }
  let mut sorted = values.to_vec();
  sorted.sort_by(f64::total_cmp);
  let rank = ((fraction * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
  Some(sorted[rank - 1])
}

/// A frequency as kHz to one decimal, without a trailing `.0`.
fn khz(hz: f64) -> String {
  let text = format!("{:.1}", hz / 1000.0);
  format!("{} kHz", text.strip_suffix(".0").unwrap_or(&text))
}

/// A share as a whole percentage.
fn percent(share: f64) -> String {
  format!("{:.0}%", share * 100.0)
}
