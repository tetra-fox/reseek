//! The relay API's calls, answered by the controller. The routes check that the daemon started
//! with relay mode on first, so a call only reaches a controller that runs.

use api_types::relay::{RELAY_SUBPROTOCOL, RelayAgentState, RelayFileUploadResult};
use async_trait::async_trait;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{FromRequestParts, Multipart, Request};
use axum::http::header;
use axum::response::Response;
use tracing::debug;
use uuid::Uuid;

use super::{Controller, Relay};
use crate::api::error::ApiError;
use crate::auth::Client;
use crate::services::{AgentCredential, RelayService};
use reseek_relay_proto::frames::MAX_FRAME;

impl Relay {
  fn controller(&self) -> &std::sync::Arc<Controller> {
    self
      .controller
      .as_ref()
      .expect("the relay routes answer only with relay mode on")
  }
}

#[async_trait]
impl RelayService for Relay {
  async fn agents(&self) -> Result<Vec<RelayAgentState>, ApiError> {
    Ok(self.controller().agents())
  }

  async fn kick(&self, name: &str) -> Result<(), ApiError> {
    if self.controller().kick(name) {
      Ok(())
    } else {
      Err(ApiError::not_found(format!(
        "Agent {name} is not connected"
      )))
    }
  }

  async fn receive_shares(
    &self,
    token: Uuid,
    agent: AgentCredential,
    form: Multipart,
  ) -> Result<(), ApiError> {
    self.controller().receive_shares(token, agent, form).await
  }

  async fn receive_file(
    &self,
    token: Uuid,
    agent: AgentCredential,
    form: Multipart,
  ) -> Result<RelayFileUploadResult, ApiError> {
    self.controller().receive_file(token, agent, form).await
  }

  async fn serve_download(
    &self,
    token: Uuid,
    agent: AgentCredential,
    filename: String,
  ) -> Result<Response, ApiError> {
    self
      .controller()
      .serve_download(token, agent, filename)
      .await
  }

  async fn socket(&self, request: Request) -> Result<Response, ApiError> {
    let (mut parts, _) = request.into_parts();
    let offered = parts
      .headers
      .get_all(header::SEC_WEBSOCKET_PROTOCOL)
      .iter()
      .filter_map(|value| value.to_str().ok())
      .flat_map(|value| value.split(','))
      .any(|protocol| protocol.trim() == RELAY_SUBPROTOCOL);
    if !offered {
      return Err(ApiError::upgrade_required(format!(
        "The relay socket requires the {RELAY_SUBPROTOCOL} protocol"
      )));
    }
    let address = parts
      .extensions
      .get::<Client>()
      .map(|client| client.ip)
      .expect("every request carries its client once it is authenticated");
    let upgrade = WebSocketUpgrade::from_request_parts(&mut parts, &())
      .await
      .map_err(|e| {
        debug!(error = %e, "relay socket upgrade refused");
        ApiError::upgrade_required(e.body_text())
      })?;
    let controller = self.controller().clone();
    Ok(
      upgrade
        .protocols([RELAY_SUBPROTOCOL])
        // larger frames than the socket reads are refused by its own check, with a close code
        .max_message_size(2 * MAX_FRAME)
        .on_upgrade(move |socket| controller.serve_socket(socket, address)),
    )
  }
}
