//! Downloads and uploads. Each runs on a task of its own through its state machine; the client
//! keeps a record of every transfer from the moment it is accepted until its task has cleaned up,
//! one transfer per file, user and direction, with tokens unique across both directions. The
//! limits every transfer of a direction shares live here too: how many run at once, the speed
//! limit, and the one upload at a time each user gets. So do the order the downloads from one
//! user ask for their files in, and the offers of files users send us unasked, held until a
//! download takes them.

mod download;
mod io;
mod options;
mod record;
mod remote;
mod size;
mod state;
mod task;
mod upload;

pub use options::{
  DownloadOutput, DownloadRequest, DownloadSink, Governor, InputFactory, OutputFactory,
  ProgressUpdated, Reporter, RequestStyle, SizeMismatchResolver, SlotAwaiter, SlotReleased,
  StateChanged, TransferFuture, TransferOptions, UploadInput, UploadRequest, UploadSource,
};
pub use record::Transfer;
pub use state::{TransferOutcome, TransferState};
pub use task::TransferTask;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use soulseek_proto::peer::TransferRequest;
use soulseek_proto::{RemoteFilename, TransferDirection};
use tokio::sync::{Semaphore, watch};
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::bucket::{self, TokenBucket};
use crate::cache::BoundedCache;
use crate::client::Client;
use crate::error::ClientError;
use crate::limit::Limiter;
use crate::lock;
use crate::options::ClientOptions;
use crate::token::TokenGenerator;
use record::Record;

/// how often the per-user upload turns nobody holds or waits for are dropped
const UPLOAD_TURN_CLEANUP: Duration = Duration::from_secs(15 * 60);
/// peers whose broken resume offset was logged, so it is logged once per peer
const OFFSET_LOGGED_PEERS: usize = 1000;
/// how long a peer stays among those
const OFFSET_LOGGED_FOR: Duration = Duration::from_secs(24 * 3600);

/// what makes a transfer unique: the file, the user and the direction
type FileKey = (TransferDirection, String, RemoteFilename);

fn file_key(direction: TransferDirection, username: &str, filename: &RemoteFilename) -> FileKey {
  (direction, username.to_owned(), filename.clone())
}

/// where a download hears that its uploader refused or failed it
pub(crate) type RemoteFailures = watch::Receiver<Option<ClientError>>;

/// A tracked transfer
struct Tracked {
  direction: TransferDirection,
  record: Arc<Mutex<Record>>,
  /// stops the transfer: the caller's cancellation, or the client's disposal
  cancel: CancellationToken,
  /// downloads: fails the download wherever it is, when the uploader refuses or fails it
  remote_failure: Option<watch::Sender<Option<ClientError>>>,
  /// downloads: the uploader has been asked for the file, or its offer taken, so a refusal or
  /// failure it reports from now on can be about this download
  asked: bool,
}

#[derive(Default)]
struct Table {
  by_token: HashMap<u32, Tracked>,
  by_file: HashMap<FileKey, u32>,
}

/// What the client keeps for its transfers
pub(crate) struct Transfers {
  table: Mutex<Table>,
  pub(crate) download_slots: Limiter,
  pub(crate) upload_slots: Limiter,
  pub(crate) download_speed: TokenBucket,
  pub(crate) upload_speed: TokenBucket,
  /// one semaphore of one permit per user we upload to, so each user gets one upload at a time
  upload_turns: Mutex<HashMap<String, Arc<Semaphore>>>,
  /// peers that sent the broken resume offset, logged once each
  offset_logged: Mutex<BoundedCache<String, ()>>,
  /// the offers of files users send us unasked, by user and file, until a download takes them
  offered: Mutex<HashMap<(String, RemoteFilename), TransferRequest>>,
  /// by user, the downloads that have yet to ask for their file, in the order they started
  request_turns: Mutex<HashMap<String, watch::Sender<VecDeque<u64>>>>,
  next_turn: AtomicU64,
}

/// Who and what a new transfer is
pub(crate) struct NewTransfer<'a> {
  pub(crate) direction: TransferDirection,
  pub(crate) username: &'a str,
  pub(crate) filename: &'a RemoteFilename,
  pub(crate) size: u64,
  pub(crate) start_offset: u64,
  /// the caller's cancellation
  pub(crate) cancel: &'a CancellationToken,
}

