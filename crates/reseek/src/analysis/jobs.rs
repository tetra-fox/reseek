//! The analysis queue: files wait in the order they were asked for, by the API or by a download
//! that finished while `analysis.on_download` is on, and as many run as `analysis.workers`
//! allows, each on a blocking thread of its own. A finished analysis is stored with its
//! spectrogram and waveform, pushed to the event stream and raised as an event; one canceled by
//! shutdown leaves nothing behind.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use api_types::analysis::{AnalysisCompleted, AnalysisJob, AnalysisJobState};
use api_types::events::{Event, FileAnalysisComplete};
use api_types::live::LiveFrame;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, warn};
use uuid::Uuid;

use super::analyzer::engine::{self, Settings};
use super::analyzer::reading;
use super::report::{assemble, kept_name, layout, modified_text};
use super::{Analysis, current, kept, store};
use crate::api::error::ApiError;
use crate::config::Options;
use crate::files::LocalFile;
use crate::services::FileTarget;
use crate::sync::lock;

/// The most files that may wait in the queue.
pub(super) const QUEUE_LIMIT: usize = 1000;

/// A file waiting for or under analysis.
#[derive(Clone, Debug)]
pub(super) struct Job {
  pub id: Uuid,
  pub file: LocalFile,
  pub queued_at: DateTime<Utc>,
  pub started_at: Option<DateTime<Utc>>,
  /// canceled when the job is canceled or the daemon stops
  pub cancel: CancellationToken,
  /// how far the worker is, in thousandths of the file
  pub progress: Arc<AtomicU32>,
}

impl Job {
  /// The job as the queue listing shows it.
  pub fn listed(&self) -> AnalysisJob {
    AnalysisJob {
      id: self.id,
      path: self.file.path.to_string_lossy().into_owned(),
      root: self.file.root,
      relative_path: self.file.relative.clone(),
      transfer_id: self.file.transfer_id,
      state: if self.started_at.is_some() {
        AnalysisJobState::Running
      } else {
        AnalysisJobState::Queued
      },
      queued_at: self.queued_at,
      started_at: self.started_at,
      progress: self
        .started_at
        .map(|_| f64::from(self.progress.load(Ordering::Relaxed)) / 1000.0),
    }
  }
}

/// The jobs waiting and running.
#[derive(Debug, Default)]
pub(super) struct Queue {
  pub waiting: VecDeque<Job>,
  pub running: Vec<Job>,
}

impl Queue {
  /// Whether `path` is waiting or running.
  pub fn holds(&self, path: &Path) -> bool {
    self
      .waiting
      .iter()
      .chain(&self.running)
      .any(|job| job.file.path == path)
  }
}

/// What asking for a file's analysis came to.
pub(super) enum Queued {
  /// queued as this job
  Job(Uuid),
  /// its report is current, so it was left alone
  Current,
  /// it is queued or running already
  Already,
  /// the queue holds as many files as it may
  Full,
}

impl Analysis {
  /// Queues `file`, unless its report is current and not `force`d, it is in the queue already
  /// or the queue is full.
  pub(super) async fn enqueue(&self, file: LocalFile, force: bool) -> Result<Queued, ApiError> {
    if !force {
      let path = file.path.to_string_lossy().into_owned();
      let stored = store::find(&self.db, &path)
        .await
        .map_err(ApiError::internal)?;
      // a stale report is no reading of the file, so it is analyzed again
      let current =
        |stored: &store::Stored| !stored.report.stale && current(stored, file.size, file.modified);
      if stored.as_ref().is_some_and(current) {
        return Ok(Queued::Current);
      }
    }
    let id = Uuid::new_v4();
    {
      let mut queue = lock(&self.queue);
      if queue.holds(&file.path) {
        return Ok(Queued::Already);
      }
      if queue.waiting.len() >= QUEUE_LIMIT {
        return Ok(Queued::Full);
      }
      debug!(job = %id, path = %file.path.display(), waiting = queue.waiting.len(), "analysis queued");
      queue.waiting.push_back(Job {
        id,
        file,
        queued_at: Utc::now(),
        started_at: None,
        cancel: self.shutdown.child_token(),
        progress: Arc::default(),
      });
    }
    self.wake.notify_one();
    Ok(Queued::Job(id))
  }

