//! What the client does with distributed frames. A search from the parent is passed on to every
//! child as it arrived and answered unless it is our own; a search the server sends us directly
//! makes us a branch root and is passed on unwrapped; the parent's branch level and root are
//! followed; a child's ping is answered with an empty one.

use std::sync::atomic::Ordering;

use soulseek_proto::distributed::{Embedded, Ping};
use soulseek_proto::{DecodeError, DistributedCode, DistributedMessage, SEARCH_IDENTIFIER};
use tokio::time::Instant;
use tracing::Instrument;

use super::parent::Link;
use super::tree::Command;
use crate::client::Client;
use crate::conn::{MessageConn, ReadEvent};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::waiter::WaitKey;

impl Client {
  /// A frame from our parent
  pub(super) fn parent_frame(&self, conn: &MessageConn, link: &Link, frame: Vec<u8>) {
    let code = frame.get(4).copied().map(u32::from);
    let search_like = [
      DistributedCode::Ping,
      DistributedCode::SearchRequest,
      DistributedCode::EmbeddedMessage,
    ]
    .iter()
    .any(|known| Some(known.code()) == code);
    if search_like && self.options().deduplicate_search_requests && self.repeated(&frame) {
      return;
    }
    if code == Some(DistributedCode::SearchRequest.code()) {
      return self.search_from_parent(conn, link, &frame);
    }
    let username = conn.handle().username().unwrap_or_default().to_owned();
    match DistributedMessage::decode(&frame) {
      Ok(DistributedMessage::EmbeddedMessage(wrapped)) => {
        // an old parent forwards the server's wrapper instead of the search inside it
        let embedded = wrapped.embedded;
        if embedded.distributed_code != DistributedCode::SearchRequest.code() as u8 {
          return tracing::debug!(
            code = embedded.distributed_code,
            "unhandled embedded message from the parent"
          );
        }
        self.search_from_parent(conn, link, &embedded.inner_frame());
      }
      // handled above, before decoding
      Ok(DistributedMessage::SearchRequest(_)) => {}
      Ok(DistributedMessage::Ping(ping)) => {
        let key = WaitKey::Distributed {
          code: DistributedCode::Ping,
          username,
        };
        if !self.waiter().complete(&key, ping.token) {
          tracing::trace!("ping from the parent nobody waits for");
        }
      }
      Ok(DistributedMessage::BranchLevel(level)) => {
        self.distributed_command(Command::ParentBranchLevel {
          connection: conn.handle().id(),
          level: level.level,
        });
      }
      Ok(DistributedMessage::BranchRoot(root)) => {
        self.distributed_command(Command::ParentBranchRoot {
          connection: conn.handle().id(),
          root: root.username,
        });
      }
      Ok(DistributedMessage::ChildDepth(depth)) => {
        let key = WaitKey::ChildDepthMessage {
          username,
          remote: conn.handle().remote(),
        };
        if !self.waiter().complete(&key, depth.depth) {
          tracing::trace!("child depth nobody waits for");
        }
      }
      Err(DecodeError::UnknownCode { code }) => {
        tracing::debug!(code, "unhandled distributed message from the parent");
      }
      // the decoder has logged it
      Err(_) => {}
    }
  }

  /// Whether `frame` is the same as the search-like frame the parent sent just before it; the
  /// one slot is compared and replaced in one step
  fn repeated(&self, frame: &[u8]) -> bool {
    let mut last = crate::lock(&self.distributed().last_search);
    if last.as_deref() == Some(frame) {
      return true;
    }
    *last = Some(frame.to_vec());
    false
  }

