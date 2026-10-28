//! The relay API's calls, each answered by the half it is for. The routes check the relay mode
//! the daemon started in first, so a call only reaches a half this daemon runs.

use api_types::relay::{RELAY_SUBPROTOCOL, RelayAgentState, RelayFileUploadResult};
use async_trait::async_trait;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{FromRequestParts, Multipart, Request};
use axum::http::header;
use axum::response::Response;
use tracing::debug;
use uuid::Uuid;

use super::frames::MAX_FRAME;
use super::{AgentClient, Controller, Relay};
use crate::api::error::ApiError;
use crate::auth::Client;
use crate::services::{AgentCredential, RelayService};

impl Relay {
  fn controller(&self) -> &std::sync::Arc<Controller> {
    self
      .controller
      .as_ref()
      .expect("the controller's routes answer only in controller and debug mode")
  }

  fn agent(&self) -> &std::sync::Arc<AgentClient> {
    self
      .agent
      .as_ref()
      .expect("the agent's routes answer only in agent and debug mode")
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

  async fn start_agent(&self) -> Result<(), ApiError> {
    self.agent().start();
    Ok(())
  }

  async fn stop_agent(&self) -> Result<(), ApiError> {
    self.agent().stop();
    Ok(())
  }

  async fn sync(&self) -> Result<(), ApiError> {
    if self.agent().sync_now() {
      Ok(())
    } else {
      Err(ApiError::conflict(
        "not_connected",
        Some("the agent is not logged in to its controller".to_string()),
      ))
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
        "the relay socket needs the {RELAY_SUBPROTOCOL} protocol"
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
