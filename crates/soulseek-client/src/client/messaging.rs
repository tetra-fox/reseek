//! Private messages: sending one, and acknowledging one we received so the server stops
//! replaying it at every login.

use soulseek_proto::ServerRequest;
use soulseek_proto::server::{AcknowledgePrivateMessage, PrivateMessageRequest};

use super::Client;
use super::request::{failed, non_blank, non_empty};
use crate::error::ClientError;
use crate::state::ClientState;

impl Client {
  /// sends `message` to `username`; the server does not echo it back to us
  pub async fn send_private_message(
    &self,
    username: &str,
    message: &str,
  ) -> Result<(), ClientError> {
    non_blank("username", username)?;
    non_empty("message", message)?;
    self
      .tell(
        &format!("send private message to user {username}"),
        ServerRequest::PrivateMessage(PrivateMessageRequest {
          username: username.to_owned(),
          message: message.to_owned(),
        }),
      )
      .await
  }

  /// Acknowledges the private message `id`. Allowed while logging in too, because the server
  /// replays stored messages before the login completes.
  pub async fn acknowledge_private_message(&self, id: u32) -> Result<(), ClientError> {
    let verb = format!("acknowledge private message with ID {id}");
    self.require_logging_in_or_logged_in(&verb)?;
    self
      .write_server_message(&ServerRequest::AcknowledgePrivateMessage(
        AcknowledgePrivateMessage { id },
      ))
      .await
      .map_err(|error| failed(&verb, error))
  }

  /// fails unless connected and logging in or logged in, naming what was attempted
  pub(super) fn require_logging_in_or_logged_in(&self, verb: &str) -> Result<(), ClientError> {
    let state = self.state();
    if state.contains(ClientState::CONNECTED)
      && state.intersects(ClientState::LOGGING_IN | ClientState::LOGGED_IN)
    {
      return Ok(());
    }
    Err(ClientError::InvalidOperation(format!(
      "The server connection must be connected and logging in or logged in to {verb} (currently: {state})"
    )))
  }
}
