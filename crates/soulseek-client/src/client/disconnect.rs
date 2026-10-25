//! Ending the server session. The state moves to disconnecting, waits on the server are failed
//! and the session's resources are taken in one step under the session lock; stopping the
//! listener, closing the server connection, tearing down the distributed tree, cancelling our
//! searches and forgetting the connection requests the server relayed follow, and the state ends
//! at disconnected. Message and transfer connections to other users, and the transfers on them,
//! are not part of the session and go on.

use uuid::Uuid;

use super::{Client, Inner};
use crate::conn::{Disconnect, MessageConn};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::listener::Listener;
use crate::state::ClientState;
use crate::waiter::Scope;

/// What ended the session, which decides whether a `connect` in progress is cancelled
pub(super) enum Cause {
  /// the application asked, or a failed `connect` cleans up; a `connect` in progress reports
  /// `Cancelled`
  Requested,
  /// the server dropped us; a `connect` in progress fails on its own
  Kicked,
  /// this server connection closed; ignored if it is no longer the session's connection
  ServerLost(Uuid),
}

/// The second half of a disconnect, after the state has moved to disconnecting
pub(super) struct Teardown {
  message: String,
  error: Option<ClientError>,
  listener: Option<Listener>,
  server: Option<MessageConn>,
}

impl Client {
  /// Ends the server session with `message` and `error` as the reason the `Disconnected` event
  /// carries (the message defaults to the error's). Does nothing when already disconnected or
  /// disconnecting; cancels a `connect` in progress.
  pub async fn disconnect(&self, message: Option<&str>, error: Option<ClientError>) {
    if let Some(teardown) = self
      .inner
      .begin_disconnect(message, error, Cause::Requested)
    {
      teardown.finish(self).await;
    }
  }

  /// the server connection closed by itself: a read or write failed, or the remote closed it
  pub(super) async fn server_disconnected(&self, id: Uuid, reason: Disconnect) {
    if let Some(teardown) =
      self
        .inner
        .begin_disconnect(Some(&reason.message), reason.error, Cause::ServerLost(id))
    {
      teardown.finish(self).await;
    }
  }
}

impl Inner {
  /// Moves to disconnecting and takes the session's resources, or returns `None` when there is
  /// nothing to disconnect.
  pub(super) fn begin_disconnect(
    &self,
    message: Option<&str>,
    error: Option<ClientError>,
    cause: Cause,
  ) -> Option<Teardown> {
    let mut session = self.session();
    let state = *self.state.borrow();
    if state.intersects(ClientState::DISCONNECTED | ClientState::DISCONNECTING) {
      return None;
    }
    if let Cause::ServerLost(id) = cause
      && session.server.as_ref().map(|server| server.handle().id()) != Some(id)
    {
      return None;
    }
    let message = match (message, &error) {
      (Some(message), _) => message.to_owned(),
      (None, Some(error)) => error.to_string(),
      (None, None) => "Client disconnected".to_owned(),
    };
    self.transition(
      &mut session,
      ClientState::DISCONNECTING,
      message.clone(),
      None,
    );
    if let Cause::Requested = cause
      && let Some(attempt) = &session.attempt
    {
      attempt.cancel();
    }
    let faulted = self.waiter.fault_scope(
      &Scope::Server,
      &ClientError::connection(
        ConnectionErrorKind::Other,
        format!("server connection disconnected: {message}"),
      ),
    );
    if faulted > 0 {
      tracing::debug!(faulted, "failed the waits on the server");
    }
    Some(Teardown {
      message,
      error,
      listener: session.listener.take(),
      server: session.server.take(),
    })
  }
}

impl Teardown {
  pub(super) async fn finish(self, client: &Client) {
    let inner = &*client.inner;
    let Self {
      message,
      error,
      listener,
      server,
    } = self;
    if let Some(listener) = listener {
      listener.stop().await;
    }
    if let Some(server) = server {
      server.handle().disconnect(message.clone(), error.clone());
    }
    client.tear_down_distributed().await;
    // responses to our searches reach us through the server's session
    client.searches().cancel_all();
    // answers to those requests would now be taken for a session that no longer exists
    client.peers().solicitations.clear();
    match &error {
      None | Some(ClientError::Cancelled | ClientError::Disposed) => {
        tracing::info!(message, "disconnected from server");
      }
      Some(error) => tracing::warn!(message, %error, "disconnected from server"),
    }
    let mut session = inner.session();
    session.logging_in_as = None;
    session.username = None;
    session.address = None;
    session.ip_endpoint = None;
    inner.transition(&mut session, ClientState::DISCONNECTED, message, error);
  }
}
