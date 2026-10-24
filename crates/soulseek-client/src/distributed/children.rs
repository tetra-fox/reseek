//! Children below us: one that connects directly, one we connect to because it asked the
//! server, the slot each takes under the child limit (reserved before the connection is made, so
//! children arriving together never overshoot it), the branch information each hears, and one
//! that leaves.

use std::net::SocketAddr;

use soulseek_proto::distributed::{BranchLevel, BranchRoot};
use soulseek_proto::frame::FrameKind;
use soulseek_proto::init::PierceFirewall;
use soulseek_proto::server::ConnectToPeerResponse;
use soulseek_proto::{DistributedMessage, InitMessage};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use super::tree::{Child, Command, Reservation, Tree, watch_closing};
use crate::client::Client;
use crate::conn::{Conn, ConnKind, Disconnect, MessageConn};
use crate::error::ClientError;
use crate::events::Event;
use crate::peers::request_endpoint;

impl Tree {
  /// our branch level and root, as the two frames a child is sent
  pub(super) fn branch_information(&self, client: &Client) -> Vec<u8> {
    let mut frames = DistributedMessage::BranchLevel(BranchLevel {
      level: self.branch_level(),
    })
    .encode();
    frames.extend(
      DistributedMessage::BranchRoot(BranchRoot {
        username: self.branch_root(client),
      })
      .encode(),
    );
    frames
  }

  /// tells every child our branch, after it changed
  pub(super) fn send_branch_information(&self, client: &Client) {
    let frames = self.branch_information(client);
    for (username, child) in &self.children {
      if let Err(error) = child.conn.send(frames.clone()) {
        tracing::debug!(username, %error, "branch information not sent to child");
      }
    }
  }

  /// A peer connected to us directly to become a child. It takes a slot if one is free, and
  /// replaces a child of the same name.
  pub(super) async fn child_connecting(&mut self, username: String, provisional: Conn) {
    let Some(client) = self.client() else {
      return;
    };
    if !self.has_room_for(&client, &username) {
      tracing::debug!(
        username,
        enabled = client.options().enable_distributed_network,
        has_parent = self.has_parent(),
        is_branch_root = self.is_branch_root,
        children = self.children.len(),
        limit = self.child_limit(&client),
        "child connection refused"
      );
      provisional
        .handle()
        .disconnect("Not accepting children", None);
      self.update_status(false).await;
      return;
    }
    let remote = provisional.handle().remote();
    let conn = provisional
      .handoff()
      .and_then(|stream| {
        Conn::from_handoff(
          stream,
          Some(username.clone()),
          ConnKind::INBOUND | ConnKind::DIRECT,
          client.options().distributed_connection_options.clone(),
        )
      })
      .and_then(|conn| {
        conn.into_message(
          FrameKind::Distributed,
          client.options().limits.frame_limits(),
        )
      });
    let conn = match conn {
      Ok(conn) => conn,
      Err(error) => {
        return tracing::debug!(
          username,
          %remote,
          %error,
          "Failed to establish an inbound direct child connection"
        );
      }
    };
    if let Some(reserved) = self.reserved.remove(&username) {
      reserved.cancel.cancel();
    }
    let superseded = self.children.remove(&username);
    if let Some(old) = &superseded {
      old.conn.handle().disconnect("Superseded.", None);
    }
    self.add_child(&client, username, conn, remote, superseded.is_none());
    self.update_status_eventually().await;
  }

  /// A peer asked the server to have us connect to it as its parent. We do if a slot is free,
  /// keeping the slot while the connection is made; otherwise the server hears we cannot.
  pub(super) async fn child_requested(&mut self, request: ConnectToPeerResponse) {
    let Some(client) = self.client() else {
      return;
    };
    let username = request.username.clone();
    if self.children.contains_key(&username) || self.reserved.contains_key(&username) {
      return tracing::debug!(
        username,
        "child connection request ignored; connection already exists"
      );
    }
    let endpoint = request_endpoint(request.ip, request.port);
    let (Some(endpoint), true) = (endpoint, self.has_room_for(&client, &username)) else {
      tracing::debug!(username, "child connection request refused");
      client.send_cannot_connect(request.token, &username);
      return;
    };
    self.attempts += 1;
    let attempt = self.attempts;
    let cancel = client.cancellation().child_token();
    self.reserved.insert(
      username.clone(),
      Reservation {
        attempt,
        cancel: cancel.clone(),
      },
    );
    let token = request.token;
    let span = tracing::debug_span!("child_connection", username, %endpoint, token);
    tokio::spawn(
      async move {
        let result = client
          .pierce_for_child(&username, endpoint, token, &cancel)
          .await;
        client.distributed_command(Command::ChildConnected {
          username,
          attempt,
          token,
          result,
        });
      }
      .instrument(span),
    );
  }