/// A transfer's entry in the table, removed when dropped
pub(crate) struct Registration {
  transfers: Arc<Transfers>,
  token: u32,
  key: FileKey,
  pub(crate) record: Arc<Mutex<Record>>,
  /// stops the transfer: canceled with the caller's cancellation or when the client is disposed
  pub(crate) cancel: CancellationToken,
}

impl Drop for Registration {
  fn drop(&mut self) {
    let mut table = self.transfers.table();
    table.by_token.remove(&self.token);
    if table.by_file.get(&self.key) == Some(&self.token) {
      table.by_file.remove(&self.key);
    }
  }
}

impl Transfers {
  pub(crate) fn new(options: &ClientOptions) -> Self {
    Self {
      table: Mutex::new(Table::default()),
      download_slots: Limiter::new(options.maximum_concurrent_downloads),
      upload_slots: Limiter::new(options.maximum_concurrent_uploads),
      download_speed: TokenBucket::new(bucket::capacity(options.maximum_download_speed)),
      upload_speed: TokenBucket::new(bucket::capacity(options.maximum_upload_speed)),
      upload_turns: Mutex::new(HashMap::new()),
      offset_logged: Mutex::new(BoundedCache::new(OFFSET_LOGGED_PEERS, OFFSET_LOGGED_FOR)),
      offered: Mutex::new(HashMap::new()),
      request_turns: Mutex::new(HashMap::new()),
      next_turn: AtomicU64::new(0),
    }
  }

