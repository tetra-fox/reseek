//! Maintenance jobs: the cleanup, which deletes what the retention options let go, and the
//! database vacuum. A run someone started is listed from its start; a scheduled run is listed
//! at its end only when it removed something or failed.

use std::future::Future;

use api_types::jobs::{Job, JobCounts, JobEnding, JobKind, JobPartState, JobStarter, JobState};
use bytesize::ByteSize;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uuid::Uuid;

use super::{Ended, NewJob, NewPart, RunOutcome};
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::app::tasks;
use crate::transfers::prune;

/// What a maintenance job runs, kept with the job so it can be run again: `{"run": "cleanup"}`,
/// `{"run": "vacuum"}` for every database, `{"run": "vacuum", "database": name}` for one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "run", rename_all = "lowercase")]
pub enum MaintenanceRun {
  /// deletes what the retention options let go
  Cleanup,
  /// vacuums one database, or every one when none is named
  Vacuum {
    /// the database's name, as `transfers` for `transfers.db`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    database: Option<String>,
  },
}

/// One area of a run: its name and what came of it, a count of what it removed or why it
/// failed.
struct Done {
  name: String,
  result: Result<u64, String>,
}

/// A run of steps as a job: listed from its start when `listed`, else listed at its end only
/// when it removed something or failed.
struct Run<'a> {
  app: &'a AppState,
  name: String,
  /// what a run again repeats
  run: MaintenanceRun,
  starter: JobStarter,
  listed: Option<Uuid>,
  started_at: DateTime<Utc>,
  cancel: CancellationToken,
  done: Vec<Done>,
}

impl<'a> Run<'a> {
  async fn start(
    app: &'a AppState,
    name: String,
    run: MaintenanceRun,
    starter: JobStarter,
    listed: bool,
  ) -> Self {
    let cancel = app.shutdown.child_token();
    let job = if listed {
      match app
        .jobs
        .create(NewJob {
          data: Some(run.data()),
          ..NewJob::running(JobKind::Maintenance, name.clone(), starter.clone())
        })
        .await
      {
        Ok(job) => {
          app.jobs.hold_cancel(job.id, cancel.clone());
          Some(job.id)
        }
        Err(e) => {
          warn!(error = %e, job = name, "maintenance run not listed");
          None
        }
      }
    } else {
      None
    };
    Self {
      app,
      name,
      run,
      starter,
      listed: job,
      started_at: super::now(),
      cancel,
      done: Vec::new(),
    }
  }

  /// Runs the step `name` unless the run was canceled, `of` steps in all.
  async fn step<F>(&mut self, name: &str, of: usize, work: F)
  where
    F: Future<Output = Result<u64, String>>,
  {
    if self.cancel.is_cancelled() {
      return;
    }
    let result = work.await;
    match &result {
      Ok(count) => debug!(step = name, count, "maintenance step done"),
      Err(e) => warn!(step = name, error = e, "maintenance step failed"),
    }
    self.done.push(Done {
      name: name.to_string(),
      result,
    });
    if let Some(id) = self.listed {
      let fraction = self.done.len() as f64 / of.max(1) as f64;
      self.app.jobs.progress(id, fraction).await;
    }
  }

