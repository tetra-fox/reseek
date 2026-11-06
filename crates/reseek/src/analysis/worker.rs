//! The analysis workers: the oldest analysis job's next waiting file is taken whenever a
//! worker is free, as many at once as `analysis.workers` allows, and analyzed on a blocking
//! thread of its own. A finished analysis is stored with its spectrogram and waveform, pushed to
//! the event stream and, unless its file was uploaded for it, raised as an event; one canceled
//! leaves nothing behind, and one a stop cut off waits again in its job.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::Instant;

use api_types::analysis::AnalysisCompleted;
use api_types::events::{Event, FileAnalysisComplete};
use api_types::files::FileRoot;
use api_types::jobs::{JobKind, JobPartState, JobState};
use api_types::live::LiveFrame;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast::error::RecvError;
use tracing::{Instrument, debug, error, info, info_span, warn};
use uuid::Uuid;

use super::analyzer::engine::{self, Settings};
use super::analyzer::reading;
use super::jobs::Running;
use super::report::{assemble, kept_name, layout, modified_text};
use super::{Analysis, kept, store, uploads};
use crate::config::Options;
use crate::jobs::{StoredPart, store as job_store};
use crate::sync::lock;

/// How a file's analysis ended.
pub(super) enum Done {
  /// analyzed, or left alone as its report is current
  Succeeded,
  /// it could not be
  Failed(String),
  /// it was canceled
  Canceled,
}

/// Starts waiting files whenever a worker is free, until the daemon stops.
pub(super) async fn dispatch(analysis: Arc<Analysis>) {
  // a job whose last file ended as the last stop came ends now, as nothing else would end it
  match analysis.jobs.unended(JobKind::Analysis).await {
    Ok(jobs) => {
      for job in jobs {
        analysis.settle(job.id).await;
      }
    }
    Err(e) => warn!(error = %e, "analysis jobs not checked at the start"),
  }
  let mut changes = analysis.config.changes();
  loop {
    analysis.start_free().await;
    tokio::select! {
      () = analysis.shutdown.cancelled() => return,
      () = analysis.wake.notified() => {}
      change = changes.recv() => match change {
        Ok(change) => {
          let limits = |options: &Options| {
            let analysis = &options.analysis;
            (analysis.cache_max_bytes, analysis.upload_cache_max_bytes)
          };
          let (pictures, uploads) = limits(&change.current.values);
          let (were_pictures, were_uploads) = limits(&change.previous.values);
          if pictures < were_pictures || uploads < were_uploads {
            analysis.evict().await;
          }
        }
        Err(RecvError::Lagged(_)) => analysis.evict().await,
        Err(RecvError::Closed) => return,
      },
    }
  }
}

impl Analysis {
  /// Starts as many waiting files as there are free workers, the oldest job's first.
  async fn start_free(self: &Arc<Self>) {
    // validation keeps a set count between 1 and the number of processors
    let workers = self.config.current().values.analysis.workers.count() as usize;
    loop {
      if lock(&self.running).len() >= workers {
        return;
      }
      let next = self
        .jobs
        .database()
        .write(|conn| {
          let tx = conn.transaction()?;
          let Some(part) = job_store::parts_in(
            &tx,
            JobPartState::Waiting,
            &[JobState::Queued, JobState::Running],
            1,
          )?
          .pop() else {
            return Ok(None);
          };
          job_store::set_part(&tx, part.id, JobPartState::Running, None)?;
          let data = job_store::data(&tx, part.job)?;
          let queued_at = job_store::get(&tx, part.job)?.map_or_else(Utc::now, |j| j.created_at);
          tx.commit()?;
          Ok(Some((part, data, queued_at)))
        })
        .await;
      let (part, data, queued_at) = match next {
        Ok(Some(next)) => next,
        Ok(None) => return,
        Err(e) => return error!(error = %e, "the next file to analyze not read"),
      };
      let force = data
        .as_ref()
        .and_then(|data| data["force"].as_bool())
        .unwrap_or(false);
      let job = part.job;
      if let Err(e) = self
        .jobs
        .update(job, |job| job.state = JobState::Running)
        .await
      {
        warn!(%job, error = %e, "analysis job not marked running");
      }
      self.take(part, queued_at, force).await;
    }
  }

