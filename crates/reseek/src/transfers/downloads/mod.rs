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
mod received;
mod resume;
mod service;
mod starts;
mod task;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use api_types::live::LiveFrame;
use api_types::transfers::{TransferDirection, TransferOutcome, TransferStates};
use cascade::Watcher;
use chrono::Utc;
use reseek_shares::lock;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, error, info, warn};
use uuid::Uuid;

use super::store::Saved;
use super::{BLOCKED, Record, STOPPING, removal, store};
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

/// How many enqueue calls of the API run at once.
const ENQUEUE_CALLS: usize = 2;

/// How many places in queue peers sent on their own wait to be written.
const PLACES_WAITING: usize = 64;

/// Why a download canceled without a running task ended.
const CANCELED: &str = "The operation was canceled.";

/// Why a pause or cancel of a download that finished, or whose file is being moved, is refused.
const COMPLETED: &str = "the download has completed";

/// What is kept for a download whose task runs.
struct InFlight {
  /// stops the download wherever it is
  cancel: CancellationToken,
  /// what the task was asked to end as
  ask: Ask,
  /// the task took the pause asked for and writes the download paused, so a cancel that comes
  /// now ends the row itself
  pause_taken: bool,
  /// why it was canceled, when not by a plain cancel
  reason: Option<&'static str>,
  /// its file arrived and is handed to its move, which a pause or cancel no longer stops
  arrived: bool,
  /// for a file its user sends unasked, what counts it against the files they may send at once
  /// until the entry goes
  _sending: Option<received::Sending>,
  /// dropped with the entry once the task has let go of the download, which ends every wait on
  /// a receiver of it
  ended: watch::Sender<()>,
}

impl InFlight {
  /// A download about to start its task, stopped by `cancel`.
  fn new(cancel: CancellationToken) -> Self {
    Self {
      cancel,
      ask: Ask::Nothing,
      pause_taken: false,
      reason: None,
      arrived: false,
      _sending: None,
      ended: watch::Sender::new(()),
    }
  }
}

impl InFlight {
  /// Whether a cancel asked for the download to leave the listings once it has ended.
  fn removing(&self) -> bool {
    matches!(self.ask, Ask::Cancel { remove: true })
  }
}

