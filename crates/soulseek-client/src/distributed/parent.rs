//! Finding, keeping and losing a parent. Every candidate the server offers is connected to at
//! once, directly and through the server, and must then show it is a working part of the tree by
//! sending its branch level, its branch root and a search it passes down, within the negotiation
//! timeout; a candidate that sends a negative level or an impossible root is dropped. The tree
//! task adopts the one nearest the top of its branch, follows its branch, and looks again when it
//! goes. A search straight from the server makes us a branch root instead.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use soulseek_proto::frame::FrameKind;
use soulseek_proto::{DecodeError, DistributedMessage, FieldError, PeerConnectionType};
use tokio::task::JoinSet;
use tracing::Instrument;
use uuid::Uuid;

use super::deliver;
use super::tree::{Command, Parent, Tree, watch_closing};
use crate::client::Client;
use crate::conn::{Disconnect, MessageConn, ReadEvent};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::events::{DistributedParent, Event};
use crate::peers::{Via, both_failed, race};
use crate::state::ClientState;
use crate::waiter::{Scope, WaitKey};

/// What frames from a connection to a parent, or a would-be one, are for
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
  /// still showing it can be a parent; its frames complete the negotiation's waits
  Candidate = 0,
  /// our parent; its frames are searches to pass on and answer, and our branch
  Parent = 1,
}

/// What the read loop of an upstream connection and the tree task share about it
pub(super) struct Link {
  role: AtomicU8,
  /// a search with a bad identifier from this connection has been logged
  pub(super) bad_search_logged: AtomicBool,
  /// the parent's branch as it last sent it, waiting for the tree task
  pub(super) branch: Mutex<BranchNews>,
}

/// The parent's latest branch level and root that the tree task has not taken yet. At most one
/// command for them is queued at a time, however often the parent sends them.
#[derive(Default)]
pub(super) struct BranchNews {
  pub(super) level: Option<u32>,
  pub(super) root: Option<String>,
  /// a command telling the tree task is in its queue
  pub(super) queued: bool,
}

impl Link {
  fn candidate() -> Self {
    Self {
      role: AtomicU8::new(Role::Candidate as u8),
      bad_search_logged: AtomicBool::new(false),
      branch: Mutex::new(BranchNews::default()),
    }
  }

  pub(super) fn role(&self) -> Role {
    if self.role.load(Ordering::Acquire) == Role::Parent as u8 {
      Role::Parent
    } else {
      Role::Candidate
    }
  }

  pub(super) fn set_role(&self, role: Role) {
    self.role.store(role as u8, Ordering::Release);
  }
}

/// A candidate that answered as a parent must
pub(crate) struct Candidate {
  pub(super) username: String,
  pub(super) endpoint: SocketAddr,
  pub(super) conn: MessageConn,
  pub(super) link: Arc<Link>,
  pub(super) branch_level: u32,
  pub(super) branch_root: String,
}

impl Client {
  /// Negotiates with every candidate at once and hands the ones that answered to the tree task
  /// as round `round`
  pub(super) fn negotiate_parent_candidates(
    &self,
    candidates: Vec<(String, SocketAddr)>,
    round: u64,
  ) {
    let client = self.clone();
    tokio::spawn(
      async move {
        let mut negotiations = JoinSet::new();
        for (username, endpoint) in candidates {
          let client = client.clone();
          let span = tracing::debug_span!("parent_candidate", username, %endpoint);
          negotiations.spawn(
            async move { client.negotiate_candidate(username, endpoint).await }.instrument(span),
          );
        }
        let mut answered = Vec::new();
        while let Some(joined) = negotiations.join_next().await {
          match joined {
            Ok(Ok(candidate)) => answered.push(candidate),
            Ok(Err(error)) => tracing::debug!(%error, "parent candidate failed"),
            Err(error) => tracing::error!(%error, "parent candidate negotiation task failed"),
          }
        }
        let commands = client.distributed().commands.clone();
        drop(client);
        let negotiated = Command::Negotiated {
          round,
          candidates: answered,
        };
        deliver(commands, negotiated).await;
      }
      .in_current_span(),
    );
  }

