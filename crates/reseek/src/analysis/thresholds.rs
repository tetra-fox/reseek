//! Every threshold the analyser measures and reads with, in one place. They come from what
//! common encoders do and are checked against generated audio in the tests; they are rules
//! of thumb that real files can break, which is why the reading is evidence and not proof.

/// A window quieter than this, in dBFS, is near-silent and has no cutoff.
pub(super) const SILENCE_DBFS: f64 = -70.0;
/// The cutoff is the highest band within this many dB of a window's loudest band.
pub(super) const CUTOFF_RANGE_DB: f64 = 60.0;
/// How far from a frequency the bands compared across it start, in Hz.
pub(super) const SHELF_GAP_HZ: f64 = 100.0;
/// How wide the bands compared across a frequency are, in Hz.
pub(super) const SHELF_SPAN_HZ: f64 = 500.0;
/// A drop across the cutoff of at least this many dB is an encoder's low-pass, not a natural
/// roll-off.
pub(super) const SHARP_SHELF_DB: f64 = 30.0;
/// A cutoff at or above this fraction of the band's top counts as reaching it.
pub(super) const FULL_BAND: f64 = 0.9;
/// Content that rolls off gently below this fraction of the band's top is band-limited by
/// nature, and the reading says it can not tell.
pub(super) const NATURAL_LIMIT: f64 = 0.75;
/// The lowest frequency a step inside a full band is looked for at, in Hz.
pub(super) const INNER_SHELF_FROM_HZ: f64 = 10_000.0;
/// A step of at least this many dB inside a full band is a band-limited layer under it.
pub(super) const INNER_SHELF_DB: f64 = 20.0;
/// The band gaps are counted in, in Hz: where MP3's top scale-factor band sits.
pub(super) const HOLES_FROM_HZ: f64 = 16_000.0;
/// The top of that band, in Hz.
pub(super) const HOLES_TO_HZ: f64 = 20_000.0;
/// A bin this many dB below the bins around it is a gap.
pub(super) const HOLE_DEPTH_DB: f64 = 20.0;
/// Gaps per populated bin at or above this are an encoder's.
pub(super) const HOLES_PRESENT: f64 = 0.1;
/// A frame-rhythm score at or above this is a lossy codec's.
pub(super) const FRAME_ARTEFACTS_PRESENT: f64 = 0.2;

/// A reading needs at least this many non-silent windows.
pub(super) const MIN_WINDOWS: usize = 20;
/// A reading needs at least this many seconds of non-silent audio.
pub(super) const MIN_SECONDS: f64 = 30.0;
/// The share of windows that must reach the top of the band for the content to be
/// consistent with its claim.
pub(super) const CONSISTENT_SHARE: f64 = 0.9;
/// The share of windows that must stop at one encoder cutoff for a transcode.
pub(super) const TRANSCODE_SHARE: f64 = 0.85;
/// How far from the common cutoff a window's cutoff may be and still share it, in Hz.
pub(super) const CLUSTER_HZ: f64 = 500.0;
/// The share of windows each of two sources must have for mixed sources.
pub(super) const MIXED_SHARE: f64 = 0.15;
/// A file claiming more than 48 kHz with less than this share of its energy in the upper half
/// of its band was made from audio at a lower rate.
pub(super) const UPSAMPLED_BELOW: f64 = 0.001;
/// Fewer non-silent windows than this give a reading little to go on, which it says.
pub(super) const FEW_WINDOWS: usize = 60;
/// Channels correlated above this, with the side channel at or below [`MONO_SIDE_DB`], hold
/// the same audio.
pub(super) const MONO_CORRELATION: f64 = 0.9999;
/// The side channel's level relative to the mid channel, in dB, at or below which the two
/// channels hold the same audio.
pub(super) const MONO_SIDE_DB: f64 = -90.0;
/// No reading is surer than this.
pub(super) const CONFIDENCE_CAP: f64 = 0.98;
/// A file whose median window is quieter than this, in dBFS, is quiet, which makes a reading
/// less sure.
pub(super) const QUIET_DBFS: f64 = -40.0;

/// A lossy encoder setting known by where it cuts off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Profile {
  /// where it cuts off at 44.1 and 48 kHz, in Hz
  pub hz: f64,
  /// how a report names it
  pub name: &'static str,
  /// the nominal bitrate, in kbit/s
  pub kbps: u32,
}

/// Profiles at this bitrate and above cut where some recordings are filtered when they are
/// made, so a sharp shelf there is a transcode only with frame artefacts or gaps beside it.
pub(super) const HIGH_BITRATE_KBPS: u32 = 320;

/// The cutoffs of common lossy encoder settings, lowest first. LAME's defaults are the
/// commonest; other MP3 encoders and AAC cut a little lower at the same bitrate.
pub(super) const PROFILES: &[Profile] = &[
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
pub(super) fn profile_at(hz: f64) -> Option<&'static Profile> {
  PROFILES
    .iter()
    .filter(|p| (p.hz - hz).abs() <= CLUSTER_HZ)
    .min_by(|a, b| (a.hz - hz).abs().total_cmp(&(b.hz - hz).abs()))
}

/// The guide lines drawn across a spectrogram: where common encoder settings cut off.
pub(super) const GUIDES: &[(f64, &str)] = &[
  (16_000.0, "MP3 128k"),
  (19_000.0, "MP3 192k"),
  (19_500.0, "MP3 256k"),
  (20_000.0, "MP3 320k / AAC"),
];
