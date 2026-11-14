//! Other users as the server sees them: status, statistics, privileges, and watching them so
//! their status changes are pushed to us.

use soulseek_proto::server::{
  GetStatusRequest, GetUserStatsRequest, UnwatchUser, UserPrivilegesRequest, UserStatistics,
  UserStatus, WatchUserRequest, WatchUserResponse, WatchedUser,
};
use soulseek_proto::{ServerCode, ServerRequest};
use tokio_util::sync::CancellationToken;

use super::Client;
use super::request::{non_blank, user_key};
use crate::error::ClientError;

impl Client {
  /// the presence of `username`; the server answers for any name. Once `cancel` fires it ends
  /// at once, with the reply when it had come by then
  pub async fn get_user_status(
    &self,
    username: &str,
    cancel: &CancellationToken,
  ) -> Result<UserStatus, ClientError> {
    non_blank("username", username)?;
    self
      .ask_within(
        &format!("get status for user {username}"),
        user_key(ServerCode::GetStatus, username),
        ServerRequest::GetStatus(GetStatusRequest {
          username: username.to_owned(),
        }),
        None,
        cancel,
      )
      .await
  }

  /// the upload speed and share counts of `username`, zeros for a name nobody has
  pub async fn get_user_statistics(&self, username: &str) -> Result<UserStatistics, ClientError> {
    non_blank("username", username)?;
    self
      .ask(
        &format!("get statistics for user {username}"),
        user_key(ServerCode::GetUserStats, username),
        ServerRequest::GetUserStats(GetUserStatsRequest {
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// whether `username` is privileged; the status of a watched user says the same
  pub async fn get_user_privileged(&self, username: &str) -> Result<bool, ClientError> {
    non_blank("username", username)?;
    self
      .ask(
        &format!("get privileges for user {username}"),
        user_key(ServerCode::UserPrivileges, username),
        ServerRequest::UserPrivileges(UserPrivilegesRequest {
          username: username.to_owned(),
        }),
      )
      .await
  }

  /// Watches `username`: the server pushes their status from now on, until the session ends.
  /// Returns their status and statistics, or fails with `UserNotFound` for a name nobody has.
  pub async fn watch_user(&self, username: &str) -> Result<WatchedUser, ClientError> {
    non_blank("username", username)?;
    let watched: WatchUserResponse = self
      .ask(
        &format!("watch user {username}"),
        user_key(ServerCode::WatchUser, username),
        ServerRequest::WatchUser(WatchUserRequest {
          username: username.to_owned(),
        }),
      )
      .await?;
    watched
      .user
      .ok_or_else(|| ClientError::UserNotFound(format!("User {username} does not exist")))
  }

  /// stops the status pushes a watch started
  pub async fn unwatch_user(&self, username: &str) -> Result<(), ClientError> {
    non_blank("username", username)?;
    self
      .tell(
        &format!("unwatch user {username}"),
        ServerRequest::UnwatchUser(UnwatchUser {
          username: username.to_owned(),
        }),
      )
      .await
  }
}
