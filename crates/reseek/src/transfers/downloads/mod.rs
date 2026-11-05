//! The download engine: enqueueing files from a peer, one task per download that tries it,
//! waits and tries again, writing it into the incomplete directory and moving it into the
//! downloads directory once it has arrived, and what the API does to downloads besides:
//! canceling, removing, the place in the peer's queue, and resuming the downloads a stop
//! interrupted.
//!
//! [`Downloads`] keeps every running download's cancel handle, keyed by its id, from the
//! moment its record is written until its task has written how it ended. Enqueues for one user
//! run one at a time. Every record written is pushed to the `transfers` topic.

mod attempt;
mod batches;
mod complete;
mod destination;
mod enqueue;
mod jobs;
mod queue;
mod service;
mod task;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::transfers::{TransferDirection, TransferStates};
use cascade::Watcher;
use chrono::Utc;
use rusqlite::params;
use tokio::sync::Semaphore;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::categories::UNFINISHED;
use super::{Record, cleanup, direction_text, removal, store};
use crate::analysis::Analysis;
use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::{Database, DbError};
use crate::events::EventBus;
use crate::files::Files;
use crate::live::Live;
use crate::network::Network;
use crate::paths::Paths;
use crate::relay::Controller;
use crate::searches::Searches;
use crate::sync::lock;
use crate::telemetry::Telemetry;
use crate::turns::{Turn, Turns};
use crate::users::Directory;
pub use enqueue::{Batching, Enqueued, FileRequest, Group, NewBatch};

/// How long the tasks of running downloads get to end at shutdown before their records are
/// ended anyway.
const STOPPING: Duration = Duration::from_secs(5);

/// How many enqueue calls of the API run at once.
const ENQUEUE_CALLS: usize = 2;

/// Why a download canceled at block time ended.
const BLOCKED: &str = "Blocked";

/// Why a download canceled without a running task ended.
const CANCELED: &str = "The operation was canceled.";

/// What is kept for a download whose task runs.
struct InFlight {
  /// stops the download wherever it is
  cancel: CancellationToken,
  /// remove the record from the listings once it has ended
  remove: bool,
  /// why it was canceled, when not by a plain cancel
  reason: Option<&'static str>,
}

/// The download engine.
pub struct Downloads {
  this: Weak<Downloads>,
  db: Database,
  config: Watcher<Options>,
  network: Arc<Network>,
  users: Arc<Directory>,
  files: Arc<Files>,
  searches: Arc<Searches>,
  events: EventBus,
  live: Live,
  telemetry: Arc<Telemetry>,
  /// the relay controller, told of every finished download, when relay mode is on
  relay: Option<Arc<Controller>>,
  /// told of every finished download, which it analyzes when the options ask for it
  analysis: Arc<Analysis>,
  incomplete: PathBuf,
  downloads: PathBuf,
  shutdown: CancellationToken,
  /// every download task, so the stop waits for them
  tasks: TaskTracker,
  in_flight: Mutex<HashMap<Uuid, InFlight>>,
  /// one enqueue per user at a time
  turns: Turns,
  /// the jobs whose files are still being enqueued
  open_jobs: jobs::Open,
  /// the enqueue calls of the API that may run at once
  enqueue_calls: Semaphore,
  /// whether the downloads a stop interrupted were resumed, which the first login does
  resumed: AtomicBool,
  /// when each running download last asked its peer for its place in the queue
  asked: Mutex<HashMap<Uuid, Instant>>,
}

