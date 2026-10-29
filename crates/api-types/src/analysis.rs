//! Audio analysis: the report the analyzer writes for one downloaded file, the user's own
//! judgment of it, the analysis queue, and the bodies and queries of the analysis calls.
//!
//! The report is evidence for the user to read next to the spectrogram, not proof: the
//! analyzer's verdict is its own reading and the user's judgment replaces it once set.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::files::FileRoot;

/// The analyzer's reading of whether the audio matches the format the file claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisVerdict {
  /// nearly every window reaches the top of the claimed band without an encoder's shelf
  Consistent,
  /// nearly every window stops at the same known encoder cutoff with a sharp shelf or frame
  /// artifacts
  LikelyTranscode,
  /// part of the time or part of the frequency range is band-limited while the rest is full
  /// band, as when one low-bitrate stem is mixed into a lossless track
  MixedSources,
  /// too little audio, or measures that fit no pattern
  Inconclusive,
}

/// A finding reported alongside the verdict, independent of the cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisFlag {
  /// the content stops with a cliff at the top of a lower sample rate's band where no encoder
  /// cuts (22.05 or 24 kHz), so the audio was made at that rate and converted up
  Upsampled,
  /// a file claiming 24 bits carries no more than 16
  BitPadded,
}

/// The user's own judgment of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Judgment {
  /// the file is what it claims to be
  Legit,
  /// the file is not what it claims to be
  Fake,
}

/// The analysis of one audio file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AudioAnalysis {
  /// the file's local path
  pub path: String,
  /// the directory the file is under, as the files endpoints name it
  pub root: FileRoot,
  /// the file's path under its root, `/`-separated, as the files endpoints take it
  pub relative_path: String,
  /// the download the file belongs to, null when it is no download's
  pub transfer_id: Option<Uuid>,
  /// the file's size in bytes when it was analyzed
  pub size: u64,
  /// when the analysis finished
  pub analyzed_at: DateTime<Utc>,
  /// how long the audio is, in seconds
  pub duration_seconds: f64,
  /// the format the container claims
  pub claimed: ClaimedFormat,
  /// the analyzer's reading
  pub verdict: AnalysisVerdict,
  /// how sure the analyzer is of its reading, from 0 to 0.98
  pub confidence: f64,
  /// findings independent of the verdict, empty when there are none
  pub flags: Vec<AnalysisFlag>,
  /// the encoder profile the cutoff matches, null when it matches none
  pub encoder_guess: Option<EncoderGuess>,
  /// one sentence saying what the analyzer found and where
  #[schema(
    example = "68% of the time is full-band; 32% shows a 16 kHz cutoff concentrated in vocal content."
  )]
  pub summary: String,
  /// the aggregate measures over the non-silent windows
  pub measures: AnalysisMeasures,
  /// the measures of each analyzed window, null where the per-window list was left out
  pub windows: Option<Vec<AnalysisWindow>>,
  /// where the overview of this report's spectrogram is served
  pub spectrogram_url: String,
  /// how the spectrogram and the waveform are served and how their axes run; null when the file
  /// could not be read as audio and has neither
  pub spectrogram: Option<SpectrogramLayout>,
  /// the user's judgment, shown in place of the verdict; null until the user sets one
  #[serde(rename = "override")]
  pub judgment: Option<AnalysisOverride>,
  /// what made this analysis less reliable, such as few windows or a decoder error
  pub limitations: Vec<String>,
  /// true when an older version of reseek analyzed the file and this one cannot show its
  /// report: only the file, when, the reading and the user's judgment are kept, and analyzing
  /// the file again replaces it
  pub stale: bool,
}

/// The format a file's container claims. A file that could not be read as audio claims the
/// codec `unknown`, with no samples per second and no channels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimedFormat {
  /// the codec's short name
  #[schema(example = "flac")]
  pub codec: String,
  /// samples per second
  #[schema(example = 44100)]
  pub sample_rate: u32,
  /// bits per sample, null for codecs that have none such as MP3
  pub bit_depth: Option<u32>,
  /// the number of channels
  pub channels: u32,
  /// the nominal bitrate in kbit/s, null for lossless codecs
  pub bitrate_kbps: Option<u32>,
}

/// An encoder profile the measured cutoff matches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncoderGuess {
  /// the codec and approximate bitrate
  #[schema(example = "mp3 ~128 kbps")]
  pub profile: String,
  /// how sure the analyzer is of the match, from 0 to 1
  pub confidence: f64,
}

