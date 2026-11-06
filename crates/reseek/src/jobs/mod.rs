//! Jobs: the longer work reseek does, of four kinds. A download job is everything queued in one
//! go; a share scan; an analysis of one or more files; a maintenance run, the cleanup or a
//! database vacuum. Each kind runs in its own engine, which tells this one as its job is made,
//! starts, changes and ends; this one keeps every job in `jobs.db`, pushes it to the `jobs`
//! topic, keeps the counts in the state and raises `JobFinished` as a job ends. Ended jobs stay
//! in the list until the cleanup deletes them past `retention.jobs`.
//!
//! A job is queued, running or paused, then ended as succeeded, finished with failures, failed
//! or canceled. Download and analysis jobs carry on after a restart; a scan or maintenance run
//! a restart cut off ends failed.

pub mod maintenance;
pub mod scans;
pub mod schedules;
pub mod store;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use api_types::events::{Event, JobFinished};
use api_types::jobs::{
  Job, JobCounts, JobDetail, JobEnding, JobKind, JobListQuery, JobStarter, JobState, JobsRemoved,
};
use api_types::live::LiveFrame;
use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::{Database, DbError, Location};
use crate::events::EventBus;
use crate::live::Live;
use crate::state::StateOwner;
use crate::sync::lock;
pub use store::{NewPart, SCHEMA, StoredPart};

/// Why a scan or maintenance run a restart cut off failed.
pub const STOPPED: &str = "reseek stopped";

/// Now, to the microsecond the database keeps, so a job read back equals the one written.
pub fn now() -> DateTime<Utc> {
  let now = Utc::now();
  DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap_or(now)
}

/// `n` with the word for one or many of what it counts, such as `1 file` or `12 files`.
pub fn counted(n: impl Into<u64>, one: &str, many: &str) -> String {
  let n = n.into();
  format!("{n} {}", if n == 1 { one } else { many })
}

/// A job to make.
#[derive(Clone, Debug)]
pub struct NewJob {
  /// its id; a download job's is its downloads' job id
  pub id: Uuid,
  /// what it does
  pub kind: JobKind,
  /// what it works on
  pub name: String,
  /// who or what started it
  pub starter: JobStarter,
  /// queued or running
  pub state: JobState,
  /// the download job an analysis job analyzes the files of
  pub follows: Option<Uuid>,
  /// what a run again repeats, as the kind reads it
  pub data: Option<serde_json::Value>,
}

impl NewJob {
  /// A job of `kind` named `name`, started by `starter`, running from now.
  pub fn running(kind: JobKind, name: impl Into<String>, starter: JobStarter) -> Self {
    Self {
      id: Uuid::new_v4(),
      kind,
      name: name.into(),
      starter,
      state: JobState::Running,
      follows: None,
      data: None,
    }
  }

  /// The job as it is when made at `now`.
  fn job(&self, now: DateTime<Utc>) -> Job {
    Job {
      id: self.id,
      kind: self.kind,
      name: self.name.clone(),
      starter: self.starter.clone(),
      state: self.state,
      ending: None,
      error: None,
      summary: None,
      counts: JobCounts::default(),
      progress: None,
      follows: self.follows,
      pausable: store::pausable(self.kind, self.state),
      created_at: now,
      started_at: (self.state == JobState::Running).then_some(now),
      ended_at: None,
    }
  }
}

/// How a job ended.
#[derive(Clone, Debug, Default)]
pub struct Ended {
  /// how; worked out from the counts when not given
  pub ending: Option<JobEnding>,
  /// why the job itself failed
  pub error: Option<String>,
  /// what it came to in words
  pub summary: Option<String>,
  /// its parts as they ended, kept in its history; none keeps the parts it has
  pub parts: Option<Vec<NewPart>>,
  /// its counts; none counts its parts
  pub counts: Option<JobCounts>,
}

