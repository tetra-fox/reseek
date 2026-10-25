//! What we share: the shares of every host and the scan that indexes them.

use api_types::shares::{Share, SharesByHost};
use api_types::users::Directory;
use async_trait::async_trait;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "shares";

/// The shares and their scan.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait ShareService: Send + Sync {
  /// The shares of every host, this one listed as `local` even when it has none.
  async fn shares(&self) -> Result<SharesByHost, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The share with the id `id` on any host.
  async fn share(&self, id: &str) -> Result<Share, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Everything every host shares, as the network sees it.
  async fn contents(&self) -> Result<Vec<Directory>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// What the share with the id `id` holds, its own directory included.
  async fn share_contents(&self, id: &str) -> Result<Vec<Directory>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Starts a scan of this host's shares in the background, reading every file again when
  /// `force` is set. A scan already running is a conflict.
  async fn scan(&self, force: bool) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Asks the running scan to stop. No scan running is a conflict.
  async fn cancel_scan(&self) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }
}
