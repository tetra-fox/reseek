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
mod complete;
mod destination;
mod enqueue;
mod jobs;
mod pause;
mod queue;
mod service;
mod starts;
mod task;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::transfers::{TransferDirection, TransferStates};
use cascade::Watcher;
use chrono::Utc;
use reseek_shares::lock;
use rusqlite::params;
use tokio::sync::{Semaphore, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, info, warn};
use uuid::Uuid;

use super::categories::UNFINISHED;
use super::{Record, cleanup, direction_text, removal, store};
use crate::analysis::Analysis;
use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::{Database, DbError};
use crate::events::EventBus;
use crate::files::Files;
use crate::jobs::Jobs;
use crate::live::Live;
use crate::network::Network;
use crate::paths::Paths;
use crate::relay::Controller;
use crate::searches::Searches;
use crate::telemetry::Telemetry;
use crate::turns::{Turn, Turns};
use crate::users::Directory;
pub use destination::incomplete_path;
pub use enqueue::{Enqueued, FileRequest, Group};

/// How long the tasks of downloads still downloading get to end at shutdown before their
/// records are ended anyway. A task moving its arrived file is waited for however long it takes.
const STOPPING: Duration = Duration::from_secs(5);

/// How many enqueue calls of the API run at once.
const ENQUEUE_CALLS: usize = 2;

/// Why a download canceled at block time ended.
const BLOCKED: &str = "Blocked";

/// Why a download canceled without a running task ended.
const CANCELED: &str = "The operation was canceled.";

/// Why a pause or cancel of a download that finished, or whose file is being moved, is refused.
const COMPLETED: &str = "the download has completed";

/// What is kept for a download whose task runs.
struct InFlight {
  /// stops the download wherever it is
  cancel: CancellationToken,
  /// remove the record from the listings once it has ended
  remove: bool,
  /// why it was canceled, when not by a plain cancel
  reason: Option<&'static str>,
  /// canceled to be paused: it waits here again, keeping what arrived
  pause: bool,
  /// its file arrived and is handed to its move, which a pause or cancel no longer stops
  arrived: bool,
  /// dropped with the entry once the task has let go of the download, which ends every wait on
  /// a receiver of it
  ended: watch::Sender<()>,
}

impl InFlight {
  /// A download about to start its task, stopped by `cancel`.
  fn new(cancel: CancellationToken) -> Self {
    Self {
      cancel,
      remove: false,
      reason: None,
      pause: false,
      arrived: false,
      ended: watch::Sender::new(()),
    }
  }
}

/// What a cancel found of a download's task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Running {
  /// no task runs it
  No,
  /// its task was told to stop
  Stopped,
  /// its file arrived and is being moved, so it ends succeeded
  Arrived,
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
  /// the jobs list, which each download job is kept in
  jobs: Arc<Jobs>,
  incomplete: PathBuf,
  downloads: PathBuf,
  shutdown: CancellationToken,
  /// every download task, so the stop waits for them
  tasks: TaskTracker,
  /// the tasks moving an arrived file and letting go of its download, which the stop waits for
  /// without a limit
  moving: TaskTracker,
  /// set once the stop's limit has passed: a download that arrives after it leaves its file
  /// for the next start to take up, since the stop no longer waits for its move
  moves_closed: Mutex<bool>,
  in_flight: Mutex<HashMap<Uuid, InFlight>>,
  /// one enqueue per user at a time
  turns: Turns,
  /// the local queue: the order waiting downloads start in
  starts: starts::Starts,
  /// the jobs whose files are still being enqueued
  open_jobs: jobs::Open,
  /// the enqueue calls of the API that may run at once
  enqueue_calls: Semaphore,
  /// whether the downloads a stop interrupted were resumed, which the first login does
  resumed: AtomicBool,
  /// when each running download last asked its peer for its place in the queue
  asked: Mutex<HashMap<Uuid, Instant>>,
  /// held by a test to keep an arrived file's move from starting
  #[cfg(test)]
  move_gate: tokio::sync::Mutex<()>,
}

