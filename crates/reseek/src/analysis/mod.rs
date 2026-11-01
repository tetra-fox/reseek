//! Audio analysis of downloaded files: a tool the user runs to judge whether a file is what it
//! claims, such as a FLAC that is really a converted MP3. The file is decoded and its spectrum
//! measured a second at a time; the main output is a spectrogram and a waveform, which the
//! viewer draws with guide lines where common encoders cut off, and the analyzer's own reading
//! comes with them as evidence. The user's judgment, once recorded, is what counts. The
//! analyzer reads files under the downloads and incomplete directories and files uploaded for it,
//! and never changes one.

mod analyzer;
mod backlog;
mod jobs;
mod kept;
mod report;
mod store;
mod tiles;
mod uploads;

#[cfg(test)]
mod service_tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use api_types::analysis::{
  AnalysisBatchQueued, AnalysisJob, AnalysisJobCreated, AnalysisListQuery, AnalysisReportQuery,
  AnalysisUpload, AudioAnalysis, SetAnalysisOverride, WindowSpectrum,
};
use api_types::auth::Principal;
use api_types::files::FileRoot;
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use cascade::Watcher;
use chrono::Utc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::{Database, DbError, Location};
use crate::events::EventBus;
use crate::files::Files;
use crate::live::Live;
use crate::services::{AnalysisService, FileTarget, Piece};
use crate::sync::lock;
pub(crate) use analyzer::decode::Decoder;
use analyzer::{engine, picture, reading};
use jobs::{Job, QUEUE_LIMIT, Queue, Queued};
use report::modified_text;
pub use store::SCHEMA;
pub use uploads::uploads_dir;

/// The analysis service: the queue, the reports and judgments in `analysis.db`, and the
/// spectrogram and waveform files in the data directory.
pub struct Analysis {
  db: Database,
  files: Arc<Files>,
  config: Watcher<Options>,
  live: Live,
  events: EventBus,
  /// where the spectrogram and waveform files are kept
  dir: PathBuf,
  /// where files uploaded for analysis are held
  uploads: PathBuf,
  /// the uploads held, which go with a restart
  held: Mutex<uploads::Uploads>,
  queue: Mutex<Queue>,
  /// the files of batches waiting for room in the queue
  backlog: Mutex<backlog::Backlog>,
  /// paths of reports and spectrogram files that did not read, so each is warned about once
  unreadable: Mutex<HashSet<String>>,
  /// woken when a job is queued or one ends
  wake: Notify,
  /// a finished download was dropped from the full queue and not queued since
  full: AtomicBool,
  shutdown: CancellationToken,
}

impl Analysis {
  /// Opens `analysis.db` at `location`, keeps spectrogram and waveform files under
  /// `data`/analysis, removing what an older version kept there and every upload of the last run,
  /// and starts the queue, which runs until `shutdown`.
  pub async fn open(
    location: &Location,
    data: &std::path::Path,
    files: Arc<Files>,
    config: Watcher<Options>,
    live: Live,
    events: EventBus,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, DbError> {
    let db = Database::open(&SCHEMA, location).await?;
    let dir = data.join("analysis");
    let older = dir.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || kept::remove_older(&older)).await {
      warn!(error = %e, "removing older spectrogram files failed");
    }
    let analysis = Arc::new(Self {
      db,
      files,
      config,
      live,
      events,
      uploads: uploads_dir(data),
      held: Mutex::default(),
      dir,
      queue: Mutex::new(Queue::default()),
      backlog: Mutex::default(),
      unreadable: Mutex::default(),
      wake: Notify::new(),
      full: AtomicBool::new(false),
      shutdown,
    });
    analysis.clear_uploads().await?;
    tokio::spawn(jobs::dispatch(analysis.clone()));
    Ok(analysis)
  }

  /// The database, for listing and closing.
  pub fn database(&self) -> &Database {
    &self.db
  }

