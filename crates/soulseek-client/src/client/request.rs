//! The shape every server operation shares: check the arguments, require a logged-in session,
//! register the wait for the reply before writing the request, and report a failure as a
//! failure to do what was asked, keeping timeouts, cancellation and the operation's own
//! outcomes as they are.

use std::time::Duration;

use soulseek_proto::{ServerCode, ServerRequest};
use tokio_util::sync::CancellationToken;

use super::Client;
use crate::error::ClientError;
use crate::state::ClientState;
use crate::waiter::WaitKey;

impl Client {
  /// fails unless logged in, naming what was attempted
  pub(crate) fn require_logged_in(&self, verb: &str) -> Result<(), ClientError> {
    let state = self.state();
    if state.is_logged_in() {
      return Ok(());
    }
    Err(not_logged_in(verb, state))
  }

  /// Writes `request` and returns the reply that completes the wait under `key`, which is in
  /// place before the request is written so a fast reply cannot be missed
  pub(super) async fn ask<T: Send + 'static>(
    &self,
    verb: &str,
    key: WaitKey,
    request: ServerRequest,
  ) -> Result<T, ClientError> {
    self.ask_within(verb, key, request, None).await
  }

  /// [`Client::ask`] with a timeout other than the default one
  pub(super) async fn ask_within<T: Send + 'static>(
    &self,
    verb: &str,
    key: WaitKey,
    request: ServerRequest,
    timeout: Option<Duration>,
  ) -> Result<T, ClientError> {
    self.require_logged_in(verb)?;
    let reply = self
      .waiter()
      .wait::<T>(key, timeout, CancellationToken::new());
    self
      .write_server_message(&request)
      .await
      .map_err(|error| failed(verb, error))?;
    reply.await.map_err(|error| failed(verb, error))
  }

  /// writes `request`, which the server does not answer, and waits until it is on the socket
  pub(super) async fn tell(&self, verb: &str, request: ServerRequest) -> Result<(), ClientError> {
    self.require_logged_in(verb)?;
    self
      .write_server_message(&request)
      .await
      .map_err(|error| failed(verb, error))
  }
}

/// what a failed operation reports: its own outcomes as they are, anything else as a failure
/// to `verb` wrapping the cause
pub(crate) fn failed(verb: &str, error: ClientError) -> ClientError {
  match error {
    ClientError::Timeout(_)
    | ClientError::Canceled
    | ClientError::UserOffline(_)
    | ClientError::UserNotFound(_)
    | ClientError::Room { .. }
    | ClientError::NoResponse(_) => error,
    error => {
      tracing::debug!(verb, %error, "server operation failed");
      ClientError::client(format!("Failed to {verb}: {error}"), Some(error))
    }
  }
}

pub(super) fn user_key(code: ServerCode, username: &str) -> WaitKey {
  WaitKey::ServerUser {
    code,
    username: username.to_owned(),
  }
}

pub(super) fn interest_key(code: ServerCode, interest: &str) -> WaitKey {
  WaitKey::ServerInterest {
    code,
    interest: interest.to_owned(),
  }
}

pub(super) fn room_key(code: ServerCode, room: &str) -> WaitKey {
  WaitKey::ServerRoom {
    code,
    room: room.to_owned(),
  }
}

pub(super) fn room_user_key(code: ServerCode, room: &str, username: &str) -> WaitKey {
  WaitKey::ServerRoomUser {
    code,
    room: room.to_owned(),
    username: username.to_owned(),
  }
}

/// fails when `value` is empty or only whitespace, naming it as `name`
pub(crate) fn non_blank(name: &str, value: &str) -> Result<(), ClientError> {
  if value.trim().is_empty() {
    return Err(ClientError::InvalidArgument(format!(
      "The {name} must not be a null or empty string, or one consisting only of whitespace"
    )));
  }
  Ok(())
}

/// fails when `value` is empty, naming it as `name`; whitespace is allowed
pub(super) fn non_empty(name: &str, value: &str) -> Result<(), ClientError> {
  if value.is_empty() {
    return Err(ClientError::InvalidArgument(format!(
      "The {name} must not be a null or empty string"
    )));
  }
  Ok(())
}

/// The refusal of the operation `verb` while the session is in `state`, short of logged in.
pub(super) fn not_logged_in(verb: &str, state: ClientState) -> ClientError {
  ClientError::InvalidOperation(format!(
    "The server connection must be connected and logged in to {verb} (currently: {state})"
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_operation_keeps_its_own_outcomes_and_wraps_the_rest() {
    assert!(matches!(
      failed("join chat room x", ClientError::Timeout("late".into())),
      ClientError::Timeout(_)
    ));
    assert!(matches!(
      failed(
        "get the status of user x",
        ClientError::UserOffline("gone".into())
      ),
      ClientError::UserOffline(_)
    ));
    let wrapped = failed(
      "leave chat room x",
      ClientError::InvalidOperation("closed".into()),
    );
    assert!(
      matches!(&wrapped, ClientError::Client { message, .. } if message == "Failed to leave chat room x: closed")
    );
  }
}
