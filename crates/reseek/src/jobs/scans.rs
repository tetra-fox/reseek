//! Share scans as jobs. The share engine tells of each scan as it starts and ends: a scan
//! someone asked for or the one at startup is listed from its start, while a scheduled rescan
//! is listed at its end only when it did not complete; every scheduled one updates the share
//! rescan's card.

use std::sync::{Arc, Mutex};

use api_types::jobs::{Job, JobCounts, JobEnding, JobKind, JobStarter, JobStarterKind, JobState};
use chrono::{DateTime, Utc};
use reseek_shares::{ScanEnded, ScanHook, ScanNotice, ScanOrigin, SharedCounts};
use tracing::warn;
use uuid::Uuid;

use super::{Ended, Jobs, NewJob};
use crate::sync::lock;

/// The schedule a share rescan runs on.
pub const SHARE_RESCAN: &str = "share-rescan";

/// The scan going on.
struct Running {
  /// its job, when it is listed from its start
  job: Option<Uuid>,
  name: &'static str,
  origin: ScanOrigin,
  started_at: DateTime<Utc>,
}

/// The hook the share engine tells of its scans, keeping them as jobs in `jobs`.
pub fn hook(jobs: Arc<Jobs>) -> ScanHook {
  let running: Arc<Mutex<Option<Running>>> = Arc::default();
  Box::new(move |notice| {
    let jobs = jobs.clone();
    let running = running.clone();
    Box::pin(async move {
      match notice {
        ScanNotice::Started { force, origin } => started(&jobs, &running, force, origin).await,
        ScanNotice::Ended(ended) => {
          let scan = lock(&running).take();
          match scan {
            Some(scan) => finished(&jobs, scan, ended).await,
            None => warn!("a share scan ended that was not told of as started"),
          }
        }
      }
    })
  })
}

/// The job name of a scan.
fn name(force: bool) -> &'static str {
  if force {
    "Full share scan"
  } else {
    "Share scan"
  }
}

/// Who started a scan of `origin`.
fn starter(origin: ScanOrigin) -> JobStarter {
  match origin {
    ScanOrigin::Asked => JobStarter::you(),
    ScanOrigin::Scheduled => JobStarter::schedule(SHARE_RESCAN),
    ScanOrigin::Startup => JobStarter::of(JobStarterKind::Resumed),
  }
}

async fn started(jobs: &Jobs, running: &Mutex<Option<Running>>, force: bool, origin: ScanOrigin) {
  let name = name(force);
  let job = if origin == ScanOrigin::Scheduled {
    None
  } else {
    let mut new = NewJob::running(JobKind::Scan, name, starter(origin));
    new.data = Some(serde_json::json!({ "force": force }));
    match jobs.create(new).await {
      Ok(job) => Some(job.id),
      Err(e) => {
        warn!(error = %e, "share scan not listed as a job");
        None
      }
    }
  };
  *lock(running) = Some(Running {
    job,
    name,
    origin,
    started_at: super::now(),
  });
  if origin == ScanOrigin::Scheduled {
    jobs.schedules_changed.notify_one();
  }
}

/// What a scan's end means for its job.
fn outcome(ended: &ScanEnded) -> (JobEnding, Option<String>, Option<String>) {
  match ended {
    ScanEnded::Completed(SharedCounts { directories, files }) => (
      JobEnding::Succeeded,
      None,
      Some(format!(
        "{} in {}",
        super::counted(*files, "file", "files"),
        super::counted(*directories, "folder", "folders")
      )),
    ),
    ScanEnded::Canceled => (JobEnding::Canceled, None, None),
    ScanEnded::Failed(error) => (JobEnding::Failed, Some(error.clone()), None),
  }
}

async fn finished(jobs: &Jobs, scan: Running, ended: ScanEnded) {
  let (ending, error, summary) = outcome(&ended);
  let now = super::now();
  let mut job_id = scan.job;
  match scan.job {
    Some(id) => {
      let end = Ended {
        ending: Some(ending),
        error: error.clone(),
        summary: summary.clone(),
        parts: None,
        counts: None,
      };
      if let Err(e) = jobs.end(id, end).await {
        warn!(job = %id, error = %e, "share scan job not ended");
      }
    }
    None if ending != JobEnding::Succeeded => {
      let job = Job {
        id: Uuid::new_v4(),
        kind: JobKind::Scan,
        name: scan.name.to_string(),
        starter: starter(scan.origin),
        state: JobState::Ended,
        ending: Some(ending),
        error: error.clone(),
        summary: summary.clone(),
        counts: JobCounts::default(),
        progress: None,
        follows: None,
        pausable: false,
        created_at: scan.started_at,
        started_at: Some(scan.started_at),
        ended_at: Some(now),
      };
      job_id = Some(job.id);
      let data = serde_json::json!({ "force": scan.name == name(true) });
      if let Err(e) = jobs.record(job, Vec::new(), Some(data)).await {
        warn!(error = %e, "scheduled share scan not listed");
      }
    }
    None => {}
  }
  let scheduled = scan.origin == ScanOrigin::Scheduled;
  let started_at = scan.started_at;
  jobs
    .update_last_run(SHARE_RESCAN, move |last| {
      if scheduled {
        last.scheduled_at = Some(started_at);
      }
      last.run_at = Some(started_at);
      last.ended_at = Some(now);
      last.ending = Some(ending);
      last.error = error;
      last.summary = summary;
      last.job_id = job_id;
    })
    .await;
}