  /// Says once that the report of the file at `path` could not be read and why, when it could
  /// not; it reads as stale.
  fn unreadable(&self, path: &str, why: Option<&str>) {
    let Some(why) = why else {
      return;
    };
    if lock(&self.unreadable).insert(path.to_string()) {
      warn!(
        path,
        error = why,
        "an analysis report does not read; it shows as stale"
      );
    }
  }

  /// The stored report of the file at `path`, 404 when it has none.
  async fn stored(&self, path: &std::path::Path) -> Result<store::Stored, ApiError> {
    let path = path.to_string_lossy();
    let stored = store::find(&self.db, &path)
      .await
      .map_err(ApiError::internal)?
      .ok_or_else(|| ApiError::not_found(format!("'{path}' has not been analyzed")))?;
    self.unreadable(&stored.report.path, stored.unreadable.as_deref());
    Ok(stored)
  }
}

/// The analyzer's verdict on audio held in memory, read as a downloaded file is, given ten
/// seconds. It exists for fuzzing the decoders and the measures with arbitrary bytes; the
/// daemon reads files.
#[doc(hidden)]
pub fn verdict_of(bytes: Vec<u8>) -> api_types::analysis::AnalysisVerdict {
  let settings = engine::Settings {
    max_full_seconds: 900.0,
    sampled_windows: 240,
    deadline: std::time::Instant::now() + std::time::Duration::from_secs(10),
    progress: None,
  };
  match engine::read_bytes(bytes, &settings, &CancellationToken::new()) {
    Ok(measured) => reading::read(&measured).verdict,
    Err(engine::Canceled) => unreachable!("nothing cancels this token"),
  }
}

/// Whether `stored` was made from a file of `size` last changed at `modified`.
fn current(stored: &store::Stored, size: u64, modified: SystemTime) -> bool {
  stored.size == size && stored.modified == modified_text(modified)
}

#[async_trait]
impl AnalysisService for Analysis {
  async fn run(&self, target: FileTarget, force: bool) -> Result<AnalysisJobCreated, ApiError> {
    let file = self.files.local_file(&target).await?;
    match self.enqueue(file, force).await? {
      Queued::Job(id) => Ok(AnalysisJobCreated { job_id: id }),
      Queued::Current => Err(ApiError::conflict(
        "The file has already been analyzed",
        Some("Its report is current. Use force=true to analyze it again.".to_string()),
      )),
      Queued::Already => Err(ApiError::conflict(
        "The file is queued or being analyzed",
        None,
      )),
      Queued::Full => Err(ApiError::too_many(format!(
        "The analysis queue is full ({QUEUE_LIMIT} files)"
      ))),
    }
  }

  async fn upload(&self, by: &str, name: &str, body: Body) -> Result<AnalysisUpload, ApiError> {
    self.receive(by, name, body).await
  }

  async fn run_batch(
    &self,
    files: Vec<(u32, FileTarget)>,
  ) -> Result<AnalysisBatchQueued, ApiError> {
    Ok(self.batch(files).await)
  }

  async fn report(
    &self,
    target: FileTarget,
    query: AnalysisReportQuery,
  ) -> Result<AudioAnalysis, ApiError> {
    let file = self.files.local_path(&target).await?;
    let stored = self.stored(&file.path).await?;
    let note = match file.found {
      None => Some("The file is no longer there."),
      Some((size, modified)) if !current(&stored, size, modified) => {
        Some("The file has changed since it was analyzed.")
      }
      Some(_) => None,
    };
    let mut report = stored.report;
    report.limitations.extend(note.map(str::to_string));
    if let Some(layout) = &mut report.spectrogram {
      layout.scale = self
        .config
        .current()
        .values
        .analysis
        .spectrogram
        .scale
        .into();
    }
    if !query.windows {
      report.windows = None;
    }
    Ok(report)
  }

