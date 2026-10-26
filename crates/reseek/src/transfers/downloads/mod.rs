//! The download engine: enqueueing files from a peer, one task per download that tries it,
//! waits and tries again, writing it into the incomplete directory and moving it into the
//! downloads directory once it has arrived, and what the API does to downloads besides:
//! cancelling, removing, the place in the peer's queue, and resuming the downloads a stop
//! interrupted.
//!
//! [`Downloads`] keeps every running download's cancel handle, keyed by its id, from the
//! moment its record is written until its task has written how it ended. Enqueues for one user
//! run one at a time. Every record written is pushed to the `transfers` topic.

mod attempt;
pub mod batches;
mod complete;
pub mod destination;
mod enqueue;
mod queue;
mod service;
mod task;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::transfers::{TransferDirection, TransferStates};
use cascade::Watcher;
use chrono::Utc;
use rusqlite::params;
use tokio::sync::{OwnedMutexGuard, Semaphore};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Record, cleanup, direction_text, removal, store};
use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::{Database, DbError};
use crate::events::EventBus;
use crate::files::Files;
use crate::live::Live;
use crate::network::Network;
use crate::paths::Paths;
use crate::users::Directory;
pub use enqueue::{Batching, Enqueued, FileRequest, IN_PROGRESS, NewBatch};

/// How long the tasks of running downloads get to end at shutdown before their records are
/// ended anyway.
const STOPPING: Duration = Duration::from_secs(5);

/// How many enqueue calls of the API run at once.
const ENQUEUE_CALLS: usize = 2;

/// Why a download cancelled at block time ended.
const BLOCKED: &str = "Blocked";

/// What is kept for a download whose task runs.
struct InFlight {
  /// stops the download wherever it is
  cancel: CancellationToken,
  /// remove the record from the listings once it has ended
  remove: bool,
  /// why it was cancelled, when not by a plain cancel
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
  events: EventBus,
  live: Live,
  incomplete: PathBuf,
  downloads: PathBuf,
  shutdown: CancellationToken,
  /// every download task, so the stop waits for them
  tasks: TaskTracker,
  in_flight: Mutex<HashMap<Uuid, InFlight>>,
  /// one enqueue per user at a time
  turns: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
  /// the enqueue calls of the API that may run at once
  enqueue_calls: Semaphore,
  /// the downloads a stop interrupted, resumed after the first login
  resumable: Mutex<Option<Vec<Uuid>>>,
  /// when each running download last asked its peer for its place in the queue
  asked: Mutex<HashMap<Uuid, Instant>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  // every change under these locks is made whole before the lock is let go
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One user's turn to enqueue, held until dropped; the user's entry goes once nobody holds or
/// waits for it.
pub(crate) struct Turn {
  turns: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
  username: String,
  guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for Turn {
  fn drop(&mut self) {
    let mut turns = lock(&self.turns);
    let Some(mutex) = turns.get(&self.username) else {
      return;
    };
    // the map's and the guard's references are the only ones when nobody waits
    let idle = Arc::strong_count(mutex) == 2;
    self.guard.take();
    if idle {
      turns.remove(&self.username);
    }
  }
}

impl Downloads {
  /// The engine over `transfers.db`, reaching peers through `network`, asking `users` who is
  /// blocked, marking the files it writes in `files`, raising its events on `events` and
  /// pushing its records to `live`. Its tasks end when `shutdown` is cancelled.
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
    events: EventBus,
    live: Live,
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
      events,
      live,
      incomplete: paths.incomplete.clone(),
      downloads: paths.downloads.clone(),
      shutdown,
      tasks: TaskTracker::new(),
      in_flight: Mutex::new(HashMap::new()),
      turns: Arc::default(),
      enqueue_calls: Semaphore::new(ENQUEUE_CALLS),
      resumable: Mutex::new(None),
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
    let mutex = lock(&self.turns)
      .entry(username.to_string())
      .or_default()
      .clone();
    let guard = mutex.lock_owned().await;
    Turn {
      turns: self.turns.clone(),
      username: username.to_string(),
      guard: Some(guard),
    }
  }