  /// Takes the waiting file `part` of an analysis job: a file gone or with a current report
  /// ends at once, any other starts on a worker.
  async fn take(self: &Arc<Self>, part: StoredPart, queued_at: DateTime<Utc>, force: bool) {
    let Some(target) = self.target_of(&part) else {
      return self
        .part_done(
          part.job,
          part.id,
          Done::Failed("The file is not named".to_string()),
        )
        .await;
    };
    let file = match self.files.local_file(&target.file_target()).await {
      Ok(file) => file,
      // the file can be gone or changed since the job named it
      Err(e) => {
        return self
          .part_done(part.job, part.id, Done::Failed(e.message().to_string()))
          .await;
      }
    };
    if !force {
      match self.is_current(&file).await {
        Ok(true) => return self.part_done(part.job, part.id, Done::Succeeded).await,
        Ok(false) => {}
        Err(e) => {
          return self
            .part_done(part.job, part.id, Done::Failed(e.message().to_string()))
            .await;
        }
      }
    }
    let running = Running {
      part: part.id,
      job: part.job,
      file,
      queued_at,
      started_at: Utc::now(),
      cancel: self.shutdown.child_token(),
      progress: Arc::default(),
    };
    lock(&self.running).push(running.clone());
    debug!(part = %running.part, job = %running.job, "analysis of a file taken");
    let span = info_span!("analysis", job = %running.job, path = %running.file.path.display());
    tokio::spawn(run(self.clone(), running).instrument(span));
  }

  /// Ends the part `part` of the job `job` as `done` says and settles the job.
  pub(super) async fn part_done(&self, job: Uuid, part: Uuid, done: Done) {
    let (state, error) = match done {
      Done::Succeeded => (JobPartState::Succeeded, None),
      Done::Failed(error) => (JobPartState::Failed, Some(error)),
      Done::Canceled => (JobPartState::Canceled, None),
    };
    let written = self
      .jobs
      .database()
      .write(move |conn| job_store::set_part(conn, part, state, error.as_deref()))
      .await;
    if let Err(e) = written {
      warn!(%job, %part, error = %e, "analysis of a file not recorded in its job");
    }
    self.settle(job).await;
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

/// Runs one file and ends its part; one the daemon's stop cut off stays running in `jobs.db`,
/// so it waits again and starts over after the next start.
async fn run(analysis: Arc<Analysis>, running: Running) {
  let started = Instant::now();
  info!("analysis of a file started");
  let done = finish(&analysis, &running, started).await;
  lock(&analysis.running).retain(|r| r.part != running.part);
  if !(matches!(done, Done::Canceled) && analysis.shutdown.is_cancelled()) {
    analysis.part_done(running.job, running.part, done).await;
  }
  analysis.wake.notify_one();
}

async fn finish(analysis: &Analysis, job: &Running, started: Instant) -> Done {
  let current = analysis.config.current();
  let settings = settings(&current.values, job.progress.clone());
  let scale = current.values.analysis.spectrogram.scale.into();
  let cancel = job.cancel.clone();
  let file = job.file.clone();
  let dir = analysis.dir.clone();
  let modified = modified_text(file.modified);
  let temporary = file.root == FileRoot::Temporary;
  let hash = kept_name(&file.path.to_string_lossy(), file.size, &modified);
  let name = if temporary {
    uploads::kept_name(&file.relative, &hash)
  } else {
    hash
  };
  let kept_as = name.clone();
  let span = tracing::Span::current();
  let worked = tokio::task::spawn_blocking(move || {
    let _entered = span.enter();
    let measured = engine::run(&file.resolved, &settings, &cancel)?;
    // a job canceled since its last window leaves nothing behind
    if cancel.is_cancelled() {
      return Err(engine::Canceled);
    }
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
      return Done::Canceled;
    }
    Err(e) => {
      error!(error = %e, "analysis worker failed");
      return Done::Failed(format!("The analysis stopped: {e}"));
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
    return Done::Failed(format!("The report could not be stored: {e}"));
  }
  match bytes {
    Some(bytes) if temporary => {
      lock(&analysis.held).drawn(&job.file.relative, bytes);
      analysis.evict_uploads(None).await;
    }
    Some(bytes) => keep(analysis, &name, bytes).await,
    None => {}
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
  // the event log is history, which a file uploaded for one look stays out of
  if temporary {
    return Done::Succeeded;
  }
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
  Done::Succeeded
}

/// Records a new spectrogram and waveform file and removes the least recently viewed beyond
/// `analysis.cache_max_bytes`.
async fn keep(analysis: &Analysis, name: &str, bytes: u64) {
  if let Err(e) = store::used(&analysis.db, name, Some(bytes), Utc::now()).await {
    error!(error = %e, "spectrogram not recorded");
    return;
  }
  analysis.evict_pictures().await;
}
