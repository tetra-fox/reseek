//! The distributed search tree: a parent that passes searches down to us, children we pass them
//! on to, and what the server needs to know about our place in it. One task owns the tree and
//! takes commands from everything else; passing a search on to the children reads a shared list
//! of them instead, so the busiest path never waits for that task.

mod children;
mod dispatch;
mod parent;
mod status;
mod tree;

pub(crate) use tree::Command;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot, watch};

use crate::client::{Client, WeakClient};
use crate::conn::MessageConn;
use crate::log::LogLimiter;
use crate::state::DistributedNetworkInfo;

/// commands waiting for the tree task
const COMMANDS: usize = 1024;

static COMMANDS_FULL: LogLimiter = LogLimiter::new();

/// What the rest of the client shares with the tree task
pub(crate) struct DistributedNetwork {
  commands: mpsc::Sender<Command>,
  /// the children's connections, for passing searches down
  children: Mutex<Arc<[MessageConn]>>,
  /// the snapshot the tree task last published
  info: watch::Sender<DistributedNetworkInfo>,
  /// the tree task's own flag, mirrored so a search from the server promotes us only once
  is_branch_root: AtomicBool,
  /// moving average, in milliseconds, of how long passing a search to every child takes
  broadcast_latency: Mutex<Option<f64>>,
  /// the last search frame from the parent, to drop one sent twice in a row
  last_search: Mutex<Option<Vec<u8>>>,
  /// searches from parents dropped for a bad identifier
  invalid_search_requests: AtomicU64,
}

impl DistributedNetwork {
  /// the shared state, and the receiving end of the commands for [`DistributedNetwork::run`]
  pub(crate) fn new() -> (Self, mpsc::Receiver<Command>) {
    let (commands, received) = mpsc::channel(COMMANDS);
    let network = Self {
      commands,
      children: Mutex::new(Arc::from(Vec::new())),
      info: watch::Sender::new(DistributedNetworkInfo {
        average_broadcast_latency: None,
        branch_level: 0,
        branch_root: String::new(),
        is_branch_root: false,
        child_limit: 0,
        can_accept_children: false,
        children: Vec::new(),
        parent: (String::new(), None),
        has_parent: false,
      }),
      is_branch_root: AtomicBool::new(false),
      broadcast_latency: Mutex::new(None),
      last_search: Mutex::new(None),
      invalid_search_requests: AtomicU64::new(0),
    };
    (network, received)
  }

  /// Starts the tree task, which runs until the client is disposed or dropped.
  pub(crate) fn run(client: WeakClient, commands: mpsc::Receiver<Command>) {
    tree::spawn(client, commands);
  }

  fn children(&self) -> Arc<[MessageConn]> {
    crate::lock(&self.children).clone()
  }

  fn set_children(&self, children: Vec<MessageConn>) {
    *crate::lock(&self.children) = Arc::from(children);
  }

  fn latency(&self) -> Option<f64> {
    *crate::lock(&self.broadcast_latency)
  }

  /// folds one broadcast's wall time into the moving average
  fn record_latency(&self, millis: f64) {
    let mut average = crate::lock(&self.broadcast_latency);
    *average = Some(match *average {
      None => millis,
      Some(average) => average + (millis - average) * 0.005,
    });
  }

  fn is_branch_root(&self) -> bool {
    self.is_branch_root.load(Ordering::Relaxed)
  }
}

/// Hands `command` to the tree task, waiting while its queue is full; for a closed parent or
/// child, a child connection made and a finished acquisition round, which nothing sends again.
/// The caller lets go of the client first, so a wait never keeps it alive.
async fn deliver(commands: mpsc::Sender<Command>, command: Command) {
  let name = command.name();
  let command = match commands.try_send(command) {
    Ok(()) => return,
    Err(mpsc::error::TrySendError::Full(command)) => command,
    Err(mpsc::error::TrySendError::Closed(_)) => {
      return tracing::debug!(name, "distributed network stopped, command dropped");
    }
  };
  tracing::debug!(name, "distributed network command queue is full, waiting");
  // the tree task stops only with the client
  if commands.send(command).await.is_err() {
    tracing::debug!(name, "distributed network stopped, command dropped");
  }
}

impl Client {
  /// our place in the distributed search tree
  pub fn distributed_network(&self) -> DistributedNetworkInfo {
    let network = self.distributed();
    let mut info = network.info.borrow().clone();
    info.average_broadcast_latency = network.latency();
    info
  }

  /// Hands `command` to the tree task without waiting, for read loops and dispatchers, which
  /// must not block. A full queue, which a tree task held up on a server write causes, drops it;
  /// a dropped one leaves the tree out of date until later news repeats it. Commands whose loss
  /// would stick go through [`deliver`] instead. Whether it was queued.
  pub(crate) fn distributed_command(&self, command: Command) -> bool {
    let name = command.name();
    match self.distributed().commands.try_send(command) {
      Ok(()) => true,
      Err(mpsc::error::TrySendError::Full(_)) => {
        if let Some(suppressed) = COMMANDS_FULL.allow() {
          tracing::warn!(
            name,
            suppressed,
            "distributed network command queue is full, command dropped"
          );
        }
        false
      }
      // the tree task stops only with the client
      Err(mpsc::error::TrySendError::Closed(_)) => {
        tracing::debug!(name, "distributed network stopped, command dropped");
        false
      }
    }
  }

  /// hands a command carrying `done` to the tree task and waits until it has been carried out
  async fn distributed_call(&self, command: impl FnOnce(oneshot::Sender<()>) -> Command) {
    let (done, finished) = oneshot::channel();
    if self
      .distributed()
      .commands
      .send(command(done))
      .await
      .is_ok()
    {
      // the tree task stops only with the client, which drops the sender unanswered
      let _ = finished.await;
    }
  }

  /// Tells the server our place in the tree when it changed since it was last told, or always
  /// when `force` is set, and waits until it has been written or failed.
  pub(crate) async fn update_distributed_status(&self, force: bool) {
    self
      .distributed_call(|done| Command::UpdateStatus {
        force,
        done: Some(done),
      })
      .await;
  }

  /// Closes the parent and every child with an event for each, forgets the status last sent and
  /// leaves the branch root; for when the server session ends.
  pub(crate) async fn tear_down_distributed(&self) {
    self.distributed_call(Command::TearDown).await;
  }
}