/// The aggregate measures of a file. Each is null when the file had too little non-silent
/// audio to measure it or the measure does not apply to the file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisMeasures {
  /// pairs of a cutoff in Hz and how many non-silent windows have it, ascending by cutoff
  #[schema(example = json!([[16000, 81], [22000, 173]]))]
  pub cutoff_hz_histogram: Vec<[u32; 2]>,
  /// the median cutoff over the non-silent windows, in Hz
  pub cutoff_median_hz: Option<u32>,
  /// the 95th percentile of the energy drop across 500 Hz above the cutoff, in dB
  pub shelf_sharpness_db_p95: Option<f64>,
  /// the ratio of near-zero to populated bins between 16 and 20 kHz
  pub sfb21_holes: Option<f64>,
  /// how strongly the noise floor repeats at lossy codec frame lengths, from 0 to 1
  pub frame_artifact_score: Option<f64>,
  /// the fraction of energy above half the claimed band, null unless the file claims more
  /// than 48 kHz
  pub upsample_ratio: Option<f64>,
  /// the bits per sample actually in use, null for codecs without integer samples
  pub bit_depth_effective: Option<u32>,
  /// the estimated noise floor in dBFS
  pub noise_floor_dbfs: Option<f64>,
  /// the correlation between the channels, null for mono
  pub stereo_correlation: Option<f64>,
  /// the energy of the side channel in dB, null for mono
  pub side_energy_db: Option<f64>,
  /// the mean sample value relative to full scale
  pub dc_offset: Option<f64>,
  /// the percentage of samples at full scale
  pub clipped_samples_pct: Option<f64>,
}

/// The measures of one analyzed window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisWindow {
  /// where the window starts, in seconds from the start of the file
  pub t: f64,
  /// the highest band within 60 dB of the window's peak, in Hz; null for a near-silent window
  pub cutoff_hz: Option<u32>,
  /// the energy drop across 500 Hz above the cutoff, in dB; null without a cutoff
  pub shelf_db: Option<f64>,
  /// the window's loudness in dBFS
  pub rms_dbfs: f64,
}

/// How a spectrogram's frequency axis runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpectrogramScale {
  /// octaves take the same height
  Log,
  /// every kilohertz takes the same height
  Linear,
  /// equal steps of pitch as heard take the same height: the mel scale, by O'Shaughnessy's
  /// formula 2595 log10(1 + f / 700)
  Mel,
}

/// How a report's spectrogram and waveform are served and how their axes run. Both span the
/// file's whole length from 0 seconds. The spectrogram's columns hold levels over linear
/// frequency bins from 0 Hz to `maxHz`, which a viewer lays out on the frequency axis it
/// shows; each is served as an overview of the whole file and as tiles at full detail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpectrogramLayout {
  /// the frequency axis a viewer shows until its user picks one, from the daemon's options
  pub scale: SpectrogramScale,
  /// the frequency at the top of the highest bin, in Hz: half the sample rate
  pub max_hz: f64,
  /// the file's length in seconds, which the time axis spans
  pub duration_seconds: f64,
  /// whether the windows are samples spread over a long file, the stretches between them
  /// unread, rather than every second of it
  pub sampled: bool,
  /// the lines worth drawing across the spectrogram, lowest first
  pub guides: Vec<GuideLine>,
  /// how many frequency bins a column has; bin `b` spans `b` to `b + 1` times `maxHz / bins`
  pub bins: u32,
  /// how many spectrogram columns a second takes in a tile
  pub columns_per_second: u32,
  /// how many waveform buckets a second takes in a tile
  pub peaks_per_second: u32,
  /// how many seconds a tile spans; tile `n` starts at `n` times this
  pub tile_seconds: u32,
  /// how many tiles the file's length takes
  pub tiles: u32,
}

/// A horizontal line across the spectrogram at a frequency worth comparing the content with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GuideLine {
  /// the frequency, in Hz
  pub hz: f64,
  /// the label drawn beside it
  #[schema(example = "MP3 128k")]
  pub label: String,
  /// what the line marks
  pub kind: GuideKind,
}

/// What a guide line marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuideKind {
  /// where a common encoder setting cuts off by default; other encoders and settings cut
  /// elsewhere
  Encoder,
  /// half the file's sample rate, the highest frequency it can hold
  FileLimit,
  /// the cutoff the analyzer measured, drawn dashed
  Cutoff,
}

/// The user's judgment of a file, with who recorded it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisOverride {
  /// the judgment
  pub verdict: Judgment,
  /// why, in the user's words; null when none was given
  #[schema(example = "remix, known low-quality acapella")]
  pub note: Option<String>,
  /// the subject of the principal that recorded it
  #[schema(example = "local:admin")]
  pub by: String,
  /// when it was recorded
  pub at: DateTime<Utc>,
}

/// The body of `PUT .../analysis/override`, which records the user's judgment of a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetAnalysisOverride {
  /// the judgment
  pub verdict: Judgment,
  /// why, in the user's words
  #[serde(default)]
  #[schema(example = "remix, known low-quality acapella")]
  pub note: Option<String>,
}

/// The query of `POST .../analysis`, which queues a file for analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RunAnalysisQuery {
  /// analyze again even when a report for the file as it is now exists
  #[serde(default)]
  pub force: bool,
}

/// The answer to `POST .../analysis`: the queued job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobCreated {
  /// the job's id
  pub job_id: Uuid,
}

/// The query of `GET .../analysis`, which reads a file's report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AnalysisReportQuery {
  /// include the per-window measures, true when left out
  #[serde(default = "yes")]
  pub windows: bool,
}