  async fn piece(
    &self,
    target: FileTarget,
    piece: Piece,
    headers: &HeaderMap,
  ) -> Result<Response, ApiError> {
    let file = self.files.local_path(&target).await?;
    let stored = self.stored(&file.path).await?;
    let asked = Self::kept_piece(&stored, piece)?;
    let tag = entity_tag(&stored, piece);
    let seen = headers
      .get(header::IF_NONE_MATCH)
      .and_then(|value| value.to_str().ok())
      .is_some_and(|seen| {
        seen
          .split(',')
          .map(|tag| tag.trim().trim_start_matches("W/"))
          .any(|seen| seen == tag || seen == "*")
      });
    let bytes = if seen {
      None
    } else {
      Some(self.read_kept(&stored, asked).await?.ok_or_else(|| {
        ApiError::not_found("The file is shorter than that. The layout gives the number of tiles.")
      })?)
    };
    if piece == Piece::Spectrogram(None) {
      if file.root == FileRoot::Temporary {
        lock(&self.held).touch(&file.relative);
      } else if let Err(e) = store::used(&self.db, &stored.kept, None, Utc::now()).await {
        warn!(error = %e, "spectrogram view not recorded");
      }
    }
    let mut response = match bytes {
      Some(bytes) => (StatusCode::OK, bytes).into_response(),
      None => StatusCode::NOT_MODIFIED.into_response(),
    };
    let headers = response.headers_mut();
    headers.insert(
      header::CONTENT_TYPE,
      HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
      header::CACHE_CONTROL,
      HeaderValue::from_static("private, no-cache"),
    );
    headers.insert(
      header::X_CONTENT_TYPE_OPTIONS,
      HeaderValue::from_static("nosniff"),
    );
    if let Ok(tag) = HeaderValue::from_str(&tag) {
      headers.insert(header::ETAG, tag);
    }
    Ok(response)
  }

  async fn set_override(
    &self,
    by: &Principal,
    target: FileTarget,
    verdict: SetAnalysisOverride,
  ) -> Result<(), ApiError> {
    let file = self.files.local_path(&target).await?;
    if file.root == FileRoot::Temporary {
      return Err(ApiError::bad_request(
        "A file uploaded for analysis cannot be judged",
      ));
    }
    let stored = self.stored(&file.path).await?;
    let note = verdict.note.filter(|note| !note.trim().is_empty());
    store::judge(
      &self.db,
      &stored.report.path,
      verdict.verdict,
      note,
      &by.subject,
      Utc::now(),
    )
    .await
    .map_err(ApiError::internal)?;
    info!(by = by.subject, path = stored.report.path, judgment = ?verdict.verdict, "file judged");
    Ok(())
  }

  async fn clear_override(&self, target: FileTarget) -> Result<(), ApiError> {
    let file = self.files.local_path(&target).await?;
    let path = file.path.to_string_lossy().into_owned();
    let removed = store::unjudge(&self.db, &path)
      .await
      .map_err(ApiError::internal)?;
    if removed {
      info!(path, "judgment of a file taken away");
    }
    Ok(())
  }

  async fn list(&self, query: AnalysisListQuery) -> Result<Vec<AudioAnalysis>, ApiError> {
    let listed = store::list(&self.db, &query)
      .await
      .map_err(ApiError::internal)?;
    Ok(
      listed
        .into_iter()
        .map(|listed| {
          self.unreadable(&listed.report.path, listed.unreadable.as_deref());
          listed.report
        })
        .collect(),
    )
  }

  async fn jobs(&self) -> Result<Vec<AnalysisJob>, ApiError> {
    let queue = lock(&self.queue);
    Ok(
      queue
        .running
        .iter()
        .chain(&queue.waiting)
        .map(Job::listed)
        .collect(),
    )
  }

  async fn cancel_job(&self, id: Uuid) -> Result<(), ApiError> {
    let mut queue = lock(&self.queue);
    if let Some(at) = queue.waiting.iter().position(|job| job.id == id) {
      queue.waiting.remove(at);
      debug!(job = %id, "queued analysis taken off the queue");
      return Ok(());
    }
    match queue.running.iter().find(|job| job.id == id) {
      Some(job) => {
        // the worker sees it at its next window and takes the job off the queue itself
        job.cancel.cancel();
        Ok(())
      }
      None => Err(ApiError::not_found(format!("No analysis job {id}"))),
    }
  }