  /// Queues the file the download `id` from `username` finished into at `path`, when
  /// `analysis.on_download` is on and the file's extension is in `analysis.extensions`. A full
  /// queue drops it, said once until the queue takes a download's file again.
  pub async fn downloaded(&self, username: &str, id: Uuid, path: &Path) {
    let options = self.config.current();
    let analysis = &options.values.analysis;
    if !analysis.on_download || !listed(&analysis.extensions, path) {
      return;
    }
    let target = FileTarget::Download {
      username: username.to_string(),
      id,
    };
    let queued = match self.files.local_file(&target).await {
      Ok(file) => self.enqueue(file, false).await,
      Err(e) => Err(e),
    };
    match queued {
      Ok(Queued::Job(_)) => {
        self.full.store(false, Ordering::Relaxed);
      }
      Ok(Queued::Current | Queued::Already) => {
        debug!(path = %path.display(), "finished download analyzed or queued already");
      }
      Ok(Queued::Full) => {
        if !self.full.swap(true, Ordering::Relaxed) {
          warn!(
            path = %path.display(),
            limit = QUEUE_LIMIT,
            "analysis queue full; finished downloads are not analyzed until it has room"
          );
        }
      }
      Err(e) => {
        warn!(path = %path.display(), error = ?e, "finished download not queued for analysis")
      }
    }
  }
}

/// Whether the extension of `path` is one of `extensions`, ignoring case.
fn listed(extensions: &[String], path: &Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      extensions
        .iter()
        .any(|listed| listed.eq_ignore_ascii_case(extension))
    })
}

/// Starts queued jobs whenever a worker is free, until the daemon stops.
pub(super) async fn dispatch(analysis: Arc<Analysis>) {
  let mut changes = analysis.config.changes();
  loop {
    start_free(&analysis);
    tokio::select! {
      () = analysis.shutdown.cancelled() => return,
      () = analysis.wake.notified() => {}
      change = changes.recv() => match change {
        Ok(change) => {
          let limit = |options: &Options| options.analysis.cache_max_bytes;
          if limit(&change.current.values) < limit(&change.previous.values) {
            evict(&analysis).await;
          }
        }
        Err(RecvError::Lagged(_)) => evict(&analysis).await,
        Err(RecvError::Closed) => return,
      },
    }
  }
}

/// Starts as many waiting jobs as there are free workers.
fn start_free(analysis: &Arc<Analysis>) {
  // validation keeps a set count between 1 and the number of processors
  let workers = analysis.config.current().values.analysis.workers.count() as usize;
  loop {
    let job = {
      let mut queue = lock(&analysis.queue);
      if queue.running.len() >= workers {
        return;
      }
      let Some(mut job) = queue.waiting.pop_front() else {
        return;
      };
      job.started_at = Some(Utc::now());
      queue.running.push(job.clone());
      debug!(job = %job.id, running = queue.running.len(), workers, "analysis job taken");
      job
    };
    let span = info_span!("analysis", job = %job.id, path = %job.file.path.display());
    tokio::spawn(run(analysis.clone(), job).instrument(span));
  }
}

/// How an analysis runs under the options now.
fn settings(options: &Options, progress: Arc<AtomicU32>) -> Settings {
  let analysis = &options.analysis;
  Settings {
    max_full_seconds: f64::from(analysis.max_full_minutes) * 60.0,
    sampled_windows: analysis.sampled_windows as usize,
    deadline: Instant::now() + analysis.timeout,
    progress: Some(progress),
  }
}

/// Runs one job and takes it off the queue.
async fn run(analysis: Arc<Analysis>, job: Job) {
  let started = Instant::now();
  info!("analysis of a file started");
  finish(&analysis, &job, started).await;
  lock(&analysis.queue)
    .running
    .retain(|running| running.id != job.id);
  analysis.wake.notify_one();
}

