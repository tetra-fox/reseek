//! The task that owns the distributed tree: its state, the commands it takes, what it publishes
//! about our place in the tree, and tearing it all down when the server session ends. Finding a
//! parent, taking children and telling the server where we stand each live in a file of their
//! own.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;

use soulseek_proto::server::{ConnectToPeerResponse, ParentCandidate};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep_until};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use super::parent::Candidate;
use super::status::WATCHDOG;
use crate::client::{Client, WeakClient};
use crate::conn::{Conn, Disconnect, MessageConn};
use crate::error::ClientError;
use crate::events::{DistributedParent, Event};
use crate::peers::request_endpoint;
use crate::state::DistributedNetworkInfo;

/// What the rest of the client asks of the tree task
pub(crate) enum Command {
  /// the server offers parent candidates
  NetInfo(Vec<ParentCandidate>),
  /// the candidates of an acquisition round that answered as a parent must
  Negotiated {
    round: u64,
    candidates: Vec<Candidate>,
  },
  ParentLost {
    connection: Uuid,
    reason: Disconnect,
  },
  ParentBranchLevel {
    connection: Uuid,
    level: u32,
  },
  ParentBranchRoot {
    connection: Uuid,
    root: String,
  },
  /// a peer connected to us directly to become a child
  ChildConnecting {
    username: String,
    conn: Conn,
  },
  /// a peer asked the server to have us connect to it as its parent
  ChildRequested(ConnectToPeerResponse),
  /// the connection to a peer that asked to be our child is made, or failed
  ChildConnected {
    username: String,
    attempt: u64,
    token: u32,
    result: Result<MessageConn, ClientError>,
  },
  ChildLost {
    username: String,
    connection: Uuid,
    reason: Disconnect,
  },
  /// the server sent us a search directly, which it does only to branch roots
  Promote,
  /// an option, a server tuning value or our upload speed changed
  Changed,
  /// our average upload speed, as the server reports it
  OwnSpeed(u32),
  UpdateStatus {
    force: bool,
    done: Option<oneshot::Sender<()>>,
  },
  /// the server told every client to rebuild the tree
  Reset,
  /// the server session ended
  TearDown(oneshot::Sender<()>),
}

impl Command {
  pub(crate) fn name(&self) -> &'static str {
    match self {
      Self::NetInfo(_) => "NetInfo",
      Self::Negotiated { .. } => "Negotiated",
      Self::ParentLost { .. } => "ParentLost",
      Self::ParentBranchLevel { .. } => "ParentBranchLevel",
      Self::ParentBranchRoot { .. } => "ParentBranchRoot",
      Self::ChildConnecting { .. } => "ChildConnecting",
      Self::ChildRequested(_) => "ChildRequested",
      Self::ChildConnected { .. } => "ChildConnected",
      Self::ChildLost { .. } => "ChildLost",
      Self::Promote => "Promote",
      Self::Changed => "Changed",
      Self::OwnSpeed(_) => "OwnSpeed",
      Self::UpdateStatus { .. } => "UpdateStatus",
      Self::Reset => "Reset",
      Self::TearDown(_) => "TearDown",
    }
  }
}

/// The parent we adopted
pub(super) struct Parent {
  pub(super) candidate: Candidate,
}

/// An established child
pub(super) struct Child {
  pub(super) conn: MessageConn,
  pub(super) endpoint: SocketAddr,
}

/// a child connection being made because the peer asked the server for us
pub(super) struct Reservation {
  pub(super) attempt: u64,
  pub(super) cancel: CancellationToken,
}

/// Everything the tree task owns
pub(super) struct Tree {
  pub(super) client: WeakClient,
  pub(super) parent: Option<Parent>,
  pub(super) is_branch_root: bool,
  pub(super) children: HashMap<String, Child>,
  pub(super) reserved: HashMap<String, Reservation>,
  /// the last candidates offered, or those left over from the last adoption
  pub(super) candidates: Vec<(String, SocketAddr)>,
  /// the acquisition round under way
  pub(super) acquiring: Option<u64>,
  pub(super) rounds: u64,
  pub(super) attempts: u64,
  pub(super) own_speed: Option<u32>,
  pub(super) last_status: Option<String>,
  pub(super) last_status_at: Option<Instant>,
  pub(super) debounce: Option<Instant>,
  /// what was last published, latency aside
  pub(super) published: Option<DistributedNetworkInfo>,
}

