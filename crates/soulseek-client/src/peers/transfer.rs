//! Transfer connections, one per file and never kept: the one we open to send or fetch a file,
//! the one an uploader opens to us for a download that waits for it, directly or because it
//! asked the server to have us dial it. The token that follows the connection's first message
//! says which transfer it carries.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use soulseek_proto::init::FileTransferInit;
use soulseek_proto::server::ConnectToPeerResponse;
use soulseek_proto::{PeerConnectionType, RemoteFilename};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::message::request_endpoint;
use super::solicit::{Via, both_failed, race};
use super::{PeerConnections, lock};
use crate::client::Client;
use crate::conn::{Conn, ConnKind};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::waiter::WaitKey;

impl Client {
  /// Opens a transfer connection to `username` for the transfer `token`: the user is dialed
  /// directly and asked through the server to dial us, and whichever connects first carries the
  /// transfer. The token is written on it before it is returned.
  pub async fn get_transfer_connection(
    &self,
    username: &str,
    endpoint: SocketAddr,
    token: u32,
    cancel: &CancellationToken,
  ) -> Result<Conn, ClientError> {
    self.check_not_disposed()?;
    let own = self.own_username("establish a transfer connection")?;
    let options = self.options().transfer_connection_options.clone();
    let ticket = self.issue_ticket(
      username,
      PeerConnectionType::Transfer,
      options.connect_timeout,
    );
    let stop = cancel.child_token();
    let direct = self.connect_to_peer_socket(
      endpoint,
      username,
      Via::Direct.kind(),
      options.clone(),
      &stop,
    );
    let indirect = self.solicit(ticket, options.clone(), &stop);
    let (mut conn, via) = race(direct, indirect, &stop).await.map_err(|errors| {
      both_failed(
        format!(
          "Failed to establish a direct or indirect transfer connection to {username} ({endpoint})"
        ),
        errors,
      )
    })?;
    tracing::debug!(username, token, ?via, "transfer connection established");
    // a connection the user dialed needs no introduction, only the transfer token
    let mut init = via
      .introduction(&own, PeerConnectionType::Transfer)
      .unwrap_or_default();
    init.extend(FileTransferInit { token }.encode());
    conn.write_all(&init).await.map_err(|error| {
      ClientError::connection(
        ConnectionErrorKind::Other,
        format!("Failed to negotiate transfer connection to {username} ({endpoint}): {error}"),
      )
    })?;
    Ok(conn)
  }