  /// A search the parent passes down: only one with the protocol's identifier counts. It goes on
  /// to every child exactly as received and is answered unless it is our own.
  fn search_from_parent(&self, conn: &MessageConn, link: &Link, frame: &[u8]) {
    let identifier = frame
      .get(5..9)
      .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    if identifier.is_some_and(|identifier| identifier != SEARCH_IDENTIFIER) {
      let network = self.distributed();
      let total = network
        .invalid_search_requests
        .fetch_add(1, Ordering::Relaxed)
        + 1;
      if !link.bad_search_logged.swap(true, Ordering::Relaxed) {
        tracing::debug!(
          parent = conn.handle().username(),
          identifier,
          total,
          "search from the parent with a bad identifier dropped"
        );
      }
      return;
    }
    let Ok(DistributedMessage::SearchRequest(search)) = DistributedMessage::decode(frame) else {
      // the decoder has logged why
      return;
    };
    self.broadcast(frame);
    if self.username().as_deref() == Some(search.username.as_str()) {
      return tracing::trace!(
        token = search.token,
        "our own search from the parent, not answered"
      );
    }
    self.respond_to_search(search.username, search.token, search.query);
  }

  /// A distributed message the server wrapped for us. Only a search is ever embedded, and the
  /// server sends those only to branch roots; it goes on to the children unwrapped.
  pub(crate) fn handle_embedded(&self, embedded: &Embedded) {
    if embedded.distributed_code != DistributedCode::SearchRequest.code() as u8 {
      return tracing::debug!(
        code = embedded.distributed_code,
        "unhandled embedded message"
      );
    }
    if !self.distributed().is_branch_root() {
      self.distributed_command(Command::Promote);
    }
    let inner = embedded.inner_frame();
    // the decoder has logged anything it rejects
    if let Ok(DistributedMessage::SearchRequest(search)) = DistributedMessage::decode(&inner) {
      self.broadcast(&inner);
      self.respond_to_search(search.username, search.token, search.query);
    }
  }

  /// Starts the read loop of a child's connection
  pub(super) fn start_child_reading(&self, conn: &MessageConn) {
    let client = self.downgrade();
    let reader = conn.clone();
    conn.start_reading(move |event| {
      if let ReadEvent::Frame(frame) = event
        && let Some(client) = client.upgrade()
      {
        client.child_frame(&reader, &frame);
      }
    });
  }

  fn child_frame(&self, conn: &MessageConn, frame: &[u8]) {
    match DistributedMessage::decode(frame) {
      Ok(DistributedMessage::Ping(_)) => {
        let pong = DistributedMessage::Ping(Ping { token: None }).encode();
        if let Err(error) = conn.send(pong) {
          tracing::debug!(%error, "ping not answered");
        }
      }
      Ok(DistributedMessage::ChildDepth(depth)) => {
        tracing::trace!(depth = depth.depth, "child depth ignored");
      }
      Ok(message) => {
        let code = message.code();
        tracing::debug!(
          code = code.code(),
          name = code.name(),
          "unhandled distributed message from a child"
        );
      }
      Err(DecodeError::UnknownCode { code }) => {
        tracing::debug!(code, "unhandled distributed message from a child");
      }
      // the decoder has logged it
      Err(_) => {}
    }
  }

  /// Passes `frame` to every child at once. A child that cannot take it is closed; how long the
  /// slowest took feeds the moving average of broadcast latency.
  fn broadcast(&self, frame: &[u8]) {
    let children = self.distributed().children();
    if children.is_empty() {
      return;
    }
    let started = Instant::now();
    let mut receipts = Vec::with_capacity(children.len());
    for child in children.iter() {
      match child.send_with_receipt(frame.to_vec()) {
        Ok(receipt) => receipts.push((child.clone(), receipt)),
        Err(error) => {
          child
            .handle()
            .disconnect(format!("Broadcast failure: {error}"), Some(error));
        }
      }
    }
    let client = self.downgrade();
    tokio::spawn(
      async move {
        for (child, receipt) in receipts {
          let written = receipt.await.unwrap_or_else(|_| {
            Err(ClientError::connection(
              ConnectionErrorKind::Write,
              "the connection closed before the search was written",
            ))
          });
          if let Err(error) = written {
            child
              .handle()
              .disconnect(format!("Broadcast failure: {error}"), Some(error));
          }
        }
        if let Some(client) = client.upgrade() {
          let millis = started.elapsed().as_secs_f64() * 1000.0;
          client.distributed().record_latency(millis);
        }
      }
      .in_current_span(),
    );
  }
}