/// The query of the spectrogram and waveform routes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct TileQuery {
  /// the tile at full detail, from 0; the overview of the whole file when left out
  #[serde(default)]
  pub tile: Option<u32>,
}

/// The header of a spectrogram response, which the columns follow: `columns` times `bins`
/// bytes, column after column, each column's bins lowest first. A byte is a level from `minDb`
/// (0) to `maxDb` (255) on a straight scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpectrogramTile {
  /// when the first column starts, in seconds from the start of the file
  pub from_seconds: f64,
  /// when the last column ends, in seconds from the start of the file
  pub to_seconds: f64,
  /// how many columns follow, evenly spread from `fromSeconds` to `toSeconds`
  pub columns: u32,
  /// how many frequency bins a column has, from 0 Hz to `maxHz`
  pub bins: u32,
  /// the frequency at the top of the highest bin, in Hz
  pub max_hz: f64,
  /// the level of byte 0, in dB relative to a full-scale sine
  pub min_db: f64,
  /// the level of byte 255, in dB relative to a full-scale sine
  pub max_db: f64,
  /// runs of columns the analysis did not read, as the first and one past the last; their
  /// bytes are 0
  pub unread: Vec<[u32; 2]>,
}

/// The header of a waveform response, which the buckets follow: two signed bytes each, the
/// lowest and the highest sample over the channels from -127 (full scale below) to 127.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaveformTile {
  /// when the first bucket starts, in seconds from the start of the file
  pub from_seconds: f64,
  /// when the last bucket ends, in seconds from the start of the file
  pub to_seconds: f64,
  /// how many buckets follow, evenly spread from `fromSeconds` to `toSeconds`
  pub peaks: u32,
  /// runs of buckets the analysis did not read, as the first and one past the last
  pub unread: Vec<[u32; 2]>,
}

/// The header of a window spectra response, which every analyzed window's spectrum over its
/// second follows, in the order of the report's windows: `bins` bytes each, lowest first, a
/// byte a level from `minDb` (0) to `maxDb` (255).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowSpectra {
  /// how many windows follow
  pub windows: u32,
  /// how many frequency bins a spectrum has, from 0 Hz to `maxHz`
  pub bins: u32,
  /// the frequency at the top of the highest bin, in Hz
  pub max_hz: f64,
  /// the level of byte 0, in dB relative to a full-scale sine
  pub min_db: f64,
  /// the level of byte 255, in dB relative to a full-scale sine
  pub max_db: f64,
}

fn yes() -> bool {
  true
}

/// The query of `GET /analysis`, which lists reports newest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AnalysisListQuery {
  /// only reports with this verdict
  #[serde(default)]
  pub verdict: Option<AnalysisVerdict>,
  /// only reports that finished after this time
  #[serde(default)]
  pub since: Option<DateTime<Utc>>,
  /// how many matching reports to skip, 0 when left out
  #[serde(default)]
  pub offset: u32,
  /// how many reports to return at most, 100 when left out
  #[serde(default = "default_list_limit")]
  pub limit: u32,
}

fn default_list_limit() -> u32 {
  100
}

/// Where an analysis job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisJobState {
  /// waiting for the worker
  Queued,
  /// being analyzed
  Running,
}

/// A file waiting for or under analysis, an entry of `GET /analysis/jobs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJob {
  /// the job's id
  pub id: Uuid,
  /// the file's local path
  pub path: String,
  /// the directory the file is under, as the files endpoints name it
  pub root: FileRoot,
  /// the file's path under its root, `/`-separated, as the files endpoints take it
  pub relative_path: String,
  /// the download the file belongs to, null when it is no download's
  pub transfer_id: Option<Uuid>,
  /// where the job is
  pub state: AnalysisJobState,
  /// when it was queued
  pub queued_at: DateTime<Utc>,
  /// when the worker took it, null while queued
  pub started_at: Option<DateTime<Utc>>,
  /// how far the worker is through the file, from 0 to 1; null while queued
  pub progress: Option<f64>,
}

/// The spectrum of one analyzed window, as `GET .../analysis/windows/{index}` answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowSpectrum {
  /// the window's position in the report's windows
  pub index: u32,
  /// where the window starts, in seconds from the start of the file
  pub t: f64,
  /// the frequency of each point, in Hz, lowest first: the middle of each frequency bin
  pub hz: Vec<f64>,
  /// the level at each point, in dB relative to a full-scale sine, at least -160
  pub db: Vec<f64>,
}

/// The data of a `files.analysis` frame: a report finished.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisCompleted {
  /// the download the file belongs to, null when it was analyzed by path
  pub transfer_id: Option<Uuid>,
  /// the file's local path
  pub path: String,
  /// the analyzer's reading
  pub verdict: AnalysisVerdict,
  /// how sure the analyzer is of its reading
  pub confidence: f64,
  /// findings independent of the verdict
  pub flags: Vec<AnalysisFlag>,
}
