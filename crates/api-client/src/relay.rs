//! The relay calls: a controller's agents.

use api_types::relay::{RelayAgentState, RelayState};

use super::{Client, Result, Route, done, json};

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
}