async fn finish(analysis: &Analysis, job: &Job, started: Instant) {
  let current = analysis.config.current();
  let settings = settings(&current.values, job.progress.clone());
  let scale = current.values.analysis.spectrogram.scale.into();
  let cancel = job.cancel.clone();
  let file = job.file.clone();
  let dir = analysis.dir.clone();
  let modified = modified_text(file.modified);
  let name = kept_name(&file.path.to_string_lossy(), file.size, &modified);
  let kept_as = name.clone();
  let span = tracing::Span::current();
  let worked = tokio::task::spawn_blocking(move || {
    let _entered = span.enter();
    let measured = engine::run(&file.resolved, &settings, &cancel)?;
    let read = reading::read(&measured);
    let mut problem = None;
    let drawn = measured.claimed.as_ref().and_then(|claimed| {
      let max_hz = f64::from(claimed.sample_rate) / 2.0;
      let contents = kept::contents(&measured.windows, measured.duration, max_hz);
      match kept::write(&dir, &kept_as, &contents) {
        Ok(bytes) => {
          let layout = layout(
            measured.duration,
            max_hz,
            measured.sampled,
            read.cutoff_hz,
            scale,
          );
          Some((layout, bytes))
        }
        Err(e) => {
          warn!(error = e, "spectrogram not kept");
          problem = Some(format!("The spectrogram could not be stored: {e}"));
          None
        }
      }
    });
    let (layout, bytes) = drawn.unzip();
    let mut report = assemble(&file, &measured, read, layout, Utc::now());
    report.limitations.extend(problem);
    Ok((report, bytes))
  })
  .await;
  let (report, bytes) = match worked {
    Ok(Ok(done)) => done,
    Ok(Err(engine::Canceled)) => {
      debug!("analysis canceled");
      return;
    }
    Err(e) => {
      error!(error = %e, "analysis worker failed");
      return;
    }
  };
  let saved = store::save(
    &analysis.db,
    store::NewReport {
      report: &report,
      modified: &modified,
      kept: &name,
    },
  )
  .await;
  if let Err(e) = saved {
    error!(error = %e, "analysis report not stored");
    return;
  }
  if let Some(bytes) = bytes {
    keep(analysis, &name, bytes).await;
  }
  info!(
    verdict = ?report.verdict,
    confidence = report.confidence,
    seconds = started.elapsed().as_secs_f64(),
    "analysis of a file finished"
  );
  analysis
    .live
    .publish(LiveFrame::FilesAnalysis(AnalysisCompleted {
      transfer_id: job.file.transfer_id,
      path: report.path.clone(),
      verdict: report.verdict,
      confidence: report.confidence,
      flags: report.flags.clone(),
    }));
  let judged = store::find(&analysis.db, &report.path).await;
  let judgment = match judged {
    Ok(found) => found.and_then(|stored| stored.report.judgment),
    Err(e) => {
      warn!(error = %e, "the judgment of an analyzed file not read for its event");
      None
    }
  };
  analysis
    .events
    .raise(Event::FileAnalysisComplete(FileAnalysisComplete {
      version: 0,
      report: Box::new(api_types::analysis::AudioAnalysis {
        windows: None,
        judgment,
        ..report
      }),
    }));
}

/// Records a new spectrogram and waveform file and removes the least recently viewed beyond
/// `analysis.cache_max_bytes`.
async fn keep(analysis: &Analysis, name: &str, bytes: u64) {
  if let Err(e) = store::used(&analysis.db, name, Some(bytes), Utc::now()).await {
    error!(error = %e, "spectrogram not recorded");
    return;
  }
  evict(analysis).await;
}

/// Removes the least recently viewed spectrogram and waveform files beyond
/// `analysis.cache_max_bytes`.
async fn evict(analysis: &Analysis) {
  let limit = analysis
    .config
    .current()
    .values
    .analysis
    .cache_max_bytes
    .as_u64();
  let gone = match store::evict(&analysis.db, limit).await {
    Ok(gone) => gone,
    Err(e) => {
      error!(error = %e, "spectrograms over the limit not removed");
      return;
    }
  };
  if gone.is_empty() {
    return;
  }
  debug!(
    files = gone.len(),
    limit, "spectrograms over the limit removed"
  );
  let dir = analysis.dir.clone();
  let removed = tokio::task::spawn_blocking(move || {
    for name in gone {
      let path = kept::path(&dir, &name);
      if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
      {
        warn!(path = %path.display(), error = %e, "spectrogram not removed");
      }
    }
  })
  .await;
  if let Err(e) = removed {
    error!(error = %e, "removing spectrograms failed");
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::listed;

  #[test]
  fn listed_extensions_match_without_case_and_only_the_last_one() {
    let extensions = ["flac".to_string(), "wav".to_string()];
    assert!(listed(&extensions, Path::new("/d/album/01 song.FLAC")));
    assert!(listed(&extensions, Path::new("/d/a.mp3.wav")));
    assert!(!listed(&extensions, Path::new("/d/a.flac.mp3")));
    assert!(!listed(&extensions, Path::new("/d/flac")));
    assert!(!listed(&[], Path::new("/d/a.flac")));
  }
}
