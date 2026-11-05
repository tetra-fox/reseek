//! The user calls: what the network says about a user, the users directory, and blocking.

use api_types::messaging::{ExportFormat, ExportQuery};
use api_types::users::blocking::{
  Block, BlockImportResult, BlockRequest, BlockedQuery, BlockedUsers,
};
use api_types::users::{
  BrowseProgress, BrowseResponse, BuddyChange, CachedUser, DeleteUserQuery, Directory, DirectoryRequest,
  GroupAssignment, ProfileQuery, StatisticsSnapshot, UserEndpoint, UserInfo, UserList,
  UserListQuery, UserNotes, UserProfile, UserStatus, UserSummary, UserTags, UserTransfers,
  UserTransfersQuery,
};
use bytes::Bytes;

use super::{Client, Result, Route, bytes, done, json, text};

/// The route of something about one user.
fn about(username: &str, what: &str) -> Route {
  Route::new("users").user(username).then(what)
}

impl Client {
  /// `GET /users/{username}/endpoint`: a user's address.
  pub async fn endpoint(&self, username: &str) -> Result<UserEndpoint> {
    json(self.get(about(username, "endpoint"))).await
  }

  /// `GET /users/{username}/browse`: everything a user shares.
  pub async fn browse(&self, username: &str) -> Result<BrowseResponse> {
    json(self.get(about(username, "browse"))).await
  }

  /// `GET /users/{username}/browse/status`: how far a browse got.
  pub async fn browse_status(&self, username: &str) -> Result<BrowseProgress> {
    json(self.get(about(username, "browse/status"))).await
  }

  /// `POST /users/{username}/directory`: one of a user's directories.
  pub async fn directory(&self, username: &str, directory: &str) -> Result<Vec<Directory>> {
    let body = DirectoryRequest {
      directory: directory.to_string(),
    };
    json(self.post(about(username, "directory")).json(&body)).await
  }

  /// `GET /users/{username}/info`: a user's description and upload slots.
  pub async fn user_info(&self, username: &str) -> Result<UserInfo> {
    json(self.get(about(username, "info"))).await
  }

  /// `GET /users/{username}/status`: a user's presence.
  pub async fn user_status(&self, username: &str) -> Result<UserStatus> {
    json(self.get(about(username, "status"))).await
  }

  /// `GET /users/{username}/statistics`: a user's share and speed figures.
  pub async fn user_statistics(&self, username: &str) -> Result<StatisticsSnapshot> {
    json(self.get(about(username, "statistics"))).await
  }

  /// `PUT /users/{username}/watch`: watches a user.
  pub async fn watch(&self, username: &str) -> Result<()> {
    done(self.put(about(username, "watch"))).await
  }

  /// `DELETE /users/{username}/watch`: stops watching a user.
  pub async fn unwatch(&self, username: &str) -> Result<()> {
    done(self.delete(about(username, "watch"))).await
  }

  /// `GET /users/watched`: the users being watched.
  pub async fn watched(&self) -> Result<Vec<String>> {
    json(self.get(Route::new("users/watched"))).await
  }

  /// `GET /users/cache`: what is remembered of the users seen since the start.
  pub async fn user_cache(&self) -> Result<Vec<CachedUser>> {
    json(self.get(Route::new("users/cache"))).await
  }

  /// `GET /users`: a page of the users directory.
  pub async fn users(&self, query: &UserListQuery) -> Result<UserList> {
    json(self.get(Route::new("users").query(query)?)).await
  }

  /// `GET /users/{username}/summary`: what we keep about a user.
  pub async fn user_summary(&self, username: &str) -> Result<UserSummary> {
    json(self.get(about(username, "summary"))).await
  }

  /// `GET /users/{username}/profile`: a user's profile, refreshed first with `refresh`.
  pub async fn user_profile(&self, username: &str, refresh: bool) -> Result<UserProfile> {
    json(self.get(about(username, "profile").query(&ProfileQuery { refresh })?)).await
  }

  /// `GET /users/{username}/picture`: a user's picture.
  pub async fn user_picture(&self, username: &str) -> Result<Bytes> {
    bytes(self.get(about(username, "picture"))).await
  }