  /// Ends the run, its summary made by `summary` from the counts of the steps that succeeded,
  /// and the steps never run because of a cancel listed as canceled from `remaining`.
  async fn end(self, summary: Option<String>, remaining: &[&str]) -> RunOutcome {
    let canceled = self.cancel.is_cancelled();
    let mut parts: Vec<NewPart> = self
      .done
      .iter()
      .map(|done| NewPart {
        name: done.name.clone(),
        state: if done.result.is_ok() {
          JobPartState::Succeeded
        } else {
          JobPartState::Failed
        },
        error: done.result.as_ref().err().cloned(),
        target: None,
        key: None,
      })
      .collect();
    if canceled {
      parts.extend(remaining.iter().skip(self.done.len()).map(|name| NewPart {
        name: (*name).to_string(),
        state: JobPartState::Canceled,
        error: None,
        target: None,
        key: None,
      }));
    }
    let failed: Vec<&Done> = self.done.iter().filter(|d| d.result.is_err()).collect();
    let ending = if canceled {
      JobEnding::Canceled
    } else if failed.is_empty() {
      JobEnding::Succeeded
    } else if failed.len() == self.done.len() {
      JobEnding::Failed
    } else {
      JobEnding::FinishedWithFailures
    };
    let error = (!failed.is_empty()).then(|| {
      failed
        .iter()
        .map(|d| format!("{}: {}", d.name, d.result.as_ref().err().map_or("", |e| e)))
        .collect::<Vec<_>>()
        .join("; ")
    });
    let removed: u64 = self
      .done
      .iter()
      .filter_map(|d| d.result.as_ref().ok())
      .sum();
    let counts = JobCounts {
      total: u32::try_from(parts.len()).unwrap_or(u32::MAX),
      succeeded: count(&parts, JobPartState::Succeeded),
      failed: count(&parts, JobPartState::Failed),
      canceled: count(&parts, JobPartState::Canceled),
      paused: 0,
    };
    let mut job_id = self.listed;
    match self.listed {
      Some(id) => {
        let ended = Ended {
          ending: Some(ending),
          error: error.clone(),
          summary: summary.clone(),
          parts: Some(parts),
          counts: Some(counts),
        };
        if let Err(e) = self.app.jobs.end(id, ended).await {
          warn!(job = %id, error = %e, "maintenance job not ended");
        }
      }
      None if removed > 0 || ending != JobEnding::Succeeded => {
        let now = super::now();
        let job = Job {
          id: Uuid::new_v4(),
          kind: JobKind::Maintenance,
          name: self.name.clone(),
          starter: self.starter.clone(),
          state: JobState::Ended,
          ending: Some(ending),
          error: error.clone(),
          summary: summary.clone(),
          counts,
          progress: None,
          follows: None,
          pausable: false,
          created_at: self.started_at,
          started_at: Some(self.started_at),
          ended_at: Some(now),
        };
        job_id = Some(job.id);
        if let Err(e) = self
          .app
          .jobs
          .record(job, parts, Some(self.run.data()))
          .await
        {
          warn!(error = %e, job = self.name, "maintenance run not listed");
        }
      }
      None => {}
    }
    RunOutcome {
      ending,
      error,
      summary,
      job_id,
    }
  }
}

fn count(parts: &[NewPart], state: JobPartState) -> u32 {
  u32::try_from(parts.iter().filter(|p| p.state == state).count()).unwrap_or(u32::MAX)
}

/// The areas of a cleanup, in the order it runs them.
const CLEANUP: [&str; 9] = [
  "Finished transfers",
  "Searches",
  "Private messages",
  "Room messages",
  "Events",
  "Users",
  "Downloaded files",
  "Sessions and tokens",
  "Jobs",
];

/// What each area of a cleanup removes, one and many.
const REMOVED: [(&str, &str); 9] = [
  ("finished transfer", "finished transfers"),
  ("search", "searches"),
  ("private message", "private messages"),
  ("room message", "room messages"),
  ("event", "events"),
  ("user", "users"),
  ("file", "files"),
  ("session or token", "sessions and tokens"),
  ("ended job", "ended jobs"),
];

/// Deletes what the retention options let go, area by area, as a maintenance job started by
/// `starter`; listed from its start when `listed`.
pub async fn cleanup(app: &AppState, starter: JobStarter, listed: bool) -> RunOutcome {
  let options = app.config.current();
  let retention = &options.values.retention;
  let mut run = Run::start(
    app,
    "Cleanup".to_string(),
    MaintenanceRun::Cleanup,
    starter,
    listed,
  )
  .await;
  let of = CLEANUP.len();
  let failed = |e: crate::db::DbError| e.to_string();
  let wide = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
  run
    .step(CLEANUP[0], of, async {
      prune::prune(&app.transfers, &app.live, &retention.transfers)
        .await
        .map(wide)
        .map_err(failed)
    })
    .await;
  run
    .step(CLEANUP[1], of, async {
      app.searches.prune().await.map(wide).map_err(failed)
    })
    .await;
  run
    .step(CLEANUP[2], of, async {
      app
        .messaging
        .prune_private()
        .await
        .map(wide)
        .map_err(failed)
    })
    .await;
  run
    .step(CLEANUP[3], of, async {
      app.messaging.prune_rooms().await.map(wide).map_err(failed)
    })
    .await;
  run
    .step(CLEANUP[4], of, async {
      app
        .event_log
        .prune(Utc::now(), retention.events, retention.events_max_rows)
        .await
        .map(|(aged, capped)| wide(aged + capped))
        .map_err(failed)
    })
    .await;
  run
    .step(CLEANUP[5], of, async {
      app.users.forget_unseen().await.map(wide).map_err(failed)
    })
    .await;
  run
    .step(CLEANUP[6], of, async {
      let pruned = app.files.prune().await.map(u64::from);
      // the reports of pruned files, and of files deleted in any other way
      if let Err(e) = app.analysis.forget_missing().await {
        warn!(error = %e, "reports of files no longer on disk not removed");
      }
      pruned
    })
    .await;
  run
    .step(CLEANUP[7], of, async {
      tasks::prune_sessions(app).await.map(wide).map_err(failed)
    })
    .await;
  let jobs = &retention.jobs;
  let age = (!jobs.age.is_zero())
    .then(|| chrono::Duration::from_std(jobs.age).unwrap_or(chrono::Duration::MAX));
  let keep = (jobs.count > 0).then_some(jobs.count);
  run
    .step(CLEANUP[8], of, async {
      app.jobs.prune(age, keep).await.map(wide).map_err(failed)
    })
    .await;
  let removed: Vec<String> = run
    .done
    .iter()
    .zip(REMOVED)
    .filter_map(|(done, (one, many))| match done.result {
      Ok(n) if n > 0 => Some(super::counted(n, one, many)),
      _ => None,
    })
    .collect();
  let summary = if removed.is_empty() {
    "Nothing to remove".to_string()
  } else {
    format!("Removed {}", removed.join(", "))
  };
  run.end(Some(summary), &CLEANUP).await
}

