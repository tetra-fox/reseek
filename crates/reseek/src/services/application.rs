//! Connecting to the Soulseek server and checking for a newer release.

use api_types::state::VersionInfo;
use async_trait::async_trait;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "application";

/// What asking to connect did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connected {
  /// the client was not trying to connect and now is
  Started,
  /// the client was already trying and starts over at once instead of waiting out its back-off
  Restarted,
}

/// The connection to the Soulseek server, which the client keeps up on its own once started,
/// and the release check.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait ApplicationService: Send + Sync {
  /// Starts connecting to the server and logging in, and keeps reconnecting after a lost
  /// connection.
  async fn connect(&self) -> Result<Connected, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Disconnects from the server with `reason`, and stops reconnecting until asked to connect
  /// again.
  async fn disconnect(&self, reason: Option<String>) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Reads the release feed now and returns the version state it leads to.
  async fn check_version(&self) -> Result<VersionInfo, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