  /// `PUT /users/{username}/notes`: replaces our notes on a user.
  pub async fn set_notes(&self, username: &str, notes: &str) -> Result<()> {
    let body = UserNotes {
      notes: notes.to_string(),
    };
    done(self.put(about(username, "notes")).json(&body)).await
  }

  /// `PUT /users/{username}/tags`: replaces our tags on a user.
  pub async fn set_tags(&self, username: &str, tags: &[String]) -> Result<()> {
    let body = UserTags {
      tags: tags.to_vec(),
    };
    done(self.put(about(username, "tags")).json(&body)).await
  }

  /// `PUT /users/{username}/buddy`: makes a user our buddy, or changes how.
  pub async fn set_buddy(&self, username: &str, change: &BuddyChange) -> Result<()> {
    done(self.put(about(username, "buddy")).json(change)).await
  }

  /// `DELETE /users/{username}/buddy`: takes a user off our buddy list.
  pub async fn remove_buddy(&self, username: &str) -> Result<()> {
    done(self.delete(about(username, "buddy"))).await
  }

  /// `PUT` or `DELETE /users/{username}/mute`: mutes or unmutes a user.
  pub async fn set_muted(&self, username: &str, muted: bool) -> Result<()> {
    let route = about(username, "mute");
    done(if muted {
      self.put(route)
    } else {
      self.delete(route)
    })
    .await
  }

  /// `PUT /users/{username}/group`: puts a user in a user-defined group, or out of it.
  pub async fn set_group(&self, username: &str, group: Option<&str>) -> Result<()> {
    let body = GroupAssignment {
      group: group.map(str::to_string),
    };
    done(self.put(about(username, "group")).json(&body)).await
  }

  /// `GET /users/{username}/transfers`: a page of a user's transfers.
  pub async fn transfers_of(
    &self,
    username: &str,
    query: &UserTransfersQuery,
  ) -> Result<UserTransfers> {
    json(self.get(about(username, "transfers").query(query)?)).await
  }

  /// `DELETE /users/{username}`: forgets a user, and with `purge` their transfers and
  /// conversation.
  pub async fn forget_user(&self, username: &str, purge: bool) -> Result<()> {
    let route = Route::new("users")
      .user(username)
      .query(&DeleteUserQuery { purge })?;
    done(self.delete(route)).await
  }

  /// `GET /users/blocked`: who is blocked and by what rule.
  pub async fn blocked(&self, include_config: bool) -> Result<BlockedUsers> {
    let route = Route::new("users/blocked").query(&BlockedQuery { include_config })?;
    json(self.get(route)).await
  }

  /// `PUT /users/{username}/block`: blocks a user.
  pub async fn block(&self, username: &str, request: &BlockRequest) -> Result<Block> {
    json(self.put(about(username, "block")).json(request)).await
  }

  /// `DELETE /users/{username}/block`: unblocks a user.
  pub async fn unblock(&self, username: &str) -> Result<()> {
    done(self.delete(about(username, "block"))).await
  }

  /// `POST /users/blocked/import`: blocks every user named, one per line.
  pub async fn import_blocked_names(&self, names: &[String]) -> Result<BlockImportResult> {
    let request = self
      .post(Route::new("users/blocked/import"))
      .header("content-type", "text/plain")
      .body(names.join("\n"));
    json(request).await
  }

  /// `POST /users/blocked/import`: blocks every user of exported blocks.
  pub async fn import_blocks(&self, blocks: &[Block]) -> Result<BlockImportResult> {
    json(self.post(Route::new("users/blocked/import")).json(blocks)).await
  }

  /// `GET /users/blocked/export?format=json`: the blocks made through the API.
  pub async fn export_blocks(&self) -> Result<Vec<Block>> {
    let query = ExportQuery {
      format: ExportFormat::Json,
    };
    json(self.get(Route::new("users/blocked/export").query(&query)?)).await
  }

  /// `GET /users/blocked/export?format=text`: the blocked usernames, one per line.
  pub async fn export_blocked_names(&self) -> Result<String> {
    let query = ExportQuery {
      format: ExportFormat::Text,
    };
    text(self.get(Route::new("users/blocked/export").query(&query)?)).await
  }
}
