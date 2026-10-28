//! Audio analysis of downloaded files: a tool the user runs to judge whether a file is what it
//! claims, such as a FLAC that is really a converted MP3. The file is decoded and its spectrum
//! measured a second at a time; the main output is a spectrogram with guide lines where
//! common encoders cut off, and the analyser's own reading comes with it as evidence. The
//! user's judgement, once recorded, is what counts. The analyser reads files under the
//! downloads and incomplete directories only and never changes one.

mod canvas;
mod decode;
mod engine;
mod flags;
mod jobs;
mod measure;
mod reading;
mod render;
mod report;
mod store;
mod thresholds;
mod totals;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use api_types::analysis::{
  AnalysisJob, AnalysisJobCreated, AnalysisListQuery, AudioAnalysis, SetAnalysisOverride,
  SpectrogramQuery,
};
use api_types::auth::Principal;
use async_trait::async_trait;
use axum::http::{HeaderValue, StatusCode, header};
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
use crate::services::{AnalysisService, FileTarget};
use crate::sync::lock;
use jobs::{Job, QUEUE_LIMIT, Queue, image_paths};
use report::modified_text;
pub use store::SCHEMA;

/// The analysis service: the queue, the reports and judgements in `analysis.db`, and the
/// spectrogram images in the data directory.
pub struct Analysis {
  db: Database,
  files: Arc<Files>,
  config: Watcher<Options>,
  live: Live,
  events: EventBus,
  /// where the spectrogram images are kept
  images: PathBuf,
  queue: Mutex<Queue>,
  /// woken when a job is queued or one ends
  wake: Notify,
  shutdown: CancellationToken,
}

impl Analysis {
  /// Opens `analysis.db` at `location`, keeps images under `data`/analysis, and starts the
  /// queue, which runs until `shutdown`.
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
    let analysis = Arc::new(Self {
      db,
      files,
      config,
      live,
      events,
      images: data.join("analysis"),
      queue: Mutex::new(Queue::default()),
      wake: Notify::new(),
      shutdown,
    });
    tokio::spawn(jobs::dispatch(analysis.clone()));
    Ok(analysis)
  }

  /// The database, for listing and closing.
  pub fn database(&self) -> &Database {
    &self.db
  }

  /// The stored report of the file at `path`, 404 when it has none.
  async fn stored(&self, path: &std::path::Path) -> Result<store::Stored, ApiError> {
    let path = path.to_string_lossy();
    let stored = store::find(&self.db, &path)
      .await
      .map_err(ApiError::internal)?
      .ok_or_else(|| ApiError::not_found(format!("'{path}' has not been analysed")))?;
    Ok(stored)
  }
}

/// The analyser's verdict on audio held in memory, read as a downloaded file is, given ten
/// seconds. It exists for fuzzing the decoders and the measures with arbitrary bytes; the
/// daemon reads files.
#[doc(hidden)]
pub fn verdict_of(bytes: Vec<u8>) -> api_types::analysis::AnalysisVerdict {
  let settings = engine::Settings {
    max_full_seconds: 900.0,
    sampled_windows: 240,
    scale: measure::Scale::Log,
    deadline: std::time::Instant::now() + std::time::Duration::from_secs(10),
  };
  match engine::read_bytes(bytes, &settings, &CancellationToken::new()) {
    Ok(measured) => reading::read(&measured).verdict,
    Err(engine::Cancelled) => unreachable!("nothing cancels this token"),
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
    if !force {
      let path = file.path.to_string_lossy().into_owned();
      let stored = store::find(&self.db, &path)
        .await
        .map_err(ApiError::internal)?;
      if stored.is_some_and(|stored| current(&stored, file.size, file.modified)) {
        return Err(ApiError::conflict(
          "the file has been analysed already",
          Some("its report is current; ask with force=true to analyse it again".to_string()),
        ));
      }
    }
    let id = Uuid::new_v4();
    {
      let mut queue = lock(&self.queue);
      if queue.holds(&file.path) {
        return Err(ApiError::conflict(
          "the file is queued or being analysed",
          None,
        ));
      }
      if queue.waiting.len() >= QUEUE_LIMIT {
        return Err(ApiError::too_many(format!(
          "the analysis queue is full at {QUEUE_LIMIT} files"
        )));
      }
      debug!(job = %id, path = %file.path.display(), waiting = queue.waiting.len(), "analysis queued");
      queue.waiting.push_back(Job {
        id,
        file,
        queued_at: Utc::now(),
        started_at: None,
      });
    }
    self.wake.notify_one();
    Ok(AnalysisJobCreated { job_id: id })
  }

  async fn report(&self, target: FileTarget, windows: bool) -> Result<AudioAnalysis, ApiError> {
    let file = self.files.local_path(&target).await?;
    let stored = self.stored(&file.path).await?;
    let note = match file.found {
      None => Some("The file is no longer there."),
      Some((size, modified)) if !current(&stored, size, modified) => {
        Some("The file has changed since it was analysed.")
      }
      Some(_) => None,
    };
    let mut report = stored.report;
    report.limitations.extend(note.map(str::to_string));
    if !windows {
      report.windows = None;
    }
    Ok(report)
  }

  async fn spectrogram(
    &self,
    target: FileTarget,
    query: SpectrogramQuery,
  ) -> Result<Response, ApiError> {
    let file = self.files.local_path(&target).await?;
    let stored = self.stored(&file.path).await?;
    if stored.report.spectrogram.is_none() {
      return Err(ApiError::not_found(
        "no spectrogram was drawn for the file; its report's limitations say why",
      ));
    }
    let (with, without) = image_paths(&self.images, &stored.image);
    let path = if query.guides { with } else { without };
    let bytes = match tokio::fs::read(&path).await {
      Ok(bytes) => bytes,
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
        return Err(ApiError::not_found(
          "the spectrogram is no longer kept; analyse the file again to draw it",
        ));
      }
      Err(e) => {
        return Err(ApiError::internal(format!(
          "the spectrogram '{}' could not be read: {e}",
          path.display()
        )));
      }
    };
    if let Err(e) = store::used_image(&self.db, &stored.image, None, Utc::now()).await {
      warn!(error = %e, "spectrogram view not recorded");
    }
    let mut response = (StatusCode::OK, bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    headers.insert(
      header::CACHE_CONTROL,
      HeaderValue::from_static("private, no-cache"),
    );
    headers.insert(
      header::X_CONTENT_TYPE_OPTIONS,
      HeaderValue::from_static("nosniff"),
    );
    Ok(response)
  }

  async fn set_override(
    &self,
    by: &Principal,
    target: FileTarget,
    verdict: SetAnalysisOverride,
  ) -> Result<(), ApiError> {
    let file = self.files.local_path(&target).await?;
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
    info!(by = by.subject, path = stored.report.path, judgement = ?verdict.verdict, "file judged");
    Ok(())
  }

  async fn clear_override(&self, target: FileTarget) -> Result<(), ApiError> {
    let file = self.files.local_path(&target).await?;
    let path = file.path.to_string_lossy().into_owned();
    let removed = store::unjudge(&self.db, &path)
      .await
      .map_err(ApiError::internal)?;
    if removed {
      info!(path, "judgement of a file taken away");
    }
    Ok(())
  }

  async fn list(&self, query: AnalysisListQuery) -> Result<Vec<AudioAnalysis>, ApiError> {
    store::list(&self.db, &query)
      .await
      .map_err(ApiError::internal)
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
}