/// Vacuums every database, as a maintenance job started by `starter`; listed from its start
/// when `listed`.
pub async fn vacuum(app: &AppState, starter: JobStarter, listed: bool) -> RunOutcome {
  let names = app.databases.names();
  let mut run = Run::start(
    app,
    "Database vacuum".to_string(),
    MaintenanceRun::Vacuum { database: None },
    starter,
    listed,
  )
  .await;
  for name in &names {
    run
      .step(&format!("{name}.db"), names.len(), async {
        app
          .databases
          .vacuum(name)
          .await
          .map_err(|e| e.message().to_string())
      })
      .await;
  }
  let freed: u64 = run.done.iter().filter_map(|d| d.result.as_ref().ok()).sum();
  let summary = format!("Freed {}", ByteSize(freed));
  let names: Vec<String> = names.iter().map(|name| format!("{name}.db")).collect();
  let remaining: Vec<&str> = names.iter().map(String::as_str).collect();
  run.end(Some(summary), &remaining).await
}

/// Vacuums the database `name` as a job of its own started by the user, answering once it
/// ended: 404 for a name no file has, 500 when the vacuum failed.
pub async fn vacuum_one(app: &AppState, name: &str) -> Result<(), ApiError> {
  if !is_database(app, name) {
    return Err(ApiError::not_found(format!("No database named {name}")));
  }
  let mut run = Run::start(
    app,
    format!("Vacuum {name}.db"),
    MaintenanceRun::Vacuum {
      database: Some(name.to_string()),
    },
    JobStarter::you(),
    true,
  )
  .await;
  let mut failure = None;
  run
    .step(&format!("{name}.db"), 1, async {
      app.databases.vacuum(name).await.map_err(|e| {
        let message = e.message().to_string();
        failure = Some(e);
        message
      })
    })
    .await;
  let freed: u64 = run.done.iter().filter_map(|d| d.result.as_ref().ok()).sum();
  run
    .end(Some(format!("Freed {}", ByteSize(freed))), &[])
    .await;
  match failure {
    Some(e) => Err(e),
    None => Ok(()),
  }
}

fn is_database(app: &AppState, name: &str) -> bool {
  app.databases.names().iter().any(|known| known == name)
}

/// Runs the maintenance run `data` describes again, started by the user, in the background.
/// 409 when it cannot be: kept in a shape this version does not read, or of a database that is
/// gone.
pub fn run_again(app: &AppState, data: Option<serde_json::Value>) -> Result<(), ApiError> {
  let refused = |detail: String| ApiError::conflict("This job cannot be run again", Some(detail));
  let older = || refused("It was made by an older version of reseek".to_string());
  let run: MaintenanceRun = serde_json::from_value(data.ok_or_else(older)?).map_err(|e| {
    debug!(error = %e, "maintenance job's run not read");
    older()
  })?;
  let app = app.clone();
  match run {
    MaintenanceRun::Cleanup => {
      tokio::spawn(async move { cleanup(&app, JobStarter::you(), true).await });
    }
    MaintenanceRun::Vacuum { database: None } => {
      tokio::spawn(async move { vacuum(&app, JobStarter::you(), true).await });
    }
    MaintenanceRun::Vacuum {
      database: Some(name),
    } => {
      if !is_database(&app, &name) {
        return Err(refused(format!("The database {name}.db no longer exists")));
      }
      tokio::spawn(async move {
        // the database is known, so the vacuum makes a job, and its end says how it went
        let _ = vacuum_one(&app, &name).await;
      });
    }
  }
  Ok(())
}