impl Downloads {
  /// The engine over `transfers.db`, reaching peers through `network`, asking `users` who is
  /// blocked, marking the files it writes in `files`, reading what a batch searched for from
  /// `searches`, raising its events on `events`, pushing its records to `live`, counting the
  /// downloads that end in `telemetry` and telling `relay` and `analysis` of each that
  /// finished. Its tasks end when `shutdown` is canceled.
  #[allow(
    clippy::too_many_arguments,
    reason = "the engine takes each part of the daemon it uses"
  )]
  pub fn new(
    db: Database,
    config: Watcher<Options>,
    network: Arc<Network>,
    users: Arc<Directory>,
    files: Arc<Files>,
    searches: Arc<Searches>,
    events: EventBus,
    live: Live,
    telemetry: Arc<Telemetry>,
    relay: Option<Arc<Controller>>,
    analysis: Arc<Analysis>,
    paths: &Paths,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db,
      config,
      network,
      users,
      files,
      searches,
      events,
      live,
      telemetry,
      relay,
      analysis,
      incomplete: paths.incomplete.clone(),
      downloads: paths.downloads.clone(),
      shutdown,
      tasks: TaskTracker::new(),
      in_flight: Mutex::new(HashMap::new()),
      turns: Turns::default(),
      open_jobs: jobs::Open::default(),
      enqueue_calls: Semaphore::new(ENQUEUE_CALLS),
      resumed: AtomicBool::new(false),
      asked: Mutex::new(HashMap::new()),
    })
  }

  /// This engine, for its tasks to hold.
  fn this(&self) -> Arc<Self> {
    self
      .this
      .upgrade()
      .expect("the engine is alive while a method runs on it")
  }

  /// Waits for `username`'s turn to enqueue.
  async fn turn(&self, username: &str) -> Turn {
    self.turns.take(username).await
  }

  /// Writes `record` whole, adds its state to its history when it `entered` it, and pushes it:
  /// as updated, or, for a record removed, as removed once, with the state it ended in. A write
  /// that ends the last download of its job tells of the job after. A failed write is logged;
  /// the download goes on.
  async fn save(&self, record: &Record, entered: bool) {
    let written = record.clone();
    let open = self.open_jobs.clone();
    let result = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let ending = written.job_id.is_some() && written.state.contains(TransferStates::Completed);
        let unfinished = ending && jobs::unfinished(&tx, written.id)?;
        let found = store::save(&tx, &written)?;
        if found && entered {
          store::entered(&tx, written.id, written.state, Utc::now())?;
        }
        let finished = if found {
          jobs::ended_by(&tx, &written, unfinished, &open)?
        } else {
          None
        };
        tx.commit()?;
        Ok((found, finished))
      })
      .await;
    match result {
      Ok((true, finished)) => {
        if !record.removed || entered {
          self.push(record);
        }
        self.tell(finished);
      }
      Ok((false, _)) => warn!(id = %record.id, "download record gone; not written"),
      Err(e) => warn!(id = %record.id, error = %e, "download record not written"),
    }
  }

  /// Pushes `record` to the event stream: as removed when it is, else as updated.
  fn push(&self, record: &Record) {
    if record.removed {
      removal::push(
        &self.live,
        TransferDirection::Download,
        &[(record.id, record.username.clone())],
      );
    } else {
      self
        .live
        .publish(LiveFrame::TransfersUpdated(Box::new(record.transfer())));
    }
  }

  /// The download `id` of `username`, removed or not; 404 when there is none.
  async fn record_of(&self, username: &str, id: Uuid) -> Result<Record, ApiError> {
    super::listing::record(&self.db, TransferDirection::Download, username, id)
      .await
      .map_err(ApiError::internal)?
      .ok_or_else(|| ApiError::not_found(format!("No download {id} from {username}")))
  }

  /// Cancels the download `id` of `username`: a running one stops wherever it is and ends
  /// canceled; one whose task is gone without ending it is ended canceled here. With
  /// `remove` it leaves the listings once it has ended. 404 when there is no such download.
  pub async fn cancel(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    let mut record = self.record_of(username, id).await?;
    if self.cancel_running(id, remove, None) {
      return Ok(());
    }
    // a record no task runs any more, left behind by a failure, is ended canceled here; a task
    // that ended it since it was read keeps how it ended. The removal is in the same write, so
    // a removed download is left out of its job when that write ends the job
    let canceled = TransferStates::Completed | TransferStates::Cancelled;
    let at = Utc::now();
    let unfinished = !record.is_completed();
    if !unfinished && !remove {
      return Ok(());
    }
    record.state = canceled;
    record.ended_at = Some(at);
    record.exception = Some(CANCELED.to_string());
    record.next_attempt_at = None;
    record.place_in_queue = None;
    let written = record.clone();
    let open = self.open_jobs.clone();
    let (ended, removed, finished) = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let ended = unfinished && store::end_unless_ended(&tx, id, canceled, at, CANCELED)?;
        if ended {
          store::entered(&tx, id, canceled, at)?;
        }
        let removed = if remove {
          removal::remove_one(&tx, TransferDirection::Download, id)?
        } else {
          None
        };
        let finished = if ended {
          jobs::ended_by(&tx, &written, true, &open)?
        } else {
          None
        };
        tx.commit()?;
        Ok((ended, removed, finished))
      })
      .await
      .map_err(ApiError::internal)?;
    if ended {
      info!(%id, username, "download without a running task ended as canceled");
      self.push(&record);
    }
    if let Some(peer) = removed {
      removal::push(&self.live, TransferDirection::Download, &[(id, peer)]);
    }
    self.tell(finished);
    Ok(())
  }

  /// Stops the running download `id`, marking it for removal with `remove` and noting
  /// `reason`; false when it is not running.
  fn cancel_running(&self, id: Uuid, remove: bool, reason: Option<&'static str>) -> bool {
    let mut in_flight = lock(&self.in_flight);
    let Some(entry) = in_flight.get_mut(&id) else {
      return false;
    };
    entry.remove |= remove;
    entry.reason = entry.reason.or(reason);
    entry.cancel.cancel();
    debug!(%id, remove, "running download canceled");
    true
  }

  /// Cancels every unfinished download from `username`, who was just blocked, when `scopes`
  /// include downloads.
  pub async fn blocked(&self, username: &str, scopes: &[api_types::users::blocking::BlockScope]) {
    if !scopes.contains(&api_types::users::blocking::BlockScope::Downloads) {
      return;
    }
    let name = username.to_string();
    let unfinished = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(&format!(
          "SELECT Id FROM Transfers WHERE Username = ?1 AND Direction = ?2 AND {UNFINISHED}"
        ))?;
        let ids = statement
          .query_map(
            params![name, direction_text(TransferDirection::Download)],
            |row| row.get::<_, String>(0),
          )?
          .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
      })
      .await;
    let ids = match unfinished {
      Ok(ids) => ids,
      Err(e) => return warn!(username, error = %e, "downloads from a blocked user not canceled"),
    };
    let mut canceled = 0;
    for id in self::ids(ids) {
      if self.cancel_running(id, false, Some(BLOCKED)) {
        canceled += 1;
      }
    }
    info!(username, canceled, "downloads from a blocked user canceled");
  }

  /// Enqueues again the downloads a stop interrupted that were not removed since, each user's
  /// together and each in its batch and job, as new downloads that supersede the old records,
  /// so a job goes on and is told of once all of it has ended. A download from before jobs were
  /// kept goes into a new job with the others of its user and batch. Only the first call after a
  /// start does anything, which is after the first login; the marks are cleared before the
  /// enqueues, so a user that cannot be reached is logged, not tried again.
  pub async fn resume(&self) {
    if self.resumed.swap(true, Ordering::AcqRel) {
      return;
    }
    let resumable = match cleanup::take_resumable(&self.db).await {
      Ok(resumable) => resumable,
      Err(e) => return warn!(error = %e, "interrupted downloads not resumed"),
    };
    let mut jobs: BTreeMap<Uuid, Vec<Group>> = BTreeMap::new();
    for ((username, batch, job), files) in resumable {
      jobs
        .entry(job.unwrap_or_else(Uuid::new_v4))
        .or_default()
        .push(Group {
          username,
          files,
          batching: batch.map_or(Batching::Single, Batching::Existing),
        });
    }
    for (job, groups) in jobs {
      let asked: Vec<(String, usize)> = groups
        .iter()
        .map(|g| (g.username.clone(), g.files.len()))
        .collect();
      let results = self.enqueue(job, groups).await;
      for ((username, count), result) in asked.into_iter().zip(results) {
        match result {
          Ok(enqueued) => info!(
            username,
            %job,
            resumed = enqueued.enqueued.len(),
            failed = enqueued.failed.len(),
            "interrupted downloads resumed"
          ),
          Err(e) => warn!(
            username,
            %job,
            downloads = count,
            error = e.message(),
            "interrupted downloads not resumed; the user is likely offline"
          ),
        }
      }
    }
  }

  /// Waits, at shutdown and once the client is stopped, for the download tasks the shutdown
  /// canceled to end; what they leave unfinished the stop ends after.
  pub async fn stop(&self) {
    self.tasks.close();
    if tokio::time::timeout(STOPPING, self.tasks.wait())
      .await
      .is_err()
    {
      warn!(
        running = self.tasks.len(),
        "download tasks still running at shutdown"
      );
    }
  }
}

/// Reads the ids of `rows`, logging the ones that are not ids.
fn ids(rows: Vec<String>) -> Vec<Uuid> {
  rows
    .into_iter()
    .filter_map(|text| {
      text
        .parse()
        .inspect_err(|e| warn!(id = text, error = %e, "transfer with a malformed id skipped"))
        .ok()
    })
    .collect()
}

/// A database failure as the API answers it.
fn internal(error: DbError) -> ApiError {
  ApiError::internal(error)
}
