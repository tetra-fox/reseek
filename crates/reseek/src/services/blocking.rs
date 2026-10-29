//! Blocked users.

use api_types::auth::Principal;
use api_types::users::blocking::{Block, BlockImportResult, BlockRequest, BlockedUsers};
use async_trait::async_trait;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "blocking";

/// The users to block, as an import gives them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockImport {
  /// usernames, each blocked with the default scopes
  Names(Vec<String>),
  /// whole blocks, as an export wrote them
  Blocks(Vec<Block>),
}

/// Blocking and unblocking users, and the lists of who is blocked.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait BlockingService: Send + Sync {
  /// The blocked users, with those the configuration blocks when asked for, and the
  /// configuration's blocking rules.
  async fn blocked(&self, include_config: bool) -> Result<BlockedUsers, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Blocks a user, or replaces their block, as `by`. `request` has passed the API's checks,
  /// and its `expiresIn` is read as `expires_in`. Blocking ourselves is a conflict.
  async fn block(
    &self,
    by: &Principal,
    username: &str,
    request: BlockRequest,
    expires_in: Option<std::time::Duration>,
  ) -> Result<Block, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Removes a user's block as `by`; 404 when they are not blocked, a conflict when the
  /// configuration blocks them.
  async fn unblock(&self, by: &Principal, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Blocks every user of `import` not blocked yet, as `by`; an expired block or one whose
  /// reason is too long is skipped.
  async fn import(
    &self,
    by: &Principal,
    import: BlockImport,
  ) -> Result<BlockImportResult, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Every block made through the API, for an export.
  async fn export(&self) -> Result<Vec<Block>, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