/// What a scheduled run came to, for its schedule's card.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOutcome {
  /// how it ended
  pub ending: JobEnding,
  /// why it failed
  pub error: Option<String>,
  /// what it came to in words
  pub summary: Option<String>,
  /// its job, when it is listed
  pub job_id: Option<Uuid>,
}

/// The jobs engine.
pub struct Jobs {
  db: Database,
  live: Live,
  events: EventBus,
  state: StateOwner,
  /// how far each running job without parts is, from 0 to 1
  progress: Mutex<HashMap<Uuid, f64>>,
  /// the cancel of each maintenance run going on
  cancels: Mutex<HashMap<Uuid, CancellationToken>>,
  /// woken when a schedule's last run or what it shows changed, for the schedules to be pushed
  pub schedules_changed: tokio::sync::Notify,
  /// the schedules with a run going on
  pub running_schedules: Mutex<std::collections::HashSet<&'static str>>,
  /// when the scheduler started, which a schedule that never ran counts from
  pub scheduler_started: DateTime<Utc>,
}

impl Jobs {
  /// Opens `jobs.db` at `location`. A scan or maintenance run the last stop cut off ends failed,
  /// and the parts of analysis jobs that were running wait again.
  pub async fn open(
    location: &Location,
    live: Live,
    events: EventBus,
    state: StateOwner,
  ) -> Result<Arc<Self>, DbError> {
    let db = Database::open(&SCHEMA, location).await?;
    let jobs = Arc::new(Self {
      db,
      live,
      events,
      state,
      progress: Mutex::default(),
      cancels: Mutex::default(),
      schedules_changed: tokio::sync::Notify::new(),
      running_schedules: Mutex::default(),
      scheduler_started: Utc::now(),
    });
    jobs.end_interrupted().await?;
    jobs.summarize().await;
    Ok(jobs)
  }

  /// The database, for listing and closing.
  pub fn database(&self) -> &Database {
    &self.db
  }

