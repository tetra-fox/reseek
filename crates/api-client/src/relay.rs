//! The relay calls: a controller's agents, and the calls an agent makes to its controller with
//! its API key.

use api_types::relay::{RelayAgentState, RelayFileUploadResult, RelayState};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use reqwest::multipart::Form;
use uuid::Uuid;

use super::{Client, Result, Route, bytes, done, json};

/// Who an agent's call says it is.
#[derive(Clone, Copy, Debug)]
pub struct AgentCall<'a> {
  /// the token the controller asked or announced with
  pub token: Uuid,
  /// the agent's instance name
  pub agent: &'a str,
  /// the agent's credential for the token
  pub credential: &'a str,
}

impl Client {
  /// `GET /relay`: the relay's state.
  pub async fn relay(&self) -> Result<RelayState> {
    json(self.get(Route::new("relay"))).await
  }

  /// `GET /relay/agents`: the agents logged in to this controller.
  pub async fn relay_agents(&self) -> Result<Vec<RelayAgentState>> {
    json(self.get(Route::new("relay/agents"))).await
  }

  /// `DELETE /relay/agents/{name}`: closes an agent's connection.
  pub async fn kick_agent(&self, name: &str) -> Result<()> {
    done(self.delete(Route::new("relay/agents").segment(name))).await
  }

  /// `POST /relay/controller/shares/{token}`: an agent sends its share list and database.
  pub async fn send_agent_shares(&self, call: AgentCall<'_>, form: Form) -> Result<()> {
    let route = Route::new("relay/controller/shares").segment(&call.token.to_string());
    done(agent(self.post(route), call).multipart(form)).await
  }

  /// `POST /relay/controller/files/{token}`: an agent sends a file a peer downloads.
  pub async fn send_agent_file(
    &self,
    call: AgentCall<'_>,
    form: Form,
  ) -> Result<RelayFileUploadResult> {
    let route = Route::new("relay/controller/files").segment(&call.token.to_string());
    json(agent(self.post(route), call).multipart(form)).await
  }

  /// `GET /relay/controller/downloads/{token}`: an agent fetches a finished download by its
  /// path under the downloads directory.
  pub async fn fetch_agent_download(&self, call: AgentCall<'_>, filename: &str) -> Result<Bytes> {
    let route = Route::new("relay/controller/downloads").segment(&call.token.to_string());
    let request =
      agent(self.get(route), call).header("x-relay-filename-base64", STANDARD.encode(filename));
    bytes(request).await
  }
}

/// `request` with the headers that say which agent calls.
fn agent(request: reqwest::RequestBuilder, call: AgentCall<'_>) -> reqwest::RequestBuilder {
  request
    .header("x-relay-agent", call.agent)
    .header("x-relay-credential", call.credential)
}