  /// Waits for `username` to open the transfer connection for `filename`, which it names with
  /// `remote_token`, the token we gave it. The connection may arrive directly or after the user
  /// asked the server to have us dial it. The waits are in place when this returns, so the
  /// caller tells the uploader to connect only afterwards; the returned future resolves with the
  /// connection, or fails once both ways time out.
  pub fn await_transfer_connection(
    &self,
    username: &str,
    filename: RemoteFilename,
    remote_token: u32,
    cancel: &CancellationToken,
  ) -> impl Future<Output = Result<Conn, ClientError>> + Send + 'static {
    let timeout = Some(self.options().transfer_connection_options.connect_timeout);
    let stop = cancel.child_token();
    let direct = self.waiter().wait::<Conn>(
      WaitKey::DirectTransfer {
        username: username.to_owned(),
        remote_token,
      },
      timeout,
      stop.clone(),
    );
    let indirect = self.waiter().wait::<Conn>(
      WaitKey::IndirectTransfer {
        username: username.to_owned(),
        filename: filename.clone(),
        remote_token,
      },
      timeout,
      stop.clone(),
    );
    let awaited = AwaitedTransfer::register(self.peers_arc(), username, remote_token, filename);
    let username = username.to_owned();
    async move {
      // on a tie the connection we dialed is kept: an uploader asks for it once its own dial failed
      let (conn, via) = race(direct, indirect, &stop).await.map_err(|errors| {
        both_failed(
          format!(
            "Failed to establish a direct or indirect transfer connection to {username} with remote token {remote_token} for {}",
            awaited.filename
          ),
          errors,
        )
      })?;
      tracing::debug!(
        username,
        remote_token,
        ?via,
        "awaited transfer connection arrived"
      );
      Ok(conn)
    }
  }

  /// A peer connected to us directly for a transfer. The token after its first message names
  /// the transfer; a connection nothing waits for is closed.
  pub(crate) async fn accept_transfer_connection(
    &self,
    username: &str,
    peer_init_token: u32,
    provisional: Conn,
  ) -> Result<(), ClientError> {
    let remote = provisional.handle().remote();
    let failed = |error: ClientError| {
      ClientError::connection(
        ConnectionErrorKind::Other,
        format!(
          "Failed to establish an inbound transfer connection to {username} ({remote}) for token {peer_init_token}: {error}"
        ),
      )
    };
    let stream = provisional.handoff().map_err(failed)?;
    let mut conn = Conn::from_handoff(
      stream,
      Some(username.to_owned()),
      ConnKind::INBOUND | ConnKind::DIRECT,
      self.options().transfer_connection_options.clone(),
    )
    .map_err(failed)?;
    let raw = conn.read_exact(4).await.map_err(failed)?;
    let remote_token = FileTransferInit::decode(&raw)
      .map_err(|error| failed(ClientError::message_read(error.to_string(), error)))?
      .token;
    tracing::debug!(
      username,
      peer_init_token,
      remote_token,
      "inbound transfer connection"
    );
    let key = WaitKey::DirectTransfer {
      username: username.to_owned(),
      remote_token,
    };
    if let Err(conn) = self.waiter().offer(&key, conn) {
      tracing::debug!(
        username,
        peer_init_token,
        remote_token,
        "Unexpected transfer connection"
      );
      conn
        .handle()
        .disconnect("Transfer connection rejected: unknown token", None);
    }
    Ok(())
  }

  /// A user asked the server to have us dial it for a transfer. We do only for a download that
  /// waits for that user's connection: dial, answer with the request's token, read which
  /// transfer the user means and hand the connection to that download. Anything else is
  /// refused and reported to the server.
  pub(crate) fn connect_to_peer_for_transfer(&self, request: ConnectToPeerResponse) {
    let username = request.username.clone();
    let expected = lock(&self.peers().awaited_transfers)
      .keys()
      .any(|(awaited, _)| *awaited == username);
    if !expected {
      tracing::debug!(
        username,
        ip = %request.ip,
        port = request.port,
        "Unexpected transfer request; Ignored"
      );
      self.send_cannot_connect(request.token, &username);
      return;
    }
    let client = self.clone();
    let span = tracing::debug_span!("transfer_connection", username, token = request.token);
    tokio::spawn(
      async move {
        if let Err(error) = client.connect_for_awaited_transfer(&request).await {
          tracing::debug!(%error, "inbound indirect transfer connection failed");
          client.send_cannot_connect(request.token, &request.username);
        }
      }
      .instrument(span),
    );
  }

  async fn connect_for_awaited_transfer(
    &self,
    request: &ConnectToPeerResponse,
  ) -> Result<(), ClientError> {
    let username = &request.username;
    let failed = |reason: String| {
      ClientError::connection(
        ConnectionErrorKind::Other,
        format!(
          "Failed to establish an inbound indirect transfer connection to {username} ({}:{}): {reason}",
          request.ip, request.port
        ),
      )
    };
    let endpoint = request_endpoint(request.ip, request.port)
      .ok_or_else(|| failed("the port is out of range".into()))?;
    let mut conn = self
      .pierce(
        username,
        endpoint,
        request.token,
        self.options().transfer_connection_options.clone(),
        self.cancellation(),
      )
      .await
      .map_err(|error| failed(error.to_string()))?;
    let raw = conn
      .read_exact(4)
      .await
      .map_err(|error| failed(error.to_string()))?;
    let remote_token = FileTransferInit::decode(&raw)
      .map_err(|error| failed(error.to_string()))?
      .token;
    let filename = lock(&self.peers().awaited_transfers)
      .get(&(username.clone(), remote_token))
      .cloned();
    let Some(filename) = filename else {
      conn.handle().disconnect("Unknown transfer", None);
      return Err(failed(format!(
        "no transfer waits for remote token {remote_token}"
      )));
    };
    let key = WaitKey::IndirectTransfer {
      username: username.clone(),
      filename,
      remote_token,
    };
    if let Err(conn) = self.waiter().offer(&key, conn) {
      conn.handle().disconnect("Unknown transfer", None);
      return Err(failed(format!(
        "the transfer for remote token {remote_token} stopped waiting"
      )));
    }
    tracing::debug!(remote_token, "inbound indirect transfer connection");
    Ok(())
  }
}

/// a download waiting for its uploader's transfer connection, listed while the wait lasts
struct AwaitedTransfer {
  peers: Arc<PeerConnections>,
  key: (String, u32),
  filename: RemoteFilename,
}

impl AwaitedTransfer {
  fn register(
    peers: Arc<PeerConnections>,
    username: &str,
    remote_token: u32,
    filename: RemoteFilename,
  ) -> Self {
    let key = (username.to_owned(), remote_token);
    lock(&peers.awaited_transfers).insert(key.clone(), filename.clone());
    Self {
      peers,
      key,
      filename,
    }
  }
}

impl Drop for AwaitedTransfer {
  fn drop(&mut self) {
    lock(&self.peers.awaited_transfers).remove(&self.key);
  }
}
