//! Accepted connections, once their first message says what they are for, go to the part of the
//! client that owns that kind: a message connection, a transfer a download waits for, a child
//! for the distributed tree, or the answer to a connection request we had the server relay.

use super::Client;
use crate::conn::Conn;
use crate::distributed::Command;
use crate::error::{ClientError, ConnectionErrorKind};
use crate::inbound::InboundConnection;

impl Client {
  /// Hands a classified connection to its owner. A connection nothing takes is closed with the
  /// reason, which it logs once.
  pub(super) async fn route_inbound(&self, inbound: InboundConnection) {
    let routed = match inbound {
      InboundConnection::Peer { username, conn } => {
        self.add_or_update_message_connection(&username, conn)
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

  /// A peer connected because the server relayed one of our requests: a solicitation still
  /// waiting takes it, or it is the searcher an undelivered answer was kept for, who gets the
  /// answer now. Anything else is closed.
  async fn pierced(&self, token: u32, conn: Conn) -> Result<(), ClientError> {
    if let Some(solicitation) = self.peers().solicitations.get(token) {
      if let Err(conn) = self.waiter().offer(&solicitation.key(token), conn) {
        conn
          .handle()
          .disconnect("Connection request no longer awaited", None);
      }
      return Ok(());
    }
    if let Some(username) = self.responder().kept_for(token) {
      self.add_or_update_message_connection(&username, conn)?;
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
}