  async fn negotiate_candidate(
    &self,
    username: String,
    endpoint: SocketAddr,
  ) -> Result<Candidate, ClientError> {
    let own = self.own_username("negotiate with a parent candidate")?;
    let options = self.options();
    let connection_options = options.distributed_connection_options.clone();
    let cancel = self.cancellation().child_token();
    let direct = self.connect_to_peer_socket(
      endpoint,
      &username,
      Via::Direct.kind(),
      connection_options.clone(),
      &cancel,
    );
    let indirect = self.solicit(
      &username,
      self.next_token(),
      PeerConnectionType::Distributed,
      connection_options.clone(),
      connection_options.connect_timeout,
      &cancel,
    );
    let (conn, via) = race(direct, indirect).await.map_err(|errors| {
      both_failed(
        format!(
          "Failed to establish a direct or indirect distributed connection to {username} ({endpoint})"
        ),
        errors,
      )
    })?;
    let conn = conn.into_message(FrameKind::Distributed, options.limits.frame_limits())?;
    let id = conn.handle().id();
    let timeout = options.limits.distributed_parent_negotiation_timeout;
    let level = self.waiter().wait::<u32>(
      WaitKey::BranchLevelMessage { connection: id },
      Some(timeout),
      cancel.clone(),
    );
    let search = self.waiter().wait::<()>(
      WaitKey::SearchRequestMessage { connection: id },
      Some(timeout),
      cancel.clone(),
    );
    // the root may arrive first or last; its own deadline starts once level and search are in
    let root = self.waiter().wait_indefinitely::<String>(
      WaitKey::BranchRootMessage { connection: id },
      cancel.clone(),
    );
    let link = Arc::new(Link::candidate());
    self.start_upstream_reading(&conn, &link);
    if let Some(introduction) = via.introduction(&own, PeerConnectionType::Distributed) {
      conn.write(introduction).await?;
    }
    let answered = async {
      let level = level.await?;
      search.await?;
      let root = if level > 0 {
        tokio::time::timeout(timeout, root)
          .await
          .map_err(|_| ClientError::Timeout(format!("no branch root within {timeout:?}")))??
      } else {
        // a root at level 0 is its own branch root; older clients do not say so
        match tokio::time::timeout(Duration::ZERO, root).await {
          Ok(Ok(root)) => root,
          _ => username.clone(),
        }
      };
      Ok::<_, ClientError>((level, root))
    }
    .await;
    match answered {
      Ok((branch_level, branch_root)) => {
        tracing::debug!(branch_level, branch_root, "parent candidate answered");
        Ok(Candidate {
          username,
          endpoint,
          conn,
          link,
          branch_level,
          branch_root,
        })
      }
      Err(error) => {
        conn.handle().disconnect(
          "One or more required messages was not received.",
          Some(error),
        );
        Err(ClientError::connection(
          ConnectionErrorKind::Other,
          format!(
            "Failed to retrieve branch info from parent candidate connection to {username} ({endpoint}); one or more required messages was not received."
          ),
        ))
      }
    }
  }

  /// Starts the read loop of a connection to a parent or a would-be one
  fn start_upstream_reading(&self, conn: &MessageConn, link: &Arc<Link>) {
    let client = self.downgrade();
    let reader = conn.clone();
    let link = link.clone();
    conn.start_reading(move |event| {
      if let ReadEvent::Frame(frame) = event
        && let Some(client) = client.upgrade()
      {
        match link.role() {
          Role::Candidate => client.candidate_frame(&reader, &frame),
          Role::Parent => client.parent_frame(&reader, &link, frame),
        }
      }
    });
  }