  /// Ends the scans and maintenance runs the last stop cut off as failed, and puts the parts
  /// of analysis jobs that were running back to waiting, so the file starts over.
  async fn end_interrupted(&self) -> Result<(), DbError> {
    let now = now();
    let ended = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let mut ended = Vec::new();
        for mut job in store::unended(&tx, None)? {
          match job.kind {
            JobKind::Scan | JobKind::Maintenance => {
              job.state = JobState::Ended;
              job.ending = Some(JobEnding::Failed);
              job.error = Some(STOPPED.to_string());
              job.ended_at = Some(now);
              job.pausable = false;
              store::update(&tx, &job)?;
              ended.push(job);
            }
            JobKind::Analysis => {
              store::set_parts(
                &tx,
                job.id,
                api_types::jobs::JobPartState::Running,
                api_types::jobs::JobPartState::Waiting,
              )?;
            }
            JobKind::Download => {}
          }
        }
        tx.commit()?;
        Ok(ended)
      })
      .await?;
    for job in &ended {
      info!(job = %job.id, kind = ?job.kind, name = job.name, "job cut off by the last stop ended as failed");
    }
    Ok(())
  }

  /// Makes the job `new` and pushes it.
  pub async fn create(&self, new: NewJob) -> Result<Job, DbError> {
    let job = new.job(now());
    let written = job.clone();
    let data = new.data;
    self
      .db
      .write(move |conn| store::insert(conn, &written, data.as_ref()))
      .await?;
    debug!(job = %job.id, kind = ?job.kind, name = job.name, starter = ?job.starter.kind, "job made");
    self.pushed(&job).await;
    Ok(job)
  }

  /// Makes `new` with `parts`, in one write, and pushes it; returns it with its parts' ids.
  pub async fn create_with_parts(
    &self,
    new: NewJob,
    parts: Vec<NewPart>,
  ) -> Result<(Job, Vec<Uuid>), DbError> {
    let mut job = new.job(now());
    job.counts.total = u32::try_from(parts.len()).unwrap_or(u32::MAX);
    let written = job.clone();
    let data = new.data;
    let ids = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        store::insert(&tx, &written, data.as_ref())?;
        let ids = store::add_parts(&tx, written.id, &parts)?;
        tx.commit()?;
        Ok(ids)
      })
      .await?;
    debug!(job = %job.id, kind = ?job.kind, name = job.name, parts = job.counts.total, "job made");
    self.pushed(&job).await;
    Ok((job, ids))
  }

  /// The job `id` as `change` leaves it, written and pushed when it changed; none when there
  /// is no such job. An ended job is not changed.
  pub async fn update(
    &self,
    id: Uuid,
    change: impl FnOnce(&mut Job) + Send + 'static,
  ) -> Result<Option<Job>, DbError> {
    let changed = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let Some(mut job) = store::get(&tx, id)? else {
          return Ok(None);
        };
        if job.state == JobState::Ended {
          return Ok(None);
        }
        let before = job.clone();
        change(&mut job);
        if job.state == JobState::Running && job.started_at.is_none() {
          job.started_at = Some(now());
        }
        job.pausable = store::pausable(job.kind, job.state);
        if job == before {
          return Ok(None);
        }
        store::update(&tx, &job)?;
        tx.commit()?;
        Ok(Some(job))
      })
      .await?;
    if let Some(job) = &changed {
      self.pushed(job).await;
    }
    Ok(changed)
  }

  /// Sets the counts of the job `id` and its state, which is one of queued, running or
  /// paused.
  pub async fn set_counts(
    &self,
    id: Uuid,
    counts: JobCounts,
    state: JobState,
  ) -> Result<Option<Job>, DbError> {
    self
      .update(id, move |job| {
        job.counts = counts;
        job.state = state;
      })
      .await
  }

  /// Sets how far the running job `id` is, pushing it when the percent changed.
  pub async fn progress(&self, id: Uuid, fraction: f64) {
    let fraction = fraction.clamp(0.0, 1.0);
    let changed = {
      let mut progress = lock(&self.progress);
      let before = progress.insert(id, fraction);
      before.is_none_or(|before| (before * 100.0).round() != (fraction * 100.0).round())
    };
    if !changed {
      return;
    }
    match self.db.read(move |conn| store::get(conn, id)).await {
      Ok(Some(job)) if job.state != JobState::Ended => self.push(job),
      Ok(_) => {}
      Err(e) => debug!(job = %id, error = %e, "job progress not pushed"),
    }
  }

  /// Ends the job `id` as `ended` says, keeping its parts in its history, raising `JobFinished`
  /// and pushing it; none when there is no such job or it has ended.
  pub async fn end(&self, id: Uuid, ended: Ended) -> Result<Option<Job>, DbError> {
    let now = now();
    let finished = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let Some(mut job) = store::get(&tx, id)? else {
          return Ok(None);
        };
        if job.state == JobState::Ended {
          return Ok(None);
        }
        if let Some(parts) = &ended.parts {
          store::replace_parts(&tx, id, parts)?;
        }
        job.counts = match ended.counts {
          Some(counts) => counts,
          None if ended.parts.is_some() => store::counts(&tx, id)?,
          None => job.counts,
        };
        job.counts.paused = 0;
        job.state = JobState::Ended;
        job.ending = Some(ended.ending.unwrap_or_else(|| job.counts.ending()));
        job.error = ended.error;
        job.summary = ended.summary.or(job.summary);
        job.started_at = job.started_at.or(Some(now));
        job.ended_at = Some(now);
        job.pausable = false;
        store::update(&tx, &job)?;
        tx.commit()?;
        Ok(Some(job))
      })
      .await?;
    if let Some(job) = &finished {
      self.finished(job).await;
    }
    Ok(finished)
  }

  /// Writes `job`, which ended as a run that is kept, with its `parts` and what a run again
  /// repeats, and tells of its end.
  pub async fn record(
    &self,
    job: Job,
    parts: Vec<NewPart>,
    data: Option<serde_json::Value>,
  ) -> Result<(), DbError> {
    let written = job.clone();
    self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        store::insert(&tx, &written, data.as_ref())?;
        store::add_parts(&tx, written.id, &parts)?;
        tx.commit()
      })
      .await?;
    self.finished(&job).await;
    Ok(())
  }

  /// Tells of `job`, which just ended: logged, pushed, raised as `JobFinished`.
  async fn finished(&self, job: &Job) {
    lock(&self.progress).remove(&job.id);
    lock(&self.cancels).remove(&job.id);
    let ending = job.ending.unwrap_or(JobEnding::Succeeded);
    info!(
      job = %job.id,
      kind = ?job.kind,
      name = job.name,
      ending = ?ending,
      error = job.error,
      parts = job.counts.total,
      failed = job.counts.failed,
      "job ended"
    );
    self.pushed(job).await;
    self.events.raise(Event::JobFinished(JobFinished {
      version: 0,
      id: job.id,
      kind: job.kind,
      name: job.name.clone(),
      starter: job.starter.clone(),
      ending,
      error: job.error.clone(),
      summary: job.summary.clone(),
      counts: job.counts,
    }));
  }

  /// The job `id`, none when there is none.
  pub async fn get(&self, id: Uuid) -> Result<Option<Job>, DbError> {
    let job = self.db.read(move |conn| store::get(conn, id)).await?;
    Ok(job.map(|job| self.with_progress(job)))
  }

  /// What a run again of the job `id` repeats.
  pub async fn data(&self, id: Uuid) -> Result<Option<serde_json::Value>, DbError> {
    self.db.read(move |conn| store::data(conn, id)).await
  }

  /// The job `id` with its stored parts.
  pub async fn detail(&self, id: Uuid) -> Result<Option<JobDetail>, DbError> {
    let found = self
      .db
      .read(move |conn| {
        let Some(job) = store::get(conn, id)? else {
          return Ok(None);
        };
        let parts = store::parts(conn, id)?
          .into_iter()
          .map(|stored| stored.part)
          .collect();
        Ok(Some((job, parts)))
      })
      .await?;
    Ok(found.map(|(job, parts)| JobDetail {
      job: self.with_progress(job),
      parts,
    }))
  }

  /// The jobs `query` asks for: running and paused first, then queued, then the ended ones
  /// newest first.
  pub async fn list(&self, query: JobListQuery) -> Result<Vec<Job>, DbError> {
    let jobs = self.db.read(store::list).await?;
    let text = query.q.as_deref().map(str::to_lowercase);
    Ok(
      jobs
        .into_iter()
        .filter(|job| query.kind.is_none_or(|kind| job.kind == kind))
        .filter(|job| query.state.is_none_or(|state| job.state == state))
        .filter(|job| query.starter.is_none_or(|kind| job.starter.kind == kind))
        .filter(|job| {
          text
            .as_deref()
            .is_none_or(|text| job.name.to_lowercase().contains(text))
        })
        .map(|job| self.with_progress(job))
        .collect(),
    )
  }

  /// The jobs not ended of `kind`.
  pub async fn unended(&self, kind: JobKind) -> Result<Vec<Job>, DbError> {
    self
      .db
      .read(move |conn| store::unended(conn, Some(kind)))
      .await
  }

  /// Takes the ended jobs `ids` out of the list; returns those that were there.
  pub async fn remove(&self, ids: Vec<Uuid>) -> Result<Vec<Uuid>, DbError> {
    let removed = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let mut ended = Vec::new();
        for id in ids {
          if store::get(&tx, id)?.is_some_and(|job| job.state == JobState::Ended) {
            ended.push(id);
          }
        }
        let removed = store::remove(&tx, &ended)?;
        tx.commit()?;
        Ok(removed)
      })
      .await?;
    self.removed(&removed).await;
    Ok(removed)
  }

  /// Takes every ended job out of the list; returns how many.
  pub async fn clear_ended(&self) -> Result<usize, DbError> {
    let removed = self
      .db
      .write(|conn| {
        let tx = conn.transaction()?;
        let ended = store::ended(&tx)?;
        let removed = store::remove(&tx, &ended)?;
        tx.commit()?;
        Ok(removed)
      })
      .await?;
    self.removed(&removed).await;
    Ok(removed.len())
  }

  /// Deletes the ended jobs past `age` or past the newest `count`, each `None` for no limit;
  /// returns how many.
  pub async fn prune(
    &self,
    age: Option<chrono::Duration>,
    count: Option<u32>,
  ) -> Result<usize, DbError> {
    let before = age.map(|age| crate::db::shifted(Utc::now(), -age));
    let removed = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let expired = store::expired(&tx, before, count)?;
        let removed = store::remove(&tx, &expired)?;
        tx.commit()?;
        Ok(removed)
      })
      .await?;
    self.removed(&removed).await;
    Ok(removed.len())
  }

  /// Pushes that `ids` left the list.
  async fn removed(&self, ids: &[Uuid]) {
    if ids.is_empty() {
      return;
    }
    self
      .live
      .publish(LiveFrame::JobsRemoved(JobsRemoved { ids: ids.to_vec() }));
    self.summarize().await;
  }

  /// The last run of every schedule that ran, by name.
  pub async fn last_runs(&self) -> Result<HashMap<String, store::LastRun>, DbError> {
    Ok(self.db.read(store::last_runs).await?.into_iter().collect())
  }

  /// Changes the last run of the schedule `name` as `change` says, in one write, and has the
  /// schedules pushed again.
  pub async fn update_last_run(
    &self,
    name: &str,
    change: impl FnOnce(&mut store::LastRun) + Send + 'static,
  ) {
    let schedule = name.to_string();
    let written = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let mut last = store::last_run(&tx, &schedule)?.unwrap_or_default();
        change(&mut last);
        store::set_last_run(&tx, &schedule, &last)?;
        tx.commit()
      })
      .await;
    if let Err(e) = written {
      warn!(schedule = name, error = %e, "the last run of a schedule not written");
    }
    self.schedules_changed.notify_one();
  }

  /// Keeps the cancel of the run of `id`, which a cancel of the job uses.
  pub fn hold_cancel(&self, id: Uuid, cancel: CancellationToken) {
    lock(&self.cancels).insert(id, cancel);
  }

  /// Cancels the run of `id` held with [`Jobs::hold_cancel`]; false when none is held.
  pub fn cancel_held(&self, id: Uuid) -> bool {
    match lock(&self.cancels).get(&id) {
      Some(cancel) => {
        cancel.cancel();
        true
      }
      None => false,
    }
  }

  /// `job` with how far it is.
  fn with_progress(&self, mut job: Job) -> Job {
    if job.state != JobState::Ended {
      job.progress = lock(&self.progress).get(&job.id).copied();
    }
    job
  }

  /// Pushes `job` and brings the counts in the state up to date.
  async fn pushed(&self, job: &Job) {
    self.push(job.clone());
    self.summarize().await;
  }

  /// Pushes `job` as it is now.
  fn push(&self, job: Job) {
    let job = self.with_progress(job);
    self.live.publish(LiveFrame::JobsUpdated(Box::new(job)));
  }

  /// Puts how many jobs run and are paused, and the last failure, into the state.
  async fn summarize(&self) {
    match self.db.read(store::summary).await {
      Ok(summary) => {
        if self.state.current().jobs != summary {
          self.state.update(|state| state.jobs = summary);
        }
      }
      Err(e) => warn!(error = %e, "job counts not read"),
    }
  }
}
