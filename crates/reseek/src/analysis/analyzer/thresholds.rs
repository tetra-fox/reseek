//! Every threshold the analyzer measures and reads with, in one place. They come from what
//! common encoders do and are checked against generated audio in the tests; they are rules
//! of thumb that real files can break, which is why the reading is evidence and not proof.

/// A window quieter than this, in dBFS, is near-silent and has no cutoff.
pub(in crate::analysis) const SILENCE_DBFS: f64 = -70.0;
/// The cutoff is the highest band within this many dB of a window's loudest band.
pub(in crate::analysis) const CUTOFF_RANGE_DB: f64 = 60.0;
/// The cutoff line never sits less than this many dB over the noise of the samples' bit
/// depth, so a quiet passage's quantization noise does not read as content.
pub(in crate::analysis) const FLOOR_MARGIN_DB: f64 = 10.0;
/// How far from a frequency the bands compared across it start, in Hz.
pub(in crate::analysis) const SHELF_GAP_HZ: f64 = 100.0;
/// How wide the bands compared across a frequency are, in Hz.
pub(in crate::analysis) const SHELF_SPAN_HZ: f64 = 500.0;
/// A drop across the cutoff of at least this many dB is an encoder's low-pass, not a natural
/// roll-off.
pub(in crate::analysis) const SHARP_SHELF_DB: f64 = 30.0;
/// A cutoff at or above this fraction of the band's top counts as reaching it.
pub(in crate::analysis) const FULL_BAND: f64 = 0.9;
/// Content that rolls off gently below this fraction of the band's top is band-limited by
/// nature, and the reading says it can not tell.
pub(in crate::analysis) const NATURAL_LIMIT: f64 = 0.75;
/// The lowest frequency a step inside a full band is looked for at, in Hz.
pub(in crate::analysis) const INNER_SHELF_FROM_HZ: f64 = 10_000.0;
/// A step of at least this many dB inside a full band is a band-limited layer under it.
pub(in crate::analysis) const INNER_SHELF_DB: f64 = 20.0;
/// A step of at least this many dB where the highest bitrates cut, under a band that reaches
/// the top, is reason enough to doubt the band: it reads as something the analyzer can not
/// tell, which costs an honest file nothing. Honest spectra step by a decibel or two there.
pub(in crate::analysis) const HIGH_STEP_DB: f64 = 10.0;
/// The band above such a step is content only when it is at least this many dB over the
/// cutoff line; nearer the line it is the recording's noise floor, hiss or dither.
pub(in crate::analysis) const CLEAR_DB: f64 = 10.0;
/// The band gaps are counted in, in Hz: where MP3's top scale-factor band sits.
pub(in crate::analysis) const HOLES_FROM_HZ: f64 = 16_000.0;
/// The top of that band, in Hz.
pub(in crate::analysis) const HOLES_TO_HZ: f64 = 20_000.0;
/// A bin this many dB below the bins around it is a gap.
pub(in crate::analysis) const HOLE_DEPTH_DB: f64 = 20.0;

