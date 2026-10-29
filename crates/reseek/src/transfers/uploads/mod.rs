//! The upload engine: what we do when another user asks for one of our files. A request is
//! checked at the network's edge, then decided here, one at a time per user: the file must be
//! shared and on disk, and the user's group and limits must allow it. An accepted request is
//! recorded, put in the queue, and handed to a task that runs the client's upload once the
//! queue gives it a slot, its bytes paced by its group's speed limit.
//!
//! [`Uploads`] keeps every running upload's cancel handle, keyed by its id, from the moment its
//! record is written until its task has written how it ended. Every record written is pushed
//! to the `transfers` topic. Uploads do not outlive the daemon: a stop ends the ones still
//! queued or running, and their users ask again.

mod enqueue;
mod governor;
mod intake;
mod limits;
mod queue;
mod service;
mod task;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::transfers::TransferDirection;
use cascade::Watcher;
use soulseek_client::{Client, ClientError};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, info, info_span, warn};
use uuid::Uuid;

use super::{Record, removal, store};
use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::Database;
use crate::events::EventBus;
use crate::live::Live;
use crate::network::Network;
use crate::relay::Controller;
use crate::shares::Shares;
use crate::sync::lock;
use crate::telemetry::Telemetry;
use crate::users::Directory;
use governor::Governor;
use intake::Intake;
pub use intake::{NOT_SHARED, OVERWHELMED};
pub use queue::Forecast;
use queue::UploadQueue;

/// How long the tasks of running uploads get to end at shutdown.
const STOPPING: Duration = Duration::from_secs(5);

/// Why an upload canceled at block time ended.
const BLOCKED: &str = "Blocked";

/// What is kept for an upload whose task runs.
struct InFlight {
  /// stops the upload wherever it is
  cancel: CancellationToken,
  /// remove the record from the listings once it has ended
  remove: bool,
  /// why it was canceled, when not by a plain cancel
  reason: Option<&'static str>,
  /// the client's upload ended, before the task has written how
  ended: bool,
}

/// The upload engine.
pub struct Uploads {
  this: Weak<Uploads>,
  db: Database,
  config: Watcher<Options>,
  /// the client, while the daemon runs
  network: Weak<Network>,
  users: Arc<Directory>,
  shares: Arc<Shares>,
  events: EventBus,
  live: Live,
  telemetry: Arc<Telemetry>,
  /// the relay controller, which serves the files of its agents, in controller and debug mode
  relay: Option<Arc<Controller>>,
  /// an agent's file is taken as its index has it, without asking the agent
  trusts_agents: bool,
  shutdown: CancellationToken,
  /// every upload task, so the stop waits for them
  tasks: TaskTracker,
  in_flight: Mutex<HashMap<Uuid, InFlight>>,
  queue: UploadQueue,
  governor: Arc<Governor>,
  intake: Intake,
}

