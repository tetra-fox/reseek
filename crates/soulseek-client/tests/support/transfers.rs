//! What the transfer suites share: files to send, a client that queues every upload a peer asks
//! for and hands the request to the test, and the states a transfer goes through.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use soulseek_client::{ClientOptions, Event, Transfer, TransferOutcome, TransferState};
use soulseek_proto::{RemoteFilename, TransferDirection};
use tokio::sync::{broadcast, mpsc};

use super::{PATIENCE, next_event};

static DIRECTORIES: AtomicU32 = AtomicU32::new(0);

/// an empty directory of its own under the target directory
pub fn scratch() -> PathBuf {
  let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
    "transfers-{}-{}",
    std::process::id(),
    DIRECTORIES.fetch_add(1, Ordering::Relaxed)
  ));
  // a directory left by an earlier run of the same process id goes first
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

/// `len` bytes that differ from one offset to the next, so a byte out of place shows
pub fn content(len: usize) -> Vec<u8> {
  (0..len).map(|n| (n * 7 % 251) as u8).collect()
}

/// a file of [`content`] in `dir`
pub fn file(dir: &std::path::Path, name: &str, len: usize) -> PathBuf {
  let path = dir.join(name);
  std::fs::write(&path, content(len)).unwrap();
  path
}

/// A peer's request to queue an upload, as the client's resolver saw it
pub type Asked = mpsc::UnboundedReceiver<(String, RemoteFilename)>;

/// Has `options` queue every upload a peer asks for and hand the request to the returned
/// receiver, which is where a test starts the upload
pub fn queueing(options: &mut ClientOptions) -> Asked {
  let (asked, requests) = mpsc::unbounded_channel();
  options.resolvers.enqueue_download = Some(Arc::new(move |username, _, filename| {
    // the test may have stopped listening; the request is queued either way
    let _ = asked.send((username, filename));
    Box::pin(async { Ok(()) })
  }));
  requests
}

/// the next upload a peer asked to queue
pub async fn asked(requests: &mut Asked) -> (String, RemoteFilename) {
  tokio::time::timeout(PATIENCE, requests.recv())
    .await
    .expect("the peer asks for the file")
    .unwrap()
}

/// Every state the next transfer going `direction` reported, until it ended, and its last
/// snapshot
pub async fn states(
  events: &mut broadcast::Receiver<Event>,
  direction: TransferDirection,
) -> (Vec<TransferState>, Transfer) {
  let mut states = Vec::new();
  let mut token = None;
  tokio::time::timeout(PATIENCE * 3, async {
    loop {
      let Event::TransferStateChanged { transfer, .. } = events.recv().await.unwrap() else {
        continue;
      };
      if transfer.direction != direction || token.is_some_and(|token| token != transfer.token) {
        continue;
      }
      token = Some(transfer.token);
      states.push(transfer.state);
      if transfer.state.is_terminal() {
        return (states, transfer);
      }
    }
  })
  .await
  .expect("the transfer ends")
}

/// waits until a transfer's progress reaches `bytes`
pub async fn reached(events: &mut broadcast::Receiver<Event>, bytes: u64) {
  next_event(events, |event| match event {
    Event::TransferProgressUpdated { transfer, .. } if transfer.bytes_transferred >= bytes => {
      Some(())
    }
    _ => None,
  })
  .await;
}

/// the states a download goes through when it succeeds
pub fn download() -> Vec<TransferState> {
  vec![
    TransferState::QueuedLocally,
    TransferState::Requested,
    TransferState::QueuedRemotely,
    TransferState::Initializing,
    TransferState::InProgress,
    TransferState::Completed(TransferOutcome::Succeeded),
  ]
}

/// the states an upload goes through when it succeeds
pub fn upload() -> Vec<TransferState> {
  vec![
    TransferState::QueuedLocally,
    TransferState::Requested,
    TransferState::Initializing,
    TransferState::InProgress,
    TransferState::Completed(TransferOutcome::Succeeded),
  ]
}