  fn table(&self) -> MutexGuard<'_, Table> {
    lock(&self.table)
  }

  /// Takes `token`, or a fresh one, and the file for one transfer, in one step so two callers
  /// cannot both pass the check. A download also gets the channel its remote failures arrive on.
  pub(crate) fn register(
    self: &Arc<Self>,
    tokens: &TokenGenerator,
    token: Option<u32>,
    new: NewTransfer<'_>,
  ) -> Result<(Registration, Option<RemoteFailures>), ClientError> {
    let NewTransfer {
      direction,
      username,
      filename,
      size,
      start_offset,
      cancel,
    } = new;
    let mut table = self.table();
    let token = match token {
      Some(token) if table.by_token.contains_key(&token) => {
        return Err(ClientError::DuplicateToken(token));
      }
      Some(token) => token,
      None => tokens.next_free(|token| table.by_token.contains_key(&token)),
    };
    let key = file_key(direction, username, filename);
    if table.by_file.contains_key(&key) {
      return Err(ClientError::DuplicateTransfer {
        username: username.to_owned(),
        filename: filename.to_string(),
      });
    }
    let record = Arc::new(Mutex::new(Record::new(
      direction,
      username,
      filename,
      token,
      size,
      start_offset,
    )));
    let (remote_failure, failures) = match direction {
      TransferDirection::Download => {
        let (sender, receiver) = watch::channel(None);
        (Some(sender), Some(receiver))
      }
      TransferDirection::Upload => (None, None),
    };
    let cancel = cancel.child_token();
    table.by_token.insert(
      token,
      Tracked {
        direction,
        record: record.clone(),
        cancel: cancel.clone(),
        remote_failure,
        asked: false,
      },
    );
    table.by_file.insert(key.clone(), token);
    Ok((
      Registration {
        transfers: self.clone(),
        token,
        key,
        record,
        cancel,
      },
      failures,
    ))
  }

  /// whether a download of `filename` from `username` is tracked
  pub(crate) fn has_download(&self, username: &str, filename: &RemoteFilename) -> bool {
    let key = file_key(TransferDirection::Download, username, filename);
    self.table().by_file.contains_key(&key)
  }

  /// Holds the offer `username` made to send us a file unasked; false, holding nothing, when an
  /// offer of the same file is held already. That one is being decided or was accepted, and the
  /// download that takes it must get the offer whose size was checked
  pub(crate) fn hold_offer(&self, username: &str, offer: TransferRequest) -> bool {
    let key = (username.to_owned(), offer.filename.clone());
    match lock(&self.offered).entry(key) {
      Entry::Occupied(_) => false,
      Entry::Vacant(vacant) => {
        vacant.insert(offer);
        true
      }
    }
  }

  /// takes the offer `username` holds for `filename`, for the download that accepts it
  pub(crate) fn take_offer(
    &self,
    username: &str,
    filename: &RemoteFilename,
  ) -> Option<TransferRequest> {
    lock(&self.offered).remove(&(username.to_owned(), filename.clone()))
  }

  /// Lets go of the offer of `filename` from `username` with `token`, unless a download took it
  /// or another offer is held in its place; returns whether it was still held
  pub(crate) fn release_offer(
    &self,
    username: &str,
    filename: &RemoteFilename,
    token: u32,
  ) -> bool {
    let mut offered = lock(&self.offered);
    let key = (username.to_owned(), filename.clone());
    if offered.get(&key).is_some_and(|offer| offer.token == token) {
      offered.remove(&key);
      return true;
    }
    false
  }

  /// The download `token` is about to ask its uploader for the file, or to take its offer: a
  /// refusal or failure the uploader reports from now on can be about it
  pub(crate) fn asked(&self, token: u32) {
    if let Some(tracked) = self.table().by_token.get_mut(&token) {
      tracked.asked = true;
    }
  }

  /// whether a download of `filename` from `username` is tracked and has asked for the file
  pub(crate) fn has_asked(&self, username: &str, filename: &RemoteFilename) -> bool {
    let table = self.table();
    let key = file_key(TransferDirection::Download, username, filename);
    table
      .by_file
      .get(&key)
      .and_then(|token| table.by_token.get(token))
      .is_some_and(|tracked| tracked.asked)
  }

  /// Fails the download of `filename` from `username`, at whatever stage it is, with `error`.
  /// A download that has not asked for the file yet is left alone: the uploader cannot have
  /// refused or failed what it was not asked for, so the report is about an earlier download
  /// of the same file.
  pub(crate) fn fail_download(
    &self,
    username: &str,
    filename: &RemoteFilename,
    error: ClientError,
  ) -> RemoteFailure {
    let table = self.table();
    let key = file_key(TransferDirection::Download, username, filename);
    let Some(tracked) = table
      .by_file
      .get(&key)
      .and_then(|token| table.by_token.get(token))
    else {
      return RemoteFailure::Untracked;
    };
    if !tracked.asked {
      return RemoteFailure::NotAsked;
    }
    if let Some(remote_failure) = &tracked.remote_failure {
      remote_failure.send_replace(Some(error));
    }
    RemoteFailure::Ended
  }

  /// Takes the last place among the downloads from `username` that have yet to ask for their
  /// file
  pub(crate) fn request_turn(self: &Arc<Self>, username: &str) -> RequestTurn {
    let id = self.next_turn.fetch_add(1, Ordering::Relaxed);
    lock(&self.request_turns)
      .entry(username.to_owned())
      .or_insert_with(|| watch::Sender::new(VecDeque::new()))
      .send_modify(|waiting| waiting.push_back(id));
    RequestTurn {
      transfers: self.clone(),
      username: username.to_owned(),
      id,
    }
  }

  /// stops every transfer, for when the client is disposed
  pub(crate) fn cancel_all(&self) {
    let table = self.table();
    if !table.by_token.is_empty() {
      tracing::debug!(count = table.by_token.len(), "transfers canceled");
    }
    for tracked in table.by_token.values() {
      tracked.cancel.cancel();
    }
  }

  /// snapshots of the tracked transfers going `direction`
  fn snapshots(&self, direction: TransferDirection) -> Vec<Transfer> {
    self
      .table()
      .by_token
      .values()
      .filter(|tracked| tracked.direction == direction)
      .map(|tracked| lock(&tracked.record).snapshot())
      .collect()
  }

  /// the semaphore that gives `username` one upload at a time, made on first use
  pub(crate) fn upload_turn(&self, username: &str) -> Arc<Semaphore> {
    lock(&self.upload_turns)
      .entry(username.to_owned())
      .or_insert_with(|| Arc::new(Semaphore::new(1)))
      .clone()
  }

  /// Whether to log a broken resume offset from `username`: true the first time only
  pub(crate) fn first_bad_offset_from(&self, username: &str) -> bool {
    let mut logged = lock(&self.offset_logged);
    if logged.get(&username.to_owned()).is_some() {
      return false;
    }
    logged.insert(username.to_owned(), ());
    true
  }

  /// follows a change of the concurrency and speed options
  pub(crate) fn reconfigure(&self, options: &ClientOptions) {
    self
      .download_slots
      .resize(options.maximum_concurrent_downloads);
    self.upload_slots.resize(options.maximum_concurrent_uploads);
    self
      .download_speed
      .set_capacity(bucket::capacity(options.maximum_download_speed));
    self
      .upload_speed
      .set_capacity(bucket::capacity(options.maximum_upload_speed));
  }
}