  async fn window(&self, target: FileTarget, index: u32) -> Result<WindowSpectrum, ApiError> {
    let file = self.files.local_path(&target).await?;
    let stored = self.stored(&file.path).await?;
    let missing = || ApiError::not_found(format!("The report has no window {index}"));
    let windows = stored.report.windows.as_deref().unwrap_or_default();
    let window = windows.get(index as usize).ok_or_else(missing)?;
    let max_hz = stored
      .report
      .spectrogram
      .as_ref()
      .ok_or_else(missing)?
      .max_hz;
    let asked = Self::kept_piece(&stored, Piece::Spectra)?;
    let spectra = self.read_kept(&stored, asked).await?.ok_or_else(missing)?;
    let spectrum = kept::body(&spectra)
      .and_then(|body| body.as_chunks::<{ picture::BINS }>().0.get(index as usize))
      .ok_or_else(missing)?;
    Ok(WindowSpectrum {
      index,
      t: window.t,
      hz: (0..picture::BINS)
        .map(|bin| picture::bin_hz(bin, max_hz))
        .collect(),
      db: spectrum
        .iter()
        .map(|byte| picture::byte_level(*byte))
        .collect(),
    })
  }
}

/// The tag a piece of a report's pictures is served with: the same file analyzed at the same
/// time gives the same bytes.
fn entity_tag(stored: &store::Stored, piece: Piece) -> String {
  let piece = match piece {
    Piece::Spectrogram(None) => "spectrogram".to_string(),
    Piece::Spectrogram(Some(tile)) => format!("spectrogram-{tile}"),
    Piece::Waveform(None) => "waveform".to_string(),
    Piece::Waveform(Some(tile)) => format!("waveform-{tile}"),
    Piece::Spectra => "spectra".to_string(),
  };
  format!(
    "\"{}-{}-{piece}\"",
    &stored.kept[..stored.kept.len().min(16)],
    stored.report.analyzed_at.timestamp_micros()
  )
}

impl Analysis {
  /// What a viewer's `piece` is in a stored report's file, 404 when the report keeps none.
  fn kept_piece(stored: &store::Stored, piece: Piece) -> Result<kept::Asked, ApiError> {
    if stored.report.stale {
      return Err(ApiError::not_found(
        "The file was analyzed by an older version of reseek. Analyze it again to view it.",
      ));
    }
    if stored.report.spectrogram.is_none() {
      return Err(ApiError::not_found(
        "The file has no spectrogram. The limitations in its report give the reason.",
      ));
    }
    Ok(match piece {
      Piece::Spectrogram(None) => kept::Asked::Overview(tiles::Kind::Spectrogram),
      Piece::Spectrogram(Some(tile)) => kept::Asked::Tile(tiles::Kind::Spectrogram, tile),
      Piece::Waveform(None) => kept::Asked::Overview(tiles::Kind::Waveform),
      Piece::Waveform(Some(tile)) => kept::Asked::Tile(tiles::Kind::Waveform, tile),
      Piece::Spectra => kept::Asked::Spectra,
    })
  }

  /// Reads `asked` from a stored report's file; `None` for a tile past the end of the file, 404
  /// when the file is no longer kept.
  async fn read_kept(
    &self,
    stored: &store::Stored,
    asked: kept::Asked,
  ) -> Result<Option<Vec<u8>>, ApiError> {
    let path = kept::path(&self.dir, &stored.kept);
    let reading = path.clone();
    let read = tokio::task::spawn_blocking(move || kept::read(&reading, asked))
      .await
      .map_err(ApiError::internal)?;
    let not_kept = || {
      ApiError::not_found("The spectrogram is no longer kept. Analyze the file again to view it.")
    };
    match read {
      Ok(bytes) => Ok(bytes),
      Err(kept::Unkept::Missing) => Err(not_kept()),
      Err(kept::Unkept::Broken(why)) => {
        if lock(&self.unreadable).insert(path.to_string_lossy().into_owned()) {
          warn!(path = %path.display(), error = why, "a spectrogram file does not read");
        }
        Err(not_kept())
      }
    }
  }
}