impl Uploads {
  /// The engine over `transfers.db`, uploading through `network`'s client, finding files in
  /// `shares` and users' groups in `users`, raising its events on `events`, pushing its
  /// records to `live`, counting in `telemetry` and getting relay agents' files from `relay`. It
  /// follows the options until `shutdown`, which also ends its tasks.
  #[allow(
    clippy::too_many_arguments,
    reason = "the engine takes each part of the daemon it uses"
  )]
  pub fn new(
    db: Database,
    config: Watcher<Options>,
    network: &Arc<Network>,
    users: Arc<Directory>,
    shares: Arc<Shares>,
    events: EventBus,
    live: Live,
    telemetry: Arc<Telemetry>,
    relay: Option<Arc<Controller>>,
    shutdown: CancellationToken,
  ) -> Arc<Self> {
    let options = config.current().values.clone();
    let grouped = users.clone();
    let uploads = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db,
      config,
      network: Arc::downgrade(network),
      users,
      shares,
      events,
      live,
      telemetry,
      relay,
      trusts_agents: options.flags.optimistic_relay_file_info,
      shutdown,
      tasks: TaskTracker::new(),
      in_flight: Mutex::new(HashMap::new()),
      queue: UploadQueue::new(&options, Box::new(move |username| grouped.group(username))),
      governor: Arc::new(Governor::new(&options)),
      intake: Intake::default(),
    });
    tokio::spawn(
      uploads
        .clone()
        .follow_configuration()
        .instrument(info_span!("upload_groups")),
    );
    uploads
  }

  /// This engine, for its tasks to hold.
  fn this(&self) -> Arc<Self> {
    self
      .this
      .upgrade()
      .expect("the engine is alive while a method runs on it")
  }

  /// The Soulseek client; gone only once the daemon is torn down.
  fn client(&self) -> Result<Client, ClientError> {
    self
      .network
      .upgrade()
      .map(|network| network.client().clone())
      .ok_or(ClientError::Disposed)
  }

  /// Where a new upload request from `username` would land in the queue.
  pub fn forecast(&self, username: &str) -> Forecast {
    self.queue.forecast(username)
  }

  /// Where a new upload request from a user we know nothing about would land in the queue.
  pub fn default_forecast(&self) -> Forecast {
    self.queue.default_forecast()
  }

  /// The place a peer is told of the latest upload of `filename` to `username`, `None` when
  /// the queue holds none. Peers count from 1, so it is the queue's own place plus one.
  pub fn place(&self, username: &str, filename: &str) -> Option<u32> {
    let place = self
      .queue
      .place(username, filename)
      .map(|place| place.saturating_add(1));
    debug!(username, filename, ?place, "place in the upload queue");
    place
  }

  /// Takes every change of the options into the queue's groups and the speed limits, and
  /// watches the users a change put in a user-defined group while logged in.
  async fn follow_configuration(self: Arc<Self>) {
    let mut changes = self.config.changes();
    loop {
      let change = tokio::select! {
        () = self.shutdown.cancelled() => return,
        change = changes.recv() => change,
      };
      let (previous, current) = match change {
        Ok(change) => (Some(change.previous.clone()), change.current.clone()),
        Err(RecvError::Lagged(missed)) => {
          debug!(
            missed,
            "options changes missed; the upload groups follow the current ones"
          );
          (None, self.config.current())
        }
        Err(RecvError::Closed) => return,
      };
      let options = &current.values;
      self.queue.configure(options);
      self.governor.configure(options);
      let added: Vec<String> = members(options)
        .into_iter()
        .filter(|member| {
          previous
            .as_ref()
            .is_none_or(|p| !members(&p.values).contains(member))
        })
        .collect();
      if !added.is_empty()
        && let Ok(client) = self.client()
        && client.state() == soulseek_client::ClientState::READY
      {
        for member in added {
          self.users.watch(&client, &member).await;
        }
      }
    }
  }

  /// After a login: the users with uploads in the queue and every member of a user-defined
  /// group are watched, so their groups are current.
  pub async fn logged_in(&self) {
    let Ok(client) = self.client() else {
      return;
    };
    let mut users = self.queue.users();
    for member in members(&self.config.current().values) {
      if !users.contains(&member) {
        users.push(member);
      }
    }
    for username in users {
      self.users.watch(&client, &username).await;
    }
  }

  /// Writes the state, times, bytes, speed and error of `record`, adds its state to its
  /// history when it `entered` it, and pushes it. Whether it was removed is left as the
  /// listings have it. A failed write is logged; the upload goes on.
  async fn save(&self, record: &Record, entered: bool) {
    let written = record.clone();
    let result = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        let found = store::save_state(&tx, &written)?;
        if found && entered {
          store::entered(&tx, written.id, written.state, chrono::Utc::now())?;
        }
        let saved = store::get(&tx, written.id)?;
        tx.commit()?;
        Ok(saved)
      })
      .await;
    match result {
      Ok(Some(saved)) if !saved.removed => self
        .live
        .publish(LiveFrame::TransfersUpdated(Box::new(saved.transfer()))),
      Ok(Some(_)) => {}
      Ok(None) => warn!(id = %record.id, "upload record gone; not written"),
      Err(e) => warn!(id = %record.id, error = %e, "upload record not written"),
    }
  }

  /// The upload `id` to `username`, removed or not; 404 when there is none.
  async fn record_of(&self, username: &str, id: Uuid) -> Result<Record, ApiError> {
    super::listing::record(&self.db, TransferDirection::Upload, username, id)
      .await
      .map_err(ApiError::internal)?
      .ok_or_else(|| ApiError::not_found(format!("No upload {id} to {username}")))
  }

  /// Cancels the upload `id` to `username` if it runs; with `remove` it leaves the listings,
  /// once it has ended when it still runs. 404 when there is no such upload.
  pub async fn cancel(&self, username: &str, id: Uuid, remove: bool) -> Result<(), ApiError> {
    self.record_of(username, id).await?;
    if self.cancel_running(id, remove, None) {
      return Ok(());
    }
    if remove {
      removal::remove(&self.db, &self.live, TransferDirection::Upload, id)
        .await
        .map_err(ApiError::internal)?;
    }
    Ok(())
  }

  /// Stops the running upload `id`, marking it for removal with `remove` and noting `reason`;
  /// false when it is not running.
  fn cancel_running(&self, id: Uuid, remove: bool, reason: Option<&'static str>) -> bool {
    let mut in_flight = lock(&self.in_flight);
    let Some(entry) = in_flight.get_mut(&id) else {
      return false;
    };
    entry.remove |= remove;
    entry.reason = entry.reason.or(reason);
    entry.cancel.cancel();
    debug!(%id, remove, "running upload canceled");
    true
  }

  /// Cancels every upload to `username` still waiting in the queue, ending each with
  /// "Blocked"; for a user just blocked from uploads.
  pub fn blocked(&self, username: &str) {
    let waiting = self.queue.waiting(username);
    let canceled = waiting
      .iter()
      .filter(|id| self.cancel_running(**id, false, Some(BLOCKED)))
      .count();
    info!(
      username,
      canceled, "queued uploads to a blocked user canceled"
    );
  }

  /// Every minute: drops the per-user turns of the intake that nobody holds or waits for.
  pub fn collect_idle_turns(&self) {
    self.intake.collect_idle();
  }

  /// Waits, at shutdown and once the client is stopped, for the upload tasks the shutdown
  /// canceled to end; what they leave unfinished the stop ends after.
  pub async fn stop(&self) {
    self.tasks.close();
    if tokio::time::timeout(STOPPING, self.tasks.wait())
      .await
      .is_err()
    {
      warn!(
        running = self.tasks.len(),
        "upload tasks still running at shutdown"
      );
    }
  }
}

/// Every member of a user-defined group in `options`.
fn members(options: &Options) -> Vec<String> {
  let mut members: Vec<String> = options
    .transfers
    .groups
    .user_defined
    .values()
    .flat_map(|group| group.members.iter().cloned())
    .collect();
  members.sort();
  members.dedup();
  members
}
