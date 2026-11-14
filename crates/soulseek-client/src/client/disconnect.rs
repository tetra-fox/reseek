//! Ending the server session. The state moves to disconnecting, waits on the server are failed
//! and the session's resources are taken in one step under the session lock; stopping the
//! listener, closing the server connection, tearing down the distributed tree, canceling our
//! searches and forgetting the connection requests the server relayed follow, and the state ends
//! at disconnected. Message and transfer connections to other users, and the transfers on them,
//! are not part of the session and go on.

use tokio::task::JoinHandle;
use tracing::Instrument;
use uuid::Uuid;

use super::session::{Edge, Released, Stage};
use super::{Client, Inner};
use crate::conn::Disconnect;
use crate::error::{ClientError, ConnectionErrorKind};
use crate::state::ClientState;
use crate::waiter::Scope;

/// What ended the session, which decides whether a connect in progress is canceled and how its
/// end is logged
pub(super) enum Cause {
  /// the application asked: every `connect` called before it reports `Canceled`, the one under
  /// way and the ones still waiting their turn
  Requested,
  /// a `connect` ends the session it began, because it failed or was dropped before it logged
  /// in; the connects waiting their turn go on
  Abandoned,
  /// the server dropped us; a `connect` in progress fails on its own
  Kicked,
  /// this server connection closed; ignored if it is no longer the session's connection
  ServerLost(Uuid),
}

/// The second half of a disconnect, after the state has moved to disconnecting
pub(super) struct Teardown {
  message: String,
  error: Option<ClientError>,
  cause: Cause,
  /// the session had logged in; one that ends before is a connect that failed
  logged_in: bool,
  released: Released,
}

impl Client {
  /// Ends the server session with `message` and `error` as the reason the `Disconnected` event
  /// carries (the message defaults to the error's), and returns once the session has ended. A
  /// disconnect already under way is waited for instead. Every `connect` called before this
  /// ends with `Canceled`, under way or still waiting for its turn. The teardown runs on a task
  /// of its own, so dropping this call does not leave the client disconnecting.
  pub async fn disconnect(&self, message: Option<&str>, error: Option<ClientError>) {
    self.end_session(message, error, Cause::Requested).await;
  }

  /// ends the session for `cause` and returns once it has ended, as `disconnect` does
  pub(super) async fn end_session(
    &self,
    message: Option<&str>,
    error: Option<ClientError>,
    cause: Cause,
  ) {
    let Some(teardown) = self.inner.begin_disconnect(message, error, cause) else {
      self.teardown_ended().await;
      return;
    };
    // the other error is the runtime shutting down, which drops the teardown with every task
    if let Err(e) = teardown.spawn(self).await
      && e.is_panic()
    {
      std::panic::resume_unwind(e.into_panic());
    }
  }

  /// waits until no teardown is under way
  async fn teardown_ended(&self) {
    let mut state = self.inner.state.subscribe();
    // the client holds the sender for as long as `self` lives, so this cannot fail
    let _ = state
      .wait_for(|state| *state != ClientState::Disconnecting)
      .await;
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
    if let Cause::ServerLost(id) = cause
      && session.stage.link().map(|link| link.server.handle().id()) != Some(id)
    {
      return None;
    }
    // ahead of the edge, which a session disconnecting or disconnected refuses while a connect
    // can still wait there for its turn
    if let Cause::Requested = cause {
      std::mem::take(&mut session.connects).cancel();
    }
    let message = match (message, &error) {
      (Some(message), _) => message.to_owned(),
      (None, Some(error)) => error.to_string(),
      (None, None) => "Client disconnected".to_owned(),
    };
    let logged_in = matches!(session.stage, Stage::LoggedIn { .. });
    // refused when already disconnected or disconnecting: there is nothing to tear down
    let released = self
      .transition(&mut session, Edge::Disconnect, message.clone(), None)
      .ok()?;
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
      cause,
      logged_in,
      released,
    })
  }
}

impl Teardown {
  /// Runs the teardown on a task of its own: it is the only way out of disconnecting, so it
  /// must reach its end whether or not anyone waits for it.
  pub(super) fn spawn(self, client: &Client) -> JoinHandle<()> {
    let client = client.clone();
    tokio::spawn(async move { self.finish(&client).await }.in_current_span())
  }

  async fn finish(self, client: &Client) {
    let inner = &*client.inner;
    let Self {
      message,
      error,
      cause,
      logged_in,
      released: Released { listener, server },
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
    let mut session = inner.session();
    match (&error, cause) {
      (None | Some(ClientError::Canceled | ClientError::Disposed), _) => {
        tracing::info!(message, "disconnected from server");
      }
      // the application asked for it and says so itself; its error is the reason it gave
      (Some(error), Cause::Requested) => {
        tracing::debug!(message, %error, "disconnected from server");
      }
      // a connect that failed is tried again, so each way it fails is warned about once until
      // a login succeeds
      (Some(error), _) if !logged_in => {
        if session.outage.new_reason(&error.to_string()) {
          tracing::warn!(message, %error, "disconnected from server");
        } else {
          tracing::debug!(message, %error, "disconnected from server");
        }
      }
      (Some(error), _) => tracing::warn!(message, %error, "disconnected from server"),
    }
    inner
      .transition(&mut session, Edge::Finish, message, error)
      .expect("only the teardown that began a disconnect ends it");
  }
}
