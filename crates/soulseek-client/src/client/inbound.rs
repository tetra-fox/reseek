//! Accepted connections, once their first message says what they are for, are counted and go to
//! the part of the client that owns that kind: a message connection, a transfer a download
//! waits for, a child for the distributed tree, or the answer to a connection request we had
//! the server relay.

use soulseek_proto::PeerConnectionType;

use super::Client;
use crate::conn::{Conn, ConnKind};
use crate::distributed::Command;
use crate::error::{ClientError, ConnectionErrorKind};
use crate::inbound::InboundConnection;
use crate::peers::{Answer, Solicitation, Via};

impl Client {
  /// Hands a classified connection to its owner. A connection nothing takes is closed with the
  /// reason, which it logs once.
  pub(super) async fn route_inbound(&self, inbound: InboundConnection) {
    self.inner.inbound.send_modify(|counted| *counted += 1);
    let routed = match inbound {
      InboundConnection::Peer { username, conn } => {
        let vouched = self.vouched(&username, &conn).await;
        self.add_or_update_message_connection(
          &username,
          conn,
          ConnKind::INBOUND | ConnKind::DIRECT,
          vouched,
        )
      }
      InboundConnection::Transfer {
        username,
        token,
        conn,
      } => {
        self
          .accept_transfer_connection(&username, token, conn)
          .await
      }
      InboundConnection::Distributed { username, conn } => {
        self.distributed_command(Command::ChildConnecting { username, conn });
        Ok(())
      }
      InboundConnection::PierceFirewall { token, conn } => self.pierced(token, conn).await,
    };
    if let Err(error) = routed {
      tracing::debug!(%error, "inbound connection not taken");
    }
  }

  /// Whether `conn`, introducing itself as `username`, may replace the open connection we have
  /// with that user: only when the server gives its address for the user. With nothing open
  /// the server is not asked, and the connection is taken whatever its address.
  async fn vouched(&self, username: &str, conn: &Conn) -> bool {
    self.peers().open(username).is_some()
      && self
        .server_gives(username, conn.handle().remote().ip())
        .await
  }

  /// A peer connected because the server relayed one of our requests: a solicitation still
  /// waiting takes it, or it is the searcher answers ride on the request for, who gets those
  /// kept now and the rest as they come to be kept. One that comes after its solicitor stopped
  /// waiting is taken as [`Client::late_answer`] says. Anything else is closed, a request
  /// answered once included. The ticket it answers with went only to the user it was relayed
  /// to, so it vouches for the connection.
  pub(crate) async fn pierced(&self, token: u32, conn: Conn) -> Result<(), ClientError> {
    let answered = || self.responder().answered(token);
    if let Some((solicitation, answer)) = self.peers().solicitations.take(token, answered) {
      let conn = match answer {
        Answer::Awaited => match self.waiter().offer(&solicitation.key(token), conn) {
          Ok(()) => return Ok(()),
          // the solicitor stopped waiting a moment ago
          Err(conn) => conn,
        },
        Answer::Late => conn,
      };
      return self.late_answer(token, &solicitation, conn).await;
    }
    if let Some(username) = self.responder().claim(token) {
      self.add_or_update_message_connection(&username, conn, Via::Indirect.kind(), true)?;
      self.try_respond_cached(token).await;
      return Ok(());
    }
    let error = ClientError::connection(
      ConnectionErrorKind::Other,
      format!(
        "Unknown PierceFirewall attempt with token {token} from {}",
        conn.handle().remote()
      ),
    );
    conn.handle().disconnect(error.to_string(), Some(error));
    Ok(())
  }

  /// A user answered a solicitation after its solicitor stopped waiting, usually because
  /// dialing the user directly won meanwhile. Some clients then count the answering
  /// connection as their connection to us and drop the one we dialed, so for messages it
  /// replaces ours, as when the user introduces itself on a new connection. A transfer already
  /// runs on the connection that won, and a parent candidate keeps the connection we dialed as
  /// its link to us, so those are closed.
  async fn late_answer(
    &self,
    token: u32,
    solicitation: &Solicitation,
    conn: Conn,
  ) -> Result<(), ClientError> {
    let username = &solicitation.username;
    let remote = conn.handle().remote();
    match solicitation.connection_type {
      PeerConnectionType::Peer => {
        tracing::debug!(
          username,
          token,
          %remote,
          "late answer to a message connection request taken"
        );
        self.add_or_update_message_connection(username, conn, Via::Indirect.kind(), true)?;
        // a search answer for the user kept under this token, its delivery having failed after
        // dialing won, goes out now
        self.try_respond_cached(token).await;
        Ok(())
      }
      PeerConnectionType::Transfer => {
        tracing::debug!(
          username,
          token,
          %remote,
          "late answer to a transfer connection request refused"
        );
        conn
          .handle()
          .disconnect("Transfer connection request already answered", None);
        Ok(())
      }
      PeerConnectionType::Distributed => {
        tracing::debug!(
          username,
          token,
          %remote,
          "late answer to a distributed connection request refused"
        );
        conn
          .handle()
          .disconnect("Distributed connection request already answered", None);
        Ok(())
      }
    }
  }
}