  /// The connection to a peer that asked for us as its parent is made, or failed. It becomes
  /// a child if its slot is still its own; a failure is reported to the server.
  pub(super) async fn child_connected(
    &mut self,
    username: String,
    attempt: u64,
    token: u32,
    result: Result<MessageConn, ClientError>,
  ) {
    let Some(client) = self.client() else {
      return;
    };
    let ours = self
      .reserved
      .get(&username)
      .is_some_and(|reserved| reserved.attempt == attempt);
    if ours {
      self.reserved.remove(&username);
    }
    match result {
      Ok(conn) if ours => {
        let remote = conn.handle().remote();
        self.add_child(&client, username, conn, remote, true);
        self.update_status_eventually().await;
      }
      // a direct connection from the same user, or a teardown, took the slot
      Ok(conn) => {
        conn.handle().disconnect("Superseded.", None);
      }
      Err(ClientError::Cancelled) => {}
      Err(error) => {
        tracing::debug!(username, %error, "Failed to establish an inbound indirect child connection");
        client.send_cannot_connect(token, &username);
      }
    }
  }

  /// Records an established child: it reads from now on, hears our branch, and leaves the tree
  /// when its connection closes
  pub(super) fn add_child(
    &mut self,
    client: &Client,
    username: String,
    conn: MessageConn,
    endpoint: SocketAddr,
    announce: bool,
  ) {
    client.start_child_reading(&conn);
    if let Err(error) = conn.send(self.branch_information(client)) {
      tracing::debug!(username, %error, "branch information not sent to the new child");
    }
    watch_closing(client, &conn, {
      let username = username.clone();
      let connection = conn.handle().id();
      move |reason| Command::ChildLost {
        username,
        connection,
        reason,
      }
    });
    tracing::info!(username, %endpoint, "Added/Updated child connection");
    self
      .children
      .insert(username.clone(), Child { conn, endpoint });
    self.publish_children(client);
    if announce {
      client.emit(Event::DistributedChildAdded {
        username,
        ip_endpoint: endpoint,
      });
      self.publish(false);
    }
  }

  /// A child's connection closed. Only the connection the tree holds for that name counts, so
  /// a replaced child closing leaves its replacement in place.
  pub(super) async fn child_lost(&mut self, username: &str, connection: Uuid, reason: &Disconnect) {
    let current = self
      .children
      .get(username)
      .is_some_and(|child| child.conn.handle().id() == connection);
    if !current {
      return;
    }
    let Some(child) = self.children.remove(username) else {
      return;
    };
    let Some(client) = self.client() else {
      return;
    };
    tracing::info!(
      username,
      endpoint = %child.endpoint,
      reason = reason.message,
      "Child connection disconnected"
    );
    self.publish_children(&client);
    client.emit(Event::DistributedChildDisconnected {
      username: username.to_owned(),
      ip_endpoint: child.endpoint,
    });
    self.publish(false);
    self.update_status_eventually().await;
  }
}

impl Client {
  /// dials a peer that asked the server for us as its parent and answers with its token
  async fn pierce_for_child(
    &self,
    username: &str,
    endpoint: SocketAddr,
    token: u32,
    cancel: &CancellationToken,
  ) -> Result<MessageConn, ClientError> {
    let mut conn = self
      .connect_to_peer_socket(
        endpoint,
        username,
        ConnKind::INBOUND | ConnKind::INDIRECT,
        self.options().distributed_connection_options.clone(),
        cancel,
      )
      .await?;
    conn
      .write_all(&InitMessage::PierceFirewall(PierceFirewall { token }).encode())
      .await?;
    conn.into_message(FrameKind::Distributed, self.options().limits.frame_limits())
  }
}