pub(super) fn spawn(client: WeakClient, mut commands: mpsc::Receiver<Command>) {
  let Some(lifetime) = client.upgrade().map(|client| client.cancellation().clone()) else {
    return;
  };
  let span = tracing::info_span!("distributed");
  tokio::spawn(
    async move {
      let mut tree = Tree {
        client,
        parent: None,
        is_branch_root: false,
        children: HashMap::new(),
        reserved: HashMap::new(),
        candidates: Vec::new(),
        acquiring: None,
        rounds: 0,
        attempts: 0,
        own_speed: None,
        last_status: None,
        last_status_at: None,
        debounce: None,
        published: None,
      };
      let mut watchdog = interval_at(Instant::now() + WATCHDOG, WATCHDOG);
      watchdog.set_missed_tick_behavior(MissedTickBehavior::Delay);
      loop {
        let debounce = tree.debounce;
        tokio::select! {
          () = lifetime.cancelled() => break,
          command = commands.recv() => match command {
            Some(command) => tree.handle(command).await,
            None => break,
          },
          () = sleep_until(debounce.unwrap_or_else(Instant::now)), if debounce.is_some() => {
            tree.debounce = None;
            tree.update_status(false).await;
          }
          _ = watchdog.tick() => tree.watchdog().await,
        }
        tree.publish(false);
      }
    }
    .instrument(span),
  );
}

impl Tree {
  pub(super) fn client(&self) -> Option<Client> {
    self.client.upgrade()
  }

  async fn handle(&mut self, command: Command) {
    match command {
      Command::NetInfo(candidates) => {
        self.candidates = candidates
          .into_iter()
          .filter_map(|candidate| {
            let Some(endpoint) = request_endpoint(candidate.ip, candidate.port) else {
              tracing::debug!(
                username = candidate.username,
                port = candidate.port,
                "parent candidate with an invalid port dropped"
              );
              return None;
            };
            Some((candidate.username, endpoint))
          })
          .collect();
        self.add_parent_connection().await;
      }
      Command::Negotiated { round, candidates } => self.negotiated(round, candidates).await,
      Command::ParentLost { connection, reason } => self.parent_lost(connection, &reason).await,
      Command::ParentBranchLevel { connection, level } => {
        if let Some(parent) = self.parent_with(connection)
          && parent.candidate.branch_level != level
        {
          parent.candidate.branch_level = level;
          self.branch_changed().await;
        }
      }
      Command::ParentBranchRoot { connection, root } => {
        if let Some(parent) = self.parent_with(connection)
          && parent.candidate.branch_root != root
        {
          parent.candidate.branch_root = root;
          self.branch_changed().await;
        }
      }
      Command::ChildConnecting { username, conn } => self.child_connecting(username, conn).await,
      Command::ChildRequested(request) => self.child_requested(request).await,
      Command::ChildConnected {
        username,
        attempt,
        token,
        result,
      } => self.child_connected(username, attempt, token, result).await,
      Command::ChildLost {
        username,
        connection,
        reason,
      } => self.child_lost(&username, connection, &reason).await,
      Command::Promote => self.promote().await,
      Command::Changed => self.update_status_eventually().await,
      Command::OwnSpeed(speed) => {
        if self.own_speed != Some(speed) {
          self.own_speed = Some(speed);
          self.update_status_eventually().await;
        }
      }
      Command::UpdateStatus { force, done } => {
        self.update_status(force).await;
        if let Some(done) = done {
          // the caller may have stopped waiting
          let _ = done.send(());
        }
      }
      Command::Reset => {
        self.tear_down("Distributed network reset");
        self.reset_status();
        self.update_status(true).await;
      }
      Command::TearDown(done) => {
        self.tear_down("Server connection closed");
        self.reset_status();
        // the caller may have stopped waiting
        let _ = done.send(());
      }
    }
  }

  pub(super) fn parent_with(&mut self, connection: Uuid) -> Option<&mut Parent> {
    self
      .parent
      .as_mut()
      .filter(|parent| parent.candidate.conn.handle().id() == connection)
  }

  pub(super) fn has_parent(&self) -> bool {
    self
      .parent
      .as_ref()
      .is_some_and(|parent| parent.candidate.conn.handle().is_open())
  }

  /// our depth: our parent's plus one, or 0 without a parent
  pub(super) fn branch_level(&self) -> u32 {
    self
      .parent
      .as_ref()
      .map_or(0, |parent| parent.candidate.branch_level.saturating_add(1))
  }

  /// our parent's branch root, or our own name without a parent
  pub(super) fn branch_root(&self, client: &Client) -> String {
    match &self.parent {
      Some(parent) => parent.candidate.branch_root.clone(),
      None => client.username().unwrap_or_default(),
    }
  }

