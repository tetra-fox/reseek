//! The application service: connecting to the Soulseek server and disconnecting from it, the
//! release check asked for now, and the database files.

use std::sync::Arc;

use api_types::application::DatabaseFile;
use api_types::state::VersionInfo;
use async_trait::async_trait;
use cascade::Watcher;
use tracing::{info, warn};

use crate::api::error::ApiError;
use crate::config::Options;
use crate::databases::Databases;
use crate::network::Network;
use crate::services::{ApplicationService, Connected};
use crate::state::StateOwner;
use crate::version::Releases;

/// The application service over the network, the release feed and the databases.
pub struct Application {
  /// the client and its watchdog
  pub network: Arc<Network>,
  /// the release feed
  pub releases: Arc<Releases>,
  /// where the release check's outcome goes
  pub state: StateOwner,
  /// the database files
  pub databases: Arc<Databases>,
  /// whether the release feed may be read at all
  pub config: Watcher<Options>,
}

#[async_trait]
impl ApplicationService for Application {
  async fn connect(&self) -> Result<Connected, ApiError> {
    Ok(self.network.connect())
  }

  async fn disconnect(&self, reason: Option<String>) -> Result<(), ApiError> {
    self.network.disconnect(reason).await;
    Ok(())
  }

  async fn check_version(&self) -> Result<VersionInfo, ApiError> {
    if self.config.current().values.flags.no_version_check {
      info!("Skipping version check; the no-version-check flag is set");
      return Ok(self.state.current().version.clone());
    }
    self.releases.update(&self.state).await.map_err(|e| {
      warn!(error = e, "Failed to check version");
      ApiError::upstream("Could not check the version", Some(e))
    })
  }

  async fn databases(&self) -> Result<Vec<DatabaseFile>, ApiError> {
    self.databases.databases().await
  }

  async fn vacuum(&self, name: &str) -> Result<(), ApiError> {
    self.databases.vacuum(name).await
  }
}
