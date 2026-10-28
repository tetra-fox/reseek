//! The analysis queue: files wait in the order they were asked for and as many run as
//! `analysis.workers` allows, each on a blocking thread of its own. A finished analysis is
//! stored with its images, pushed to the event stream and raised as an event; one cancelled
//! by shutdown leaves nothing behind.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
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

use super::engine::{self, Settings};
use super::measure::Scale;
use super::render::{Picture, render};
use super::report::{assemble, image_name, modified_text};
use super::{Analysis, reading, store};
use crate::config::Options;
use crate::config::enums::SpectrogramScale;
use crate::files::LocalFile;
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
  /// cancelled when the job is cancelled or the daemon stops
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

/// The images of a report: with guide lines, and without.
pub(super) fn image_paths(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
  (
    dir.join(format!("{name}.png")),
    dir.join(format!("{name}.plain.png")),
  )
}

/// The spectrogram column of every window of a report, one after another, kept beside its
/// images for showing one window's spectrum.
pub(super) fn columns_path(dir: &Path, name: &str) -> PathBuf {
  dir.join(format!("{name}.columns"))
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
  let workers = analysis.config.current().values.analysis.workers.max(1) as usize;
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
    scale: match analysis.spectrogram.scale {
      SpectrogramScale::Log => Scale::Log,
      SpectrogramScale::Linear => Scale::Linear,
    },
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
  let settings = settings(&analysis.config.current().values, job.progress.clone());
  let cancel = job.cancel.clone();
  let file = job.file.clone();
  let images = analysis.images.clone();
  let modified = modified_text(file.modified);
  let name = image_name(&file.path.to_string_lossy(), file.size, &modified);
  let image = name.clone();
  let span = tracing::Span::current();
  let worked = tokio::task::spawn_blocking(move || {
    let _entered = span.enter();
    let measured = engine::run(&file.resolved, &settings, &cancel)?;
    let read = reading::read(&measured);
    let mut problem = None;
    let drawn = match &measured.claimed {
      Some(claimed) => {
        let picture = Picture {
          windows: &measured.windows,
          rate: claimed.sample_rate,
          scale: settings.scale,
          sampled: measured.sampled,
          cutoff_hz: read.cutoff_hz,
        };
        let kept = render(&picture)
          .map_err(|e| e.to_string())
          .and_then(|rendered| {
            let columns: Vec<u8> = measured
              .windows
              .iter()
              .flat_map(|w| w.column.iter().copied())
              .collect();
            write_images(&images, &image, &rendered.guided, &rendered.plain, &columns)?;
            Ok((rendered, columns.len()))
          });
        match kept {
          Ok((rendered, columns)) => {
            let bytes = (rendered.guided.len() + rendered.plain.len() + columns) as u64;
            Some((rendered.layout, bytes))
          }
          Err(e) => {
            warn!(error = e, "spectrogram not kept");
            problem = Some(format!("The spectrogram could not be kept: {e}"));
            None
          }
        }
      }
      None => None,
    };
    let (layout, bytes) = drawn.unzip();
    let mut report = assemble(&file, &measured, read, layout, Utc::now());
    report.limitations.extend(problem);
    Ok((report, bytes))
  })
  .await;
  let (report, bytes) = match worked {
    Ok(Ok(done)) => done,
    Ok(Err(engine::Cancelled)) => {
      debug!("analysis cancelled");
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
      image: &name,
    },
  )
  .await;
  if let Err(e) = saved {
    error!(error = %e, "analysis report not stored");
    return;
  }
  if let Some(bytes) = bytes {
    keep_images(analysis, &name, bytes).await;
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
  let judgement = match judged {
    Ok(found) => found.and_then(|stored| stored.report.judgement),
    Err(e) => {
      warn!(error = %e, "the judgement of an analysed file not read for its event");
      None
    }
  };
  analysis
    .events
    .raise(Event::FileAnalysisComplete(FileAnalysisComplete {
      version: 0,
      report: Box::new(api_types::analysis::AudioAnalysis {
        windows: None,
        judgement,
        ..report
      }),
    }));
}

/// Writes a report's two images and its windows' columns under `dir`.
fn write_images(
  dir: &Path,
  name: &str,
  guided: &[u8],
  plain: &[u8],
  columns: &[u8],
) -> Result<(), String> {
  std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  let (with, without) = image_paths(dir, name);
  let all = [
    (with, guided),
    (without, plain),
    (columns_path(dir, name), columns),
  ];
  for (path, bytes) in all {
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
  }
  Ok(())
}

/// Records new images and removes the least recently viewed beyond `analysis.cache_max_bytes`.
async fn keep_images(analysis: &Analysis, name: &str, bytes: u64) {
  if let Err(e) = store::used_image(&analysis.db, name, Some(bytes), Utc::now()).await {
    error!(error = %e, "spectrogram not recorded");
    return;
  }
  evict(analysis).await;
}

/// Removes the least recently viewed images beyond `analysis.cache_max_bytes`.
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
    images = gone.len(),
    limit, "spectrograms over the limit removed"
  );
  let dir = analysis.images.clone();
  let removed = tokio::task::spawn_blocking(move || {
    for name in gone {
      let (with, without) = image_paths(&dir, &name);
      for path in [with, without, columns_path(&dir, &name)] {
        if let Err(e) = std::fs::remove_file(&path)
          && e.kind() != std::io::ErrorKind::NotFound
        {
          warn!(path = %path.display(), error = %e, "spectrogram not removed");
        }
      }
    }
  })
  .await;
  if let Err(e) = removed {
    error!(error = %e, "removing spectrograms failed");
  }
}