impl MaintenanceRun {
  /// The run as the job keeps it.
  fn data(&self) -> serde_json::Value {
    serde_json::to_value(self).expect("a maintenance run is written as a JSON object")
  }
}

#[cfg(test)]
mod tests {
  use api_types::jobs::{JobPartState, JobStarterKind};
  use api_types::transfers::TransferStates;

  use super::*;
  use crate::testing::{TRUSTED, app};
  use crate::transfers::{Record, store};

  #[tokio::test]
  async fn a_scheduled_cleanup_is_listed_only_when_it_removed_something_and_run_now_always_is() {
    // the cleanup the schedule would run at the start is off, so only the test's runs remove
    let config = format!(
      "[schedules]\ncleanup = \"off\"\n[retention.transfers.download]\nsucceeded = \"5m\"\n{TRUSTED}"
    );
    let app = app(&config, &[]).await;
    let state = &app.state;
    let scheduled = || JobStarter::schedule(crate::jobs::schedules::Schedule::Cleanup.name());

    let quiet = cleanup(state, scheduled(), false).await;
    assert_eq!(quiet.ending, JobEnding::Succeeded);
    assert_eq!(quiet.job_id, None, "nothing removed, nothing listed");
    assert_eq!(quiet.summary.as_deref(), Some("Nothing to remove"));

    let mut old = Record::download("alice", "@@a\\old.flac", 1);
    old.state = TransferStates::Completed | TransferStates::Succeeded;
    old.ended_at = Some(Utc::now() - chrono::Duration::hours(1));
    state
      .transfers
      .write(move |conn| store::insert(conn, &old))
      .await
      .unwrap();
    let busy = cleanup(state, scheduled(), false).await;
    let id = busy
      .job_id
      .expect("a cleanup that removed something is listed");
    assert_eq!(busy.summary.as_deref(), Some("Removed 1 finished transfer"));
    let detail = state.jobs.detail(id).await.unwrap().unwrap();
    assert_eq!(detail.job.starter.kind, JobStarterKind::Schedule);
    assert_eq!(detail.job.ending, Some(JobEnding::Succeeded));
    assert_eq!(detail.parts.len(), CLEANUP.len());
    assert!(
      detail
        .parts
        .iter()
        .all(|p| p.state == JobPartState::Succeeded)
    );
    assert_eq!(detail.parts[0].name, "Finished transfers");

    let asked = cleanup(state, JobStarter::you(), true).await;
    assert!(
      asked.job_id.is_some(),
      "a run the user asked for is always listed"
    );
  }

  #[test]
  fn a_maintenance_run_is_kept_in_the_shape_jobs_already_hold() {
    let vacuum = |database: Option<&str>| MaintenanceRun::Vacuum {
      database: database.map(str::to_string),
    };
    for (run, kept) in [
      (
        MaintenanceRun::Cleanup,
        serde_json::json!({ "run": "cleanup" }),
      ),
      (vacuum(None), serde_json::json!({ "run": "vacuum" })),
      (
        vacuum(Some("transfers")),
        serde_json::json!({ "run": "vacuum", "database": "transfers" }),
      ),
    ] {
      assert_eq!(run.data(), kept);
      assert_eq!(serde_json::from_value::<MaintenanceRun>(kept).unwrap(), run);
    }
  }

  #[tokio::test]
  async fn a_vacuum_of_a_database_that_is_gone_is_refused_rather_than_dropped() {
    let app = app(TRUSTED, &[]).await;
    let gone = serde_json::json!({ "run": "vacuum", "database": "old" });
    let refused = run_again(&app.state, Some(gone)).unwrap_err();
    assert_eq!(refused.status(), axum::http::StatusCode::CONFLICT);
    let name = app.state.databases.names().remove(0);
    let known = serde_json::json!({ "run": "vacuum", "database": name });
    run_again(&app.state, Some(known)).unwrap();
  }
}