impl Downloads {
  /// The engine over `transfers.db`, reaching peers through `network`, asking `users` who is
  /// blocked, marking the files it writes in `files`, reading what a download's search was for
  /// from `searches`, raising its events on `events`, pushing its records to `live`, counting the
  /// downloads that end in `telemetry`, telling `relay` and `analysis` of each that finished
  /// and keeping each download job in `jobs`. Its tasks end when `shutdown` is canceled.
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
    jobs: Arc<Jobs>,
    paths: &Paths,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let engine = Arc::new_cyclic(|this| Self {
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
      jobs,
      incomplete: paths.incomplete.clone(),
      downloads: paths.downloads.clone(),
      shutdown,
      tasks: TaskTracker::new(),
      moving: TaskTracker::new(),
      moves_closed: Mutex::new(false),
      in_flight: Mutex::new(HashMap::new()),
      turns: Turns::default(),
      starts: starts::Starts::default(),
      open_jobs: jobs::Open::default(),
      enqueue_calls: Semaphore::new(ENQUEUE_CALLS),
      resumed: AtomicBool::new(false),
      asked: Mutex::new(HashMap::new()),
      #[cfg(test)]
      move_gate: tokio::sync::Mutex::new(()),
    });
    let slots = |options: &Options| options.transfers.download.slots.map(|n| n as usize);
    engine
      .starts
      .set_limit(slots(&engine.config.current().values));
    let followed = engine.clone();
    tokio::spawn(
      async move {
        let mut changes = followed.config.changes();
        loop {
          let change = tokio::select! {
            () = followed.shutdown.cancelled() => return,
            change = changes.recv() => change,
          };
          match change {
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
              followed
                .starts
                .set_limit(slots(&followed.config.current().values));
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
          }
        }
      }
      .in_current_span(),
    );
    engine
  }

  /// The downloads waiting in the local queue, in the order they start.
  pub fn queue_order(&self) -> Vec<Uuid> {
    self.starts.order()
  }

  /// Moves the waiting downloads `ids` to just before `before`, or to the end of the local
  /// queue, writes the new places and pushes the new order; returns those moved. A failed write
  /// is logged; the order holds until the next stop.
  pub async fn move_queued(&self, ids: &[Uuid], before: Option<Uuid>) -> Vec<Uuid> {
    let moved = self.starts.move_to(ids, before);
    if moved.is_empty() {
      return moved;
    }
    info!(moved = moved.len(), ?before, "queued downloads moved");
    self.live.publish(LiveFrame::TransfersQueue(
      api_types::transfers::DownloadQueue {
        ids: self.starts.order(),
      },
    ));
    let starts = self.starts.clone();
    let written = self
      .db
      .write(move |conn| {
        // read in the write, so of two moves the one written last writes the order now held
        let tx = conn.transaction()?;
        store::place(&tx, &starts.placed())?;
        tx.commit()
      })
      .await;
    if let Err(e) = written {
      warn!(error = %e, "local queue order not written");
    }
    moved
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
        if let (true, Some(job)) = (entered, record.job_id) {
          self.sync_job(job).await;
        }
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
  /// `remove` it leaves the listings once it has ended. False, doing nothing else, when the
  /// download has completed or its file arrived and is being moved. 404 when there is no such
  /// download.
  pub async fn cancel(&self, username: &str, id: Uuid, remove: bool) -> Result<bool, ApiError> {
    let mut record = self.record_of(username, id).await?;
    match self.cancel_running(id, remove, None) {
      Running::Stopped => return Ok(true),
      Running::Arrived => return Ok(remove),
      Running::No => {}
    }
    // a record no task runs any more, left behind by a failure, is ended canceled here; a task
    // that ended it since it was read keeps how it ended. The removal is in the same write, so
    // a removed download is left out of its job when that write ends the job
    let canceled = TransferStates::Completed | TransferStates::Cancelled;
    let at = Utc::now();
    let unfinished = !record.is_completed();
    if !unfinished && !remove {
      return Ok(false);
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
    if let Some(job) = record.job_id {
      self.sync_job(job).await;
    }
    Ok(true)
  }

  /// Stops the running download `id`, marking it for removal with `remove` and noting
  /// `reason`. One whose file arrived is not stopped, only marked for removal.
  fn cancel_running(&self, id: Uuid, remove: bool, reason: Option<&'static str>) -> Running {
    let mut in_flight = lock(&self.in_flight);
    let Some(entry) = in_flight.get_mut(&id) else {
      return Running::No;
    };
    entry.remove |= remove;
    if entry.arrived {
      debug!(%id, remove, "cancel of a download moving its file");
      return Running::Arrived;
    }
    entry.reason = entry.reason.or(reason);
    entry.cancel.cancel();
    debug!(%id, remove, "running download canceled");
    Running::Stopped
  }

  /// Hands the download `id`, whose file arrived, to its move, after which a pause or cancel
  /// no longer stops it; false when one came first and it is to end as that asked. The stop of
  /// the daemon is not such a cancel.
  fn hand_over(&self, id: Uuid) -> bool {
    let mut in_flight = lock(&self.in_flight);
    let entry = in_flight
      .get_mut(&id)
      .expect("a download is kept while its task runs");
    if entry.cancel.is_cancelled() && !self.shutdown.is_cancelled() {
      return false;
    }
    entry.arrived = true;
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
      if self.cancel_running(id, false, Some(BLOCKED)) == Running::Stopped {
        canceled += 1;
      }
    }
    info!(username, canceled, "downloads from a blocked user canceled");
  }

  /// Enqueues again the downloads a stop interrupted that were not removed since, each in its
  /// job and with its destination and search, as new downloads that supersede the old records,
  /// so a job goes on and is told of once all of it has ended. A file whole here already is
  /// enqueued apart from the rest of its group, so it is moved whether or not its user can be
  /// reached. Only the first call after a start does anything, which is after the first login;
  /// the marks are cleared before the enqueues, so a user that cannot be reached is logged,
  /// not tried again.
  pub async fn resume(&self) {
    if self.resumed.swap(true, Ordering::AcqRel) {
      return;
    }
    let resumable = match cleanup::take_resumable(&self.db).await {
      Ok(resumable) => resumable,
      Err(e) => return warn!(error = %e, "interrupted downloads not resumed"),
    };
    for (job, groups) in resumable {
      let groups = self.whole_apart(groups).await;
      self.ensure_listed(job, &groups).await;
      let asked: Vec<(String, usize)> = groups
        .iter()
        .map(|g| (g.username.clone(), g.files.len()))
        .collect();
      let results = self.enqueue(job, groups, false).await;
      for ((username, count), result) in asked.into_iter().zip(results) {
        match result {
          Ok(enqueued) => info!(
            username,
            %job,
            resumed = enqueued.enqueued.len(),
            failed = enqueued.failed.len(),
            "interrupted downloads resumed"
          ),
          // an offline user is no fault, so this logs as a refused enqueue does
          Err(e) => info!(
            username,
            %job,
            downloads = count,
            error = e.message(),
            "interrupted downloads not resumed; the user is likely offline"
          ),
        }
      }
    }
    // a job that ended or lost its downloads while the list was not told stays in step
    match self.jobs.unended(api_types::jobs::JobKind::Download).await {
      Ok(jobs) => {
        for job in jobs {
          // one stopped before its first download was written ends with nothing to show
          if self.sync_job(job.id).await {
            self
              .end_empty(
                job.id,
                api_types::jobs::JobEnding::Failed,
                Some("Nothing was queued".to_string()),
              )
              .await;
          }
        }
      }
      Err(e) => warn!(error = %e, "download jobs not checked after the restart"),
    }
  }

  /// `groups` with the files whole here already in a group of their own, ahead of the rest of
  /// the group they were in.
  async fn whole_apart(&self, groups: Vec<Group>) -> Vec<Group> {
    let mut split = Vec::with_capacity(groups.len());
    for mut group in groups {
      let mut whole = Vec::new();
      let mut rest = Vec::new();
      for file in group.files {
        if self.whole_here(&group.username, &file).await {
          whole.push(file);
        } else {
          rest.push(file);
        }
      }
      if !whole.is_empty() {
        split.push(Group {
          files: whole,
          username: group.username.clone(),
          destination: group.destination.clone(),
          search_id: group.search_id,
        });
      }
      if !rest.is_empty() {
        group.files = rest;
        split.push(group);
      }
    }
    split
  }

  /// Waits, at shutdown and once the client is stopped, for the download tasks the shutdown
  /// canceled to end; what they leave unfinished the stop ends after. A task still downloading
  /// gets [`STOPPING`]; one moving its arrived file is waited for until the move is done, so
  /// the file is never left half moved with its record unwritten.
  pub async fn stop(&self) {
    self.tasks.close();
    self.moving.close();
    // the limit passing is no error: what still runs is counted below
    let _ = tokio::time::timeout(STOPPING, self.tasks.wait()).await;
    // from here a download that arrives leaves its file for the next start
    *lock(&self.moves_closed) = true;
    let moving = self.moving.len();
    if moving > 0 {
      info!(moving, "waiting for downloads to move their files");
      self.moving.wait().await;
    }
    // a download whose file arrived handed its move to `moving` and ended, so what is left
    // here is still downloading
    let running = self.tasks.len();
    if running > 0 {
      warn!(running, "download tasks still running at shutdown");
    }
  }

  /// Runs `moving`, the move of a download whose file arrived and what follows it, as a task
  /// of its own the stop waits for without a limit; false, leaving it unrun, once the stop no
  /// longer waits for moves.
  fn spawn_move(&self, moving: impl Future<Output = ()> + Send + 'static) -> bool {
    let closed = lock(&self.moves_closed);
    if *closed {
      return false;
    }
    self.moving.spawn(moving);
    true
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
