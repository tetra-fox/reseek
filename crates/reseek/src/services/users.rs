//! Other users: what the network says about them, and the directory of users we know.

use api_types::profile::Interests;
use api_types::users::{
  BrowseProgress, BrowseResponse, BuddyChange, CachedUser, Directory, StatisticsSnapshot,
  UserEndpoint, UserInfo, UserList, UserListQuery, UserProfile, UserStatus, UserSummary,
  UserTransfers, UserTransfersQuery,
};
use async_trait::async_trait;
use axum::response::Response;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "users";

/// Asking the server and peers about users, and the users directory with what we keep about
/// each user. A user who is offline, and one who is blocked from it, is a 404 with the same
/// message.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait UserService: Send + Sync {
  /// The address the server gives for a user.
  async fn endpoint(&self, username: &str) -> Result<UserEndpoint, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Everything a user shares, asked of the user.
  async fn browse(&self, username: &str) -> Result<BrowseResponse, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// How far the running or just finished browse of a user got; 404 when there is none.
  async fn browse_status(&self, username: &str) -> Result<BrowseProgress, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The contents of one of a user's directories, named exactly as their browse gave it, asked
  /// of the user. The answer may hold its subdirectories as well.
  async fn directory(&self, username: &str, directory: String) -> Result<Vec<Directory>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A user's description, picture and upload slots, asked of the user.
  async fn info(&self, username: &str) -> Result<UserInfo, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A user's presence, asked of the server.
  async fn status(&self, username: &str) -> Result<UserStatus, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A user's share and speed figures, asked of the server.
  async fn statistics(&self, username: &str) -> Result<StatisticsSnapshot, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// What a user likes and hates, asked of the server.
  async fn interests(&self, username: &str) -> Result<Interests, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Asks the server to report a user's presence and figures as they change.
  async fn watch(&self, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Stops watching a user; a conflict while the user has uploads that have not completed.
  async fn unwatch(&self, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The users being watched.
  async fn watched(&self) -> Result<Vec<String>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// What is remembered of the users seen since the start.
  async fn cache(&self) -> Result<Vec<CachedUser>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of the users directory.
  async fn list(&self, query: UserListQuery) -> Result<UserList, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// What is stored about a user, without asking the network.
  async fn summary(&self, username: &str) -> Result<UserSummary, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A user's profile, refreshed from the network first when asked; 404 only for a user we do
  /// not know and the server says does not exist.
  async fn profile(&self, username: &str, refresh: bool) -> Result<UserProfile, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A user's stored picture as the whole response, with its type and caching headers.
  async fn picture(&self, username: &str) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Replaces our notes on a user.
  async fn set_notes(&self, username: &str, notes: String) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Replaces our tags on a user; each is lower case already.
  async fn set_tags(&self, username: &str, tags: Vec<String>) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Makes a user our buddy, or changes how; a field the change leaves out keeps its value.
  /// Our own account is a conflict.
  async fn set_buddy(&self, username: &str, change: BuddyChange) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Takes a user off our buddy list.
  async fn remove_buddy(&self, username: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Mutes a user's notifications, or unmutes them.
  async fn set_muted(&self, username: &str, muted: bool) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Puts a user in a user-defined upload group, or takes them out of the one they were put
  /// in. A user the configuration lists for another group is a conflict.
  async fn set_group(&self, username: &str, group: Option<String>) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A page of a user's transfers, newest first, removed ones included.
  async fn transfers(
    &self,
    username: &str,
    query: UserTransfersQuery,
  ) -> Result<UserTransfers, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Forgets a user and their block; with `purge` also removes their transfers from the
  /// listings and deletes the conversation with them.
  async fn remove(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }
}
