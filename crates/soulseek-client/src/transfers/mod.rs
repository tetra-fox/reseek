//! Downloads and uploads. Each runs on a task of its own through its state machine; the client
//! keeps a record of every transfer from the moment it is accepted until its task has cleaned up,
//! one transfer per file, user and direction, with tokens unique across both directions. The
//! limits every transfer of a direction shares live here too: how many run at once, the speed
//! limit, and the one upload at a time each user gets.

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
pub use state::TransferState;
pub use task::TransferTask;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

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

  /// Fails the download of `filename` from `username`, at whatever stage it is, with `error`;
  /// returns whether one was tracked
  pub(crate) fn fail_download(
    &self,
    username: &str,
    filename: &RemoteFilename,
    error: ClientError,
  ) -> bool {
    let table = self.table();
    let key = file_key(TransferDirection::Download, username, filename);
    let Some(tracked) = table
      .by_file
      .get(&key)
      .and_then(|token| table.by_token.get(token))
    else {
      return false;
    };
    if let Some(remote_failure) = &tracked.remote_failure {
      remote_failure.send_replace(Some(error));
    }
    true
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