/// What a refusal or failure the uploader reported did to the download it names
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoteFailure {
  /// the download ends with it
  Ended,
  /// the download has not asked for the file yet, so the report is about an earlier one
  NotAsked,
  /// no download of that file is tracked
  Untracked,
}

/// A download's place among the downloads from one user that have yet to ask for their file.
/// Requests to a user go out in the order the downloads started, so an album is queued at the
/// peer track by track; leaving the place, by asking or by ending, lets the next one ask.
pub(crate) struct RequestTurn {
  transfers: Arc<Transfers>,
  username: String,
  id: u64,
}

impl RequestTurn {
  /// resolves once every download from the user that started before this one has asked or ended
  pub(crate) async fn reached(&self) {
    let mut waiting = lock(&self.transfers.request_turns)
      .get(&self.username)
      .expect("a user's turns are kept while one of them is held")
      .subscribe();
    waiting
      .wait_for(|waiting| waiting.front() == Some(&self.id))
      .await
      .expect("a user's turns are kept while one of them is held");
  }
}

impl Drop for RequestTurn {
  fn drop(&mut self) {
    let mut turns = lock(&self.transfers.request_turns);
    let Some(waiting) = turns.get(&self.username) else {
      return;
    };
    waiting.send_modify(|waiting| waiting.retain(|id| *id != self.id));
    if waiting.borrow().is_empty() {
      turns.remove(&self.username);
    }
  }
}

impl Client {
  /// every download the client tracks, as it is now
  pub fn downloads(&self) -> Vec<Transfer> {
    self.transfers().snapshots(TransferDirection::Download)
  }

  /// every upload the client tracks, as it is now
  pub fn uploads(&self) -> Vec<Transfer> {
    self.transfers().snapshots(TransferDirection::Upload)
  }

  /// Every fifteen minutes, drops the per-user upload turns no upload holds or waits for.
  pub(crate) fn start_upload_turn_cleanup(&self) {
    let client = self.downgrade();
    let lifetime = self.cancellation().clone();
    tokio::spawn(
      async move {
        let mut ticks = interval_at(Instant::now() + UPLOAD_TURN_CLEANUP, UPLOAD_TURN_CLEANUP);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
          tokio::select! {
            () = lifetime.cancelled() => return,
            _ = ticks.tick() => {}
          }
          let Some(client) = client.upgrade() else {
            return;
          };
          let mut turns = lock(&client.transfers().upload_turns);
          let before = turns.len();
          // an upload holds a clone from the moment it is queued until it is cleaned up, so a
          // turn only the map refers to is neither held nor waited for
          turns.retain(|_, turn| Arc::strong_count(turn) > 1);
          tracing::trace!(dropped = before - turns.len(), "idle upload turns dropped");
        }
      }
      .in_current_span(),
    );
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;

  #[tokio::test(start_paused = true)]
  async fn idle_upload_turns_are_dropped_every_fifteen_minutes() {
    let client = Client::new(ClientOptions {
      enable_listener: false,
      ..ClientOptions::default()
    })
    .unwrap();
    let held = client.transfers().upload_turn("alice");
    client.transfers().upload_turn("bob");
    tokio::time::sleep(UPLOAD_TURN_CLEANUP - Duration::from_secs(1)).await;
    assert_eq!(lock(&client.transfers().upload_turns).len(), 2);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let turns = lock(&client.transfers().upload_turns);
    assert_eq!(turns.keys().collect::<Vec<_>>(), ["alice"]);
    drop(held);
  }
}
