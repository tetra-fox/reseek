//! Relay mode: the controller's agents and their uploads, and an agent's connection to its
//! controller.

use api_types::relay::{RelayAgentState, RelayFileUploadResult};
use async_trait::async_trait;
use axum::extract::Multipart;
use axum::response::Response;
use uuid::Uuid;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "relay";

/// Who an agent's request says it is: its instance name and the credential it made for the
/// request's token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCredential {
  /// the agent's instance name, from `X-Relay-Agent`
  pub agent: String,
  /// the credential for the token, from `X-Relay-Credential`
  pub credential: String,
}

/// The agents connected to this controller with what they send it, and this agent's own
/// connection to its controller. An unknown or unregistered agent, and a credential that does
/// not check out, is a 401.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait RelayService: Send + Sync {
  /// The agents that logged in to this controller.
  async fn agents(&self) -> Result<Vec<RelayAgentState>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Closes an agent's connection and unloads its shares; 404 when it is not connected.
  async fn kick(&self, name: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Starts this agent's connection to its controller, nothing when it is started already.
  async fn start_agent(&self) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Closes this agent's connection to its controller.
  async fn stop_agent(&self) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Sends this agent's shares to its controller again; a conflict while not connected.
  async fn sync(&self) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Takes an agent's share list and share database for the upload asked for with `token`.
  async fn receive_shares(
    &self,
    token: Uuid,
    agent: AgentCredential,
    form: Multipart,
  ) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Takes the file an agent sends for the upload asked for with `token`, passes it on to the
  /// peer, and answers once that upload has ended.
  async fn receive_file(
    &self,
    token: Uuid,
    agent: AgentCredential,
    form: Multipart,
  ) -> Result<RelayFileUploadResult, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A finished download an agent fetches with `token`, by its path relative to the downloads
  /// directory, as the whole response.
  async fn serve_download(
    &self,
    token: Uuid,
    agent: AgentCredential,
    filename: String,
  ) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The agent socket, from the request that asks for it, as the whole response.
  async fn socket(&self, request: axum::extract::Request) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