  /// A frame from a candidate completes the negotiation's waits; a negative level or an
  /// impossible root drops the candidate
  fn candidate_frame(&self, conn: &MessageConn, frame: &[u8]) {
    let id = conn.handle().id();
    let waiter = self.waiter();
    let completed = match DistributedMessage::decode(frame) {
      Ok(DistributedMessage::BranchLevel(level)) => {
        waiter.complete(&WaitKey::BranchLevelMessage { connection: id }, level.level)
      }
      Ok(DistributedMessage::BranchRoot(root)) => waiter.complete(
        &WaitKey::BranchRootMessage { connection: id },
        root.username,
      ),
      Ok(DistributedMessage::SearchRequest(_)) => {
        waiter.complete(&WaitKey::SearchRequestMessage { connection: id }, ())
      }
      Ok(DistributedMessage::EmbeddedMessage(embedded)) => {
        let inner = DistributedMessage::decode(&embedded.embedded.inner_frame());
        matches!(inner, Ok(DistributedMessage::SearchRequest(_)))
          && waiter.complete(&WaitKey::SearchRequestMessage { connection: id }, ())
      }
      Ok(DistributedMessage::Ping(_) | DistributedMessage::ChildDepth(_))
      | Err(DecodeError::UnknownCode { .. }) => false,
      Err(DecodeError::Field {
        error: FieldError::NegativeBranchLevel { .. },
        ..
      }) => return reject(self, conn, "Invalid branch level"),
      Err(DecodeError::Field {
        error: FieldError::InvalidUsername,
        ..
      }) => return reject(self, conn, "Invalid branch root"),
      // a search with another identifier proves nothing, and is not worth dropping the
      // candidate for
      Err(DecodeError::Field {
        error: FieldError::BadSearchIdentifier { .. },
        ..
      }) => false,
      Err(error) => return reject(self, conn, &error.to_string()),
    };
    if !completed {
      tracing::trace!("parent candidate frame not needed for the negotiation");
    }
  }
}

/// drops a candidate that broke the rules, failing its negotiation at once
fn reject(client: &Client, conn: &MessageConn, reason: &str) {
  conn.handle().disconnect(reason, None);
  client.waiter().fault_scope(
    &Scope::Connection(conn.handle().id()),
    &ClientError::connection(ConnectionErrorKind::Other, reason),
  );
}

impl Tree {
  /// Starts adopting a parent from the candidates, unless we have one, have none to try, or a
  /// round is under way. Without candidates the server is asked for new ones.
  pub(super) async fn add_parent_connection(&mut self) {
    let Some(client) = self.client() else {
      return;
    };
    if !client.options().enable_distributed_network {
      tracing::debug!("Parent connection solicitation ignored; distributed network is not enabled");
      return;
    }
    if client.state() != ClientState::READY {
      return;
    }
    if self.has_parent() {
      tracing::debug!("parent candidates ignored; a parent is connected");
      self.update_status(false).await;
      return;
    }
    if self.candidates.is_empty() {
      tracing::debug!(
        "Parent candidate cache is empty; requesting a new list of candidates from the server"
      );
      self.update_status(true).await;
      return;
    }
    if self.acquiring.is_some() {
      tracing::debug!(
        "parent candidates ignored; already in the process of establishing a connection"
      );
      return;
    }
    tracing::info!(
      candidates = self.candidates.len(),
      "Attempting to establish a new parent connection"
    );
    self.rounds += 1;
    self.acquiring = Some(self.rounds);
    client.negotiate_parent_candidates(self.candidates.clone(), self.rounds);
  }

  /// Adopts the candidate nearest the top of its branch from a finished round and lets the
  /// others go
  pub(super) async fn negotiated(&mut self, round: u64, mut candidates: Vec<Candidate>) {
    if self.acquiring != Some(round) {
      for candidate in candidates {
        candidate.conn.handle().disconnect("Not selected.", None);
      }
      return;
    }
    self.acquiring = None;
    candidates.retain(|candidate| candidate.conn.handle().is_open());
    candidates.sort_by_key(|candidate| candidate.branch_level);
    let mut candidates = candidates.into_iter();
    match candidates.next() {
      Some(chosen) => self.adopt(chosen).await,
      None => tracing::warn!("Failed to connect to any of the available parent candidates"),
    }
    let rest: Vec<Candidate> = candidates.collect();
    if self.has_parent() {
      self.candidates = rest
        .iter()
        .map(|candidate| (candidate.username.clone(), candidate.endpoint))
        .collect();
    }
    for candidate in rest {
      candidate.conn.handle().disconnect("Not selected.", None);
    }
    self.update_status(false).await;
  }