/// A reading needs at least this many non-silent windows.
pub(in crate::analysis) const MIN_WINDOWS: usize = 20;
/// A reading needs at least this many seconds of non-silent audio.
pub(in crate::analysis) const MIN_SECONDS: f64 = 30.0;
/// The share of windows that must reach the top of the band for the content to be
/// consistent with its claim.
pub(in crate::analysis) const CONSISTENT_SHARE: f64 = 0.9;
/// The share of windows that must stop at one encoder cutoff for a transcode.
pub(in crate::analysis) const TRANSCODE_SHARE: f64 = 0.85;
/// How far from the common cutoff a window's cutoff may be and still share it, in Hz.
pub(in crate::analysis) const CLUSTER_HZ: f64 = 500.0;
/// The share of windows each of two sources must have for mixed sources.
pub(in crate::analysis) const MIXED_SHARE: f64 = 0.15;
/// The tops of the bands of the sample rates audio is commonly made at: 22.05, 32, 44.1 and
/// 48 kHz. Content that stops with a cliff at one, in a file whose band reaches past it, was
/// made at that rate.
pub(in crate::analysis) const LOWER_RATE_EDGES: [f64; 4] = [11_025.0, 16_000.0, 22_050.0, 24_000.0];
/// Band edges of rates some old equipment recorded at, with who used them. They are left out
/// of [`LOWER_RATE_EDGES`] because they sit on encoder cutoffs (18.9 kHz beside 192 kbps
/// MP3's): a cliff at one reads as that encoder, with a limitation naming the rate.
pub(in crate::analysis) const OTHER_RATE_EDGES: [(f64, &str); 3] = [
  (18_900.0, "37.8 kHz (PlayStation and CD-i audio)"),
  (15_000.0, "30 kHz (some older samplers)"),
  (13_020.0, "26.04 kHz (some older samplers)"),
];
/// Fewer seconds of non-silent audio than this give a reading little to go on, which it says:
/// within ten seconds of the least it needs, each second moves the shares its rules compare by
/// about 2.5 percent, so a few odd seconds can tip it.
pub(in crate::analysis) const FEW_SECONDS: f64 = MIN_SECONDS + 10.0;
/// Channels correlated above this, with the side channel at or below [`MONO_SIDE_DB`], hold
/// the same audio.
pub(in crate::analysis) const MONO_CORRELATION: f64 = 0.9999;
/// The side channel's level relative to the mid channel, in dB, at or below which the two
/// channels hold the same audio.
pub(in crate::analysis) const MONO_SIDE_DB: f64 = -90.0;
/// No reading is surer than this.
pub(in crate::analysis) const CONFIDENCE_CAP: f64 = 0.98;
/// A file whose median window is quieter than this, in dBFS, is quiet, which makes a reading
/// less sure.
pub(in crate::analysis) const QUIET_DBFS: f64 = -40.0;

/// A lossy encoder setting known by where it cuts off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::analysis) struct Profile {
  /// where it cuts off at 44.1 and 48 kHz, in Hz
  pub hz: f64,
  /// how a report names it
  pub name: &'static str,
  /// the nominal bitrate, in kbit/s
  pub kbps: u32,
}

/// Profiles at this bitrate and above cut where some recordings are filtered when they are
/// made, so a sharp shelf there reads as something the analyzer can not tell, never as a
/// transcode.
pub(in crate::analysis) const HIGH_BITRATE_KBPS: u32 = 320;

/// The cutoffs of common lossy encoder settings, lowest first. LAME's defaults are the
/// commonest; other MP3 encoders and AAC cut a little lower at the same bitrate.
pub(in crate::analysis) const PROFILES: &[Profile] = &[
  Profile {
    hz: 11_000.0,
    name: "mp3 ~64 kbps",
    kbps: 64,
  },
  Profile {
    hz: 13_500.0,
    name: "mp3 ~80 kbps",
    kbps: 80,
  },
  Profile {
    hz: 15_100.0,
    name: "mp3 ~96 kbps",
    kbps: 96,
  },
  Profile {
    hz: 16_000.0,
    name: "mp3 ~128 kbps",
    kbps: 128,
  },
  Profile {
    hz: 17_000.0,
    name: "mp3 ~128 kbps",
    kbps: 128,
  },
  Profile {
    hz: 17_500.0,
    name: "mp3 ~160 kbps",
    kbps: 160,
  },
  Profile {
    hz: 18_600.0,
    name: "mp3 ~192 kbps",
    kbps: 192,
  },
  Profile {
    hz: 19_400.0,
    name: "mp3 ~224 kbps",
    kbps: 224,
  },
  Profile {
    hz: 19_700.0,
    name: "mp3 ~256 kbps",
    kbps: 256,
  },
  Profile {
    hz: 20_000.0,
    name: "mp3 ~320 kbps or aac",
    kbps: 320,
  },
  Profile {
    hz: 20_500.0,
    name: "mp3 ~320 kbps",
    kbps: 320,
  },
];

/// The profile whose cutoff is nearest `hz`, when one is within [`CLUSTER_HZ`].
pub(in crate::analysis) fn profile_at(hz: f64) -> Option<&'static Profile> {
  PROFILES
    .iter()
    .filter(|p| (p.hz - hz).abs() <= CLUSTER_HZ)
    .min_by(|a, b| (a.hz - hz).abs().total_cmp(&(b.hz - hz).abs()))
}

/// The guide lines drawn across a spectrogram: where common encoder settings cut off.
pub(in crate::analysis) const GUIDES: &[(f64, &str)] = &[
  (16_000.0, "MP3 128k"),
  (19_000.0, "MP3 192k"),
  (19_500.0, "MP3 256k"),
  (20_000.0, "MP3 320k / AAC"),
];