  /// the configured child limit, lowered to what our upload speed earns once the server has
  /// told us the ratio and our speed
  pub(super) fn child_limit(&self, client: &Client) -> u32 {
    let configured = client.options().distributed_child_limit;
    match (self.own_speed, client.server_info().parent_speed_ratio) {
      (Some(speed), Some(ratio)) if ratio > 0 => configured.min(speed / ratio),
      _ => configured,
    }
  }

  /// our upload speed is known to be below what the server wants of a parent
  pub(super) fn too_slow(&self, client: &Client) -> bool {
    matches!(
      (self.own_speed, client.server_info().parent_min_speed),
      (Some(speed), Some(minimum)) if speed < minimum
    )
  }

  /// whether the tree may grow below us at all, before counting the children
  pub(super) fn takes_children(&self, client: &Client) -> bool {
    let options = client.options();
    options.enable_distributed_network
      && options.accept_distributed_children
      && (self.has_parent() || self.is_branch_root)
      && !self.too_slow(client)
  }

  pub(super) fn can_accept_children(&self, client: &Client) -> bool {
    self.takes_children(client) && self.children.len() < self.child_limit(client) as usize
  }

  /// Whether a slot is free for `username`, counting the children being connected; a child
  /// that replaces one under the same name needs no new slot
  pub(super) fn has_room_for(&self, client: &Client, username: &str) -> bool {
    let taken = self.children.len() + self.reserved.len()
      - usize::from(self.children.contains_key(username))
      - usize::from(self.reserved.contains_key(username));
    self.takes_children(client) && taken < self.child_limit(client) as usize
  }

  pub(super) fn info(&self, client: &Client) -> DistributedNetworkInfo {
    let has_parent = self.has_parent();
    DistributedNetworkInfo {
      average_broadcast_latency: None,
      branch_level: self.branch_level(),
      branch_root: self.branch_root(client),
      is_branch_root: self.is_branch_root,
      child_limit: self.child_limit(client),
      can_accept_children: self.can_accept_children(client),
      children: self
        .children
        .iter()
        .map(|(username, child)| (username.clone(), child.endpoint))
        .collect(),
      parent: match &self.parent {
        Some(parent) => (
          parent.candidate.username.clone(),
          Some(parent.candidate.endpoint),
        ),
        None => (String::new(), None),
      },
      has_parent,
    }
  }

  /// publishes the snapshot and raises the event when it changed, or always when `force` is set
  pub(super) fn publish(&mut self, force: bool) {
    let Some(client) = self.client() else {
      return;
    };
    let mut info = self.info(&client);
    if !force && self.published.as_ref() == Some(&info) {
      return;
    }
    self.published = Some(info.clone());
    let network = client.distributed();
    network.info.send_replace(info.clone());
    network
      .is_branch_root
      .store(self.is_branch_root, Ordering::Relaxed);
    info.average_broadcast_latency = network.latency();
    client.emit(Event::DistributedNetworkStateChanged(info));
  }

  pub(super) fn publish_children(&self, client: &Client) {
    client.distributed().set_children(
      self
        .children
        .values()
        .map(|child| child.conn.clone())
        .collect(),
    );
  }

  /// Drops the parent and every child, raising an event for each, and abandons any round and
  /// child connection under way. Entries leave the tree before their connections close, so
  /// the closing is not handled a second time.
  pub(super) fn tear_down(&mut self, reason: &str) {
    self.acquiring = None;
    for (_, reserved) in self.reserved.drain() {
      reserved.cancel.cancel();
    }
    let Some(client) = self.client() else {
      return;
    };
    if let Some(parent) = self.parent.take() {
      let parent = parent.candidate;
      parent.conn.handle().disconnect(reason, None);
      client.emit(Event::DistributedParentDisconnected(DistributedParent {
        username: parent.username.clone(),
        ip_endpoint: parent.endpoint,
        branch_level: parent.branch_level,
        branch_root: parent.branch_root.clone(),
        is_branch_root: parent.username == parent.branch_root && parent.branch_level == 0,
      }));
    }
    for (username, child) in self.children.drain() {
      child.conn.handle().disconnect(reason, None);
      client.emit(Event::DistributedChildDisconnected {
        username,
        ip_endpoint: child.endpoint,
      });
    }
    self.publish_children(&client);
    self.publish(false);
  }
}

/// Hands the tree task the command `closed` makes when `conn` closes
pub(super) fn watch_closing(
  client: &Client,
  conn: &MessageConn,
  closed: impl FnOnce(Disconnect) -> Command + Send + 'static,
) {
  let client = client.downgrade();
  let handle = conn.handle().clone();
  tokio::spawn(
    async move {
      let reason = handle.disconnected().await;
      if let Some(client) = client.upgrade() {
        client.distributed_command(closed(reason));
      }
    }
    .instrument(conn.handle().span().clone()),
  );
}