  pub(super) async fn adopt(&mut self, chosen: Candidate) {
    let Some(client) = self.client() else {
      return;
    };
    chosen.link.set_role(Role::Parent);
    watch_closing(&client, &chosen.conn, {
      let connection = chosen.conn.handle().id();
      move |reason| Command::ParentLost { connection, reason }
    });
    let adopted = DistributedParent {
      username: chosen.username.clone(),
      ip_endpoint: chosen.endpoint,
      branch_level: chosen.branch_level,
      branch_root: chosen.branch_root.clone(),
      is_branch_root: chosen.username == chosen.branch_root && chosen.branch_level == 0,
    };
    tracing::info!(
      username = adopted.username,
      endpoint = %adopted.ip_endpoint,
      branch_level = adopted.branch_level,
      branch_root = adopted.branch_root,
      "Adopted parent connection"
    );
    self.parent = Some(Parent { candidate: chosen });
    self.demote();
    client.emit(Event::DistributedParentAdopted(adopted));
    self.publish(false);
    self.update_status(false).await;
    self.send_branch_information(&client);
  }

  pub(super) async fn parent_lost(&mut self, connection: Uuid, reason: &Disconnect) {
    if self.parent_with(connection).is_none() {
      return;
    }
    let Some(parent) = self.parent.take() else {
      return;
    };
    let Some(client) = self.client() else {
      return;
    };
    let parent = parent.candidate;
    tracing::info!(
      username = parent.username,
      endpoint = %parent.endpoint,
      reason = reason.message,
      "Parent connection disconnected"
    );
    client.emit(Event::DistributedParentDisconnected(DistributedParent {
      username: parent.username.clone(),
      ip_endpoint: parent.endpoint,
      branch_level: parent.branch_level,
      branch_root: parent.branch_root.clone(),
      is_branch_root: parent.username == parent.branch_root && parent.branch_level == 0,
    }));
    self.publish(false);
    // children would otherwise keep reporting the branch we just left
    self.send_branch_information(&client);
    self.add_parent_connection().await;
  }

  /// Takes the branch news the parent's link holds, which also lets the next news be queued,
  /// and follows what changed
  pub(super) async fn parent_branch_changed(&mut self, connection: Uuid) {
    let Some(parent) = self.parent_with(connection) else {
      return;
    };
    let news = std::mem::take(&mut *crate::lock(&parent.candidate.link.branch));
    let mut changed = false;
    if let Some(level) = news.level
      && level != parent.candidate.branch_level
    {
      parent.candidate.branch_level = level;
      changed = true;
    }
    if let Some(root) = news.root
      && root != parent.candidate.branch_root
    {
      parent.candidate.branch_root = root;
      changed = true;
    }
    if changed {
      self.branch_changed().await;
    }
  }

  /// our parent's level or root changed, so ours did: the children and the server hear it
  pub(super) async fn branch_changed(&mut self) {
    if let Some(client) = self.client() {
      self.send_branch_information(&client);
    }
    self.update_status_eventually().await;
  }

  pub(super) async fn promote(&mut self) {
    if self.is_branch_root || self.has_parent() {
      return;
    }
    self.is_branch_root = true;
    tracing::info!("Promoted to distributed branch root.");
    if let Some(client) = self.client() {
      client.emit(Event::PromotedToDistributedBranchRoot);
    }
    self.publish(false);
    // as a root we take children, which the server should hear
    self.update_status_eventually().await;
  }

  pub(super) fn demote(&mut self) {
    if !self.is_branch_root {
      return;
    }
    self.is_branch_root = false;
    tracing::info!("Demoted from distributed branch root.");
    if let Some(client) = self.client() {
      client.emit(Event::DemotedFromDistributedBranchRoot);
    }
    self.publish(false);
  }
}