/// What a download's task was asked to end as. A cancel beats a pause whichever came first;
/// a second ask of the same kind changes nothing but adds a removal a cancel asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
  /// nothing: it runs on
  Nothing,
  /// paused: it waits here again, keeping what arrived
  Pause,
  /// canceled, and taken out of the listings once ended with `remove`
  Cancel { remove: bool },
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
  /// the files users are sending unasked, and the job each sender's go to
  senders: Arc<Mutex<received::Senders>>,
  /// one enqueue per user at a time
  turns: Turns,
  /// one bringing in step of each job in the jobs list at a time, keyed by the job's id
  job_syncs: Turns,
  /// the local queue: the order waiting downloads start in
  starts: starts::Starts,
  /// the jobs whose files are still being enqueued
  open_jobs: jobs::Open,
  /// the enqueue calls of the API that may run at once
  enqueue_calls: Semaphore,
  /// whether no download a stop interrupted waits to be resumed any more, held by the resume
  /// under way and by a cancel giving up a job's waiting files
  resuming: tokio::sync::Mutex<bool>,

  /// when each running download last asked its peer for its place in the queue
  asked: Mutex<HashMap<Uuid, Instant>>,
  /// the places in queue peers sent on their own, written one at a time in the order they came
  places: mpsc::Sender<queue::SentPlace>,
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
    let (places, sent_places) = mpsc::channel(PLACES_WAITING);
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
      senders: Arc::default(),
      turns: Turns::default(),
      job_syncs: Turns::default(),
      starts: starts::Starts::default(),
      open_jobs: jobs::Open::default(),
      enqueue_calls: Semaphore::new(ENQUEUE_CALLS),
      resuming: tokio::sync::Mutex::default(),
      asked: Mutex::new(HashMap::new()),
      places,
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
    let placing = engine.clone();
    tokio::spawn(async move { placing.write_places(sent_places).await }.in_current_span());
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
  /// that ends the last download of its job tells of the job after. Returns how the write went,
  /// none when it failed; a write that failed or was not made is logged, and the download goes
  /// on.
  async fn save(&self, record: &Record, entered: bool) -> Option<Saved> {
    let written = record.clone();
    let open = self.open_jobs.clone();
    let result = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let ending = written.job_id.is_some() && written.is_completed();
        let unfinished = ending && jobs::unfinished(&tx, written.id)?;
        let saved = store::save(&tx, &written)?;
        let found = saved == Saved::Written;
        if found && entered {
          store::entered(&tx, written.id, written.state.kind(), Utc::now())?;
        }
        let finished = if found {
          jobs::ended_by(&tx, &written, unfinished, &open)?
        } else {
          None
        };
        tx.commit()?;
        Ok((saved, finished))
      })
      .await;
    match result {
      Ok((Saved::Written, finished)) => {
        if !record.removed || entered {
          self.push(record);
        }
        self.tell(finished);
        if let (true, Some(job)) = (entered, record.job_id) {
          self.sync_job(job).await;
        }
        Some(Saved::Written)
      }
      Ok((Saved::Gone, _)) => {
        warn!(id = %record.id, "download record gone; not written");
        Some(Saved::Gone)
      }
      // a write that raced the download ending, which keeps how it ended
      Ok((Saved::Refused(refused), _)) => {
        debug!(id = %record.id, %refused, "download record not written");
        Some(Saved::Refused(refused))
      }
      Err(e) => {
        warn!(id = %record.id, error = %e, "download record not written");
        None
      }
    }
  }

  /// Tells of the downloads `removed` taken out of the listings: each is pushed as removed and
  /// each job one was in is brought in step, so a job whose last listed download went ends
  /// there. Every removal of downloads the engine makes is told here.
  pub async fn taken_out(&self, removed: &[removal::Removed]) {
    removal::push(&self.live, TransferDirection::Download, removed);
    let jobs: std::collections::BTreeSet<Uuid> = removed.iter().filter_map(|r| r.job).collect();
    for job in jobs {
      self.sync_job(job).await;
    }
  }

  /// Pushes `record` to the event stream: as removed when it is, else as updated.
  fn push(&self, record: &Record) {
    if record.removed {
      let taken = removal::Removed {
        id: record.id,
        username: record.username.clone(),
        job: record.job_id,
      };
      removal::push(&self.live, TransferDirection::Download, &[taken]);
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
      .ok_or_else(|| not_found(username, id))
  }

  /// Cancels the download `id` of `username`: a running one stops wherever it is and ends
  /// canceled; one whose task is gone without ending it is ended canceled here. With
  /// `remove` it leaves the listings once it has ended, and one whose row cannot be read leaves
  /// them at once. False, doing nothing else but the removal, when the download has completed,
  /// its task included completing it meanwhile, or its file arrived and is being moved. 404
  /// when there is no such download.
  pub async fn cancel(&self, username: &str, id: Uuid, remove: bool) -> Result<bool, ApiError> {
    let read = super::listing::record(&self.db, TransferDirection::Download, username, id).await;
    let record = match read {
      Ok(Some(record)) => record,
      Ok(None) => return Err(not_found(username, id)),
      Err(DbError::Sqlite(e)) if remove && super::unreadable(&e) => {
        let direction = TransferDirection::Download;
        let removed = removal::remove_unreadable(&self.db, direction, username, id)
          .await
          .map_err(internal)?;
        if removed.is_empty() {
          return Err(not_found(username, id));
        }
        info!(%id, username, error = %e, "download that cannot be read removed");
        self.taken_out(&removed).await;
        return Ok(true);
      }
      Err(e) => return Err(internal(e)),
    };
    match self.cancel_running(id, remove, None) {
      Running::Stopped => Ok(true),
      Running::Arrived => Ok(remove),
      Running::No => {
        let canceled = self.cancel_unrun(record, remove).await?;
        // a resume that started its task after the check above saved its record before the
        // cancel's write, which ended it, so that task is stopped here
        if canceled {
          self.cancel_running(id, remove, None);
        }
        Ok(canceled)
      }
    }
  }

  /// Ends canceled the download of `record`, read before it was found to have no task, as
  /// [`Downloads::cancel`] answers. A task that ended it since it was read keeps how it ended.
  /// The removal is in the same write, so a removed download is left out of its job when that
  /// write ends the job.
  async fn cancel_unrun(&self, mut record: Record, remove: bool) -> Result<bool, ApiError> {
    let id = record.id;
    let canceled = TransferStates::Completed(TransferOutcome::Cancelled);
    let unfinished = record
      .complete(TransferOutcome::Cancelled, Some(CANCELED.to_string()))
      .is_ok();
    // as the end writes it: a download ended is no longer paused
    record.paused = false;
    if !unfinished && !remove {
      return Ok(false);
    }
    let at = record.ended_at.unwrap_or_else(Utc::now);
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
      info!(%id, username = record.username, "download without a running task ended as canceled");
      self.push(&record);
    }
    // the removal is pushed before the job it may have ended is told of
    let taken = removed.is_some();
    if let Some(removed) = removed {
      self.taken_out(&[removed]).await;
    }
    self.tell(finished);
    if let (false, Some(job)) = (taken, record.job_id) {
      self.sync_job(job).await;
    }
    Ok(ended || remove)
  }

  /// Asks the running download `id` to end canceled, marking it for removal with `remove` and
  /// noting `reason`, over a pause asked for before. One whose file arrived is not stopped,
  /// only marked for removal. One whose task took a pause and is writing it paused answers as
  /// one no task runs, for the cancel to end the row itself.
  fn cancel_running(&self, id: Uuid, remove: bool, reason: Option<&'static str>) -> Running {
    let mut in_flight = lock(&self.in_flight);
    let Some(entry) = in_flight.get_mut(&id) else {
      return Running::No;
    };
    let removing = entry.removing() || remove;
    entry.ask = Ask::Cancel { remove: removing };
    if entry.pause_taken {
      debug!(%id, remove, "cancel of a download writing itself paused");
      return Running::No;
    }
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
    let direction = TransferDirection::Download;
    let ids = match super::listing::unfinished(&self.db, direction, username).await {
      Ok(ids) => ids,
      Err(e) => return warn!(username, error = %e, "downloads from a blocked user not canceled"),
    };
    let mut canceled = 0;
    for id in ids {
      if self.cancel_running(id, false, Some(BLOCKED)) == Running::Stopped {
        canceled += 1;
      }
    }
    info!(username, canceled, "downloads from a blocked user canceled");
  }

  /// Runs `forget`, the forgetting of `username` with what goes with it, on the engine's task
  /// set, which the stop waits for as for the download tasks, and waits for it: a caller that
  /// stops waiting leaves it running to its end, so no user is left half purged. Its failure is
  /// logged here, once.
  pub async fn forget_user(
    &self,
    username: &str,
    forget: impl Future<Output = Result<(), ApiError>> + Send + 'static,
  ) -> Result<(), ApiError> {
    let name = username.to_string();
    let ran = self.tasks.spawn(
      async move {
        let forgotten = forget.await;
        if let Err(e) = &forgotten {
          match e.reason() {
            Some(reason) => error!(username = name, reason, "user not forgotten"),
            None => debug!(username = name, error = e.message(), "user not forgotten"),
          }
        }
        forgotten.map_err(ApiError::logged)
      }
      .in_current_span(),
    );
    match ran.await {
      Ok(forgotten) => forgotten,
      Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
      Err(e) => Err(ApiError::internal(e)),
    }
  }

  /// Cancels every unfinished download from `username`, each leaving the listings once it has
  /// ended as a cancel with removal does, then takes out the finished ones; for a user forgotten
  /// with their transfers. Returns how many it canceled or took out.
  pub async fn purge(&self, username: &str) -> Result<usize, ApiError> {
    let direction = TransferDirection::Download;
    let ids = super::listing::unfinished(&self.db, direction, username)
      .await
      .map_err(internal)?;
    let mut canceled = 0;
    for id in ids {
      match self.cancel(username, id, true).await {
        Ok(_) => canceled += 1,
        Err(e) => {
          let error = e.reason().unwrap_or(e.message());
          warn!(%id, username, error, "download of a purged user not canceled");
        }
      }
    }
    let removed = removal::remove_finished_of(&self.db, direction, username)
      .await
      .map_err(internal)?;
    self.taken_out(&removed).await;
    info!(
      username,
      canceled,
      removed = removed.len(),
      "downloads of a forgotten user purged"
    );
    Ok(canceled + removed.len())
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

/// A database failure as the API answers it.
fn internal(error: DbError) -> ApiError {
  ApiError::internal(error)
}

/// The answer for a download `id` of `username` there is none of.
fn not_found(username: &str, id: Uuid) -> ApiError {
  ApiError::not_found(format!("No download {id} from {username}"))
}