  /// Writes `record` whole, adds its state to its history when it `entered` it, and pushes it:
  /// as updated, or, for a record removed, as removed once, with the state it ended in. A failed
  /// write is logged; the download goes on.
  async fn save(&self, record: &Record, entered: bool) {
    let written = record.clone();
    let result = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let found = store::save(&tx, &written)?;
        if found && entered {
          store::entered(&tx, written.id, written.state, Utc::now())?;
        }
        tx.commit()?;
        Ok(found)
      })
      .await;
    match result {
      Ok(true) if record.removed && !entered => {}
      Ok(true) => self.push(record),
      Ok(false) => warn!(id = %record.id, "download record gone; not written"),
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
      .ok_or_else(|| ApiError::not_found(format!("no download {id} from {username}")))
  }

  /// Cancels the download `id` of `username`: a running one stops wherever it is and ends
  /// cancelled; one whose task is gone without ending it is ended cancelled here. With
  /// `remove` it leaves the listings once it has ended. 404 when there is no such download.
  pub async fn cancel(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    let mut record = self.record_of(username, id).await?;
    if self.cancel_running(id, remove, None) {
      return Ok(());
    }
    if !record.is_completed() {
      // a record no task runs any more, left behind by a failure
      record.state = TransferStates::Completed | TransferStates::Cancelled;
      record.ended_at = Some(Utc::now());
      record.exception = Some("The operation was canceled.".to_string());
      info!(%id, username, "download without a running task ended as cancelled");
      self.save(&record, true).await;
    }
    if remove {
      removal::remove(&self.db, &self.live, TransferDirection::Download, id)
        .await
        .map_err(ApiError::internal)?;
    }
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
    debug!(%id, remove, "running download cancelled");
    true
  }

  /// Cancels every unfinished download from `username`, who was just blocked, when `scopes`
  /// include downloads.
  pub async fn blocked(&self, username: &str, scopes: &[api_types::blocking::BlockScope]) {
    if !scopes.contains(&api_types::blocking::BlockScope::Downloads) {
      return;
    }
    let name = username.to_string();
    let unfinished = self
      .db
      .read(move |conn| {
        let mut statement = conn.prepare(
          "SELECT Id FROM Transfers WHERE Username = ?1 AND Direction = ?2 AND (State & 16) = 0",
        )?;
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
      Err(e) => return warn!(username, error = %e, "downloads from a blocked user not cancelled"),
    };
    let mut cancelled = 0;
    for id in self::ids(ids) {
      if self.cancel_running(id, false, Some(BLOCKED)) {
        cancelled += 1;
      }
    }
    info!(
      username,
      cancelled, "downloads from a blocked user cancelled"
    );
  }

  /// Takes the ids of the downloads a stop interrupted, to resume after the first login. Called
  /// at startup, after the records left unfinished are ended.
  pub async fn remember_resumable(&self) {
    match cleanup::resumable(&self.db).await {
      Ok(ids) => {
        if !ids.is_empty() {
          info!(
            downloads = ids.len(),
            "downloads interrupted at the last stop resume after login"
          );
        }
        *lock(&self.resumable) = Some(ids);
      }
      Err(e) => warn!(error = %e, "downloads interrupted at the last stop not found"),
    }
  }

  /// Enqueues again the downloads a stop interrupted that were not removed since, each user's
  /// together and each in its batch, as new downloads that supersede the old records. Runs
  /// once, after the first login; a user that cannot be reached is logged.
  pub async fn resume(&self) {
    let Some(ids) = lock(&self.resumable).take() else {
      return;
    };
    if ids.is_empty() {
      return;
    }
    let groups = match cleanup::resume_groups(&self.db, ids).await {
      Ok(groups) => groups,
      Err(e) => return warn!(error = %e, "interrupted downloads not resumed"),
    };
    for ((username, batch), files) in groups {
      let count = files.len();
      let batching = batch.map_or(Batching::Single, Batching::Existing);
      match self.enqueue(&username, files, batching).await {
        Ok(enqueued) => info!(
          username,
          resumed = enqueued.enqueued.len(),
          failed = enqueued.failed.len(),
          "interrupted downloads resumed"
        ),
        Err(e) => warn!(
          username,
          downloads = count,
          error = e.message(),
          "interrupted downloads not resumed; the user is likely offline"
        ),
      }
    }
  }

  /// Stops the engine at shutdown, once the client is stopped: the download tasks, which the
  /// shutdown cancelled, are waited for, and every transfer left unfinished is ended as shut
  /// down, so the next start resumes the downloads among them.
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
    match cleanup::end_unfinished(&self.db).await {
      Ok(ended) => debug!(ended, "unfinished transfers ended at shutdown"),
      Err(e) => warn!(error = %e, "unfinished transfers not ended at shutdown"),
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
