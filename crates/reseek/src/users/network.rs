//! The users service: what the network says about a user, asked of the server or the user's
//! client (their address, their shares and one of their folders, their info, presence and
//! statistics, and watching them), and the users directory, which the [`Directory`] answers. A
//! user the configuration's blacklist names is answered as offline, so the two look the same.
//! Asking about a user's presence, info or shares asks the server for their statistics again
//! too, since the server does not push those.

use std::sync::Arc;
use std::time::Duration;

use api_types::users::{
  BrowseProgress, BrowseResponse, CachedUser, Directory as SharedDirectory, StatisticsSnapshot,
  UserEndpoint, UserInfo, UserList, UserListQuery, UserProfile, UserStatus, UserSummary,
  UserTransfers, UserTransfersQuery,
};
use async_trait::async_trait;
use axum::response::Response;
use base64::Engine;
use cascade::Watcher;
use soulseek_client::{BrowseOptions, ClientError, TransferState};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::browse::Browses;
use super::{Directory, PeerInfo, Touch};
use crate::api::error::ApiError;
use crate::config::Options;
use crate::live::Live;
use crate::messaging::Messaging;
use crate::network::Network;
use crate::services::UserService;

/// How long a user has to send the contents of one folder.
const FOLDER_TIMEOUT: Duration = Duration::from_secs(5);
/// How long refreshing a profile waits for the network at most.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(5);

/// The users service.
pub struct Users {
  directory: Arc<Directory>,
  network: Arc<Network>,
  messaging: Arc<Messaging>,
  config: Watcher<Options>,
  browses: Arc<Browses>,
}

/// The answer to asking the network about `username` that failed with `error`.
fn unanswered(username: &str, error: ClientError) -> ApiError {
  match error {
    ClientError::UserOffline(_) => ApiError::offline(username),
    ClientError::UserNotFound(message) => ApiError::not_found(message),
    ClientError::InvalidOperation(detail) => ApiError::unavailable(Some(detail)),
    other => ApiError::upstream(format!("Could not ask {username}"), Some(other.to_string())),
  }
}

/// What a peer said about itself, as the API shows it.
fn info(info: &soulseek_proto::peer::UserInfo) -> UserInfo {
  UserInfo {
    description: info.description.clone(),
    has_picture: info.has_picture(),
    picture: info
      .picture
      .as_ref()
      .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)),
    upload_slots: info.upload_slots,
    queue_length: info.queue_length,
    has_free_upload_slot: info.has_free_upload_slot,
    upload_permitted: info.upload_permitted,
  }
}

/// A share listing as the API shows it.
fn listing(response: soulseek_proto::peer::BrowseResponse) -> BrowseResponse {
  let directories: Vec<SharedDirectory> = response
    .directories
    .into_iter()
    .map(SharedDirectory::from)
    .collect();
  let locked_directories: Vec<SharedDirectory> = response
    .locked_directories
    .into_iter()
    .map(|directory| {
      let mut directory = SharedDirectory::from(directory);
      for file in &mut directory.files {
        file.is_locked = true;
      }
      directory
    })
    .collect();
  BrowseResponse {
    directory_count: u32::try_from(directories.len()).unwrap_or(u32::MAX),
    directories,
    locked_directory_count: u32::try_from(locked_directories.len()).unwrap_or(u32::MAX),
    locked_directories,
  }
}

impl Users {
  /// The service over `directory`, asking the network through `network`, closing the
  /// conversations of users forgotten through `messaging`, and pushing browse progress to
  /// `live`.
  pub fn new(
    directory: Arc<Directory>,
    network: Arc<Network>,
    messaging: Arc<Messaging>,
    config: Watcher<Options>,
    live: Live,
  ) -> Self {
    Self {
      directory,
      network,
      messaging,
      config,
      browses: Arc::new(Browses::new(live)),
    }
  }

  /// Refuses a user the configuration's blacklist names, as if they were offline.
  fn reachable(&self, username: &str) -> Result<(), ApiError> {
    if self.directory.configuration_blocks(username) {
      debug!(
        username,
        "user the configuration blocks answered as offline"
      );
      return Err(ApiError::offline(username));
    }
    Ok(())
  }

  /// Asks `username`'s client for its info, keeping and touching what it said.
  async fn fetch_info(&self, username: &str) -> Result<UserInfo, ClientError> {
    let fetched = self
      .network
      .client()
      .get_user_info(username, &CancellationToken::new())
      .await?;
    let shown = info(&fetched);
    let kept = PeerInfo {
      description: fetched.description.clone(),
      has_picture: fetched.has_picture(),
      picture: fetched.picture,
      upload_slots: fetched.upload_slots,
      queue_length: fetched.queue_length,
      has_free_slot: fetched.has_free_upload_slot,
    };
    self.directory.touch(username, Touch::Info(kept)).await;
    Ok(shown)
  }

  /// `username`'s profile after asking the network for their status, statistics and info,
  /// each on its own and all within five seconds; what could not be asked is the stored value,
  /// marked stale. A user nobody stored who the server says does not exist is a 404, and is
  /// asked about first: the server answers statistics for any name, and keeping those would
  /// store a row for a user who does not exist.
  async fn refreshed_profile(&self, username: &str) -> Result<UserProfile, ApiError> {
    let mut profile = self.directory.profile(username).await?;
    if !profile.user.known && !self.exists(username).await {
      return Err(ApiError::not_found(format!(
        "User {username} does not exist"
      )));
    }
    let client = self.network.client();
    let (status, statistics, fetched) = tokio::join!(
      tokio::time::timeout(REFRESH_TIMEOUT, self.directory.status_of(client, username)),
      tokio::time::timeout(
        REFRESH_TIMEOUT,
        self.directory.statistics_of(client, username)
      ),
      tokio::time::timeout(REFRESH_TIMEOUT, self.fetch_info(username)),
    );
    let (status, statistics, fetched) = (
      refreshed("status", username, status),
      refreshed("statistics", username, statistics),
      refreshed("info", username, fetched),
    );
    let now = chrono::Utc::now();
    if let Some(status) = status {
      profile.user.presence = Some(status.presence);
      profile.user.presence_at = Some(now);
      profile.user.is_privileged = status.is_privileged;
      let touch = Touch::Status {
        presence: status.presence,
        is_privileged: status.is_privileged,
      };
      self.directory.touch(username, touch).await;
    }
    match (statistics, profile.user.statistics.as_mut()) {
      (Some(fresh), _) => {
        profile.user.statistics = Some(api_types::users::StoredStatistics {
          statistics: fresh.statistics,
          fetched_at: fresh.updated_at,
          stale: false,
        });
      }
      (None, Some(stored)) => stored.stale = true,
      (None, None) => {}
    }
    match (fetched, profile.info.as_mut()) {
      (Some(fresh), _) => {
        profile.info = Some(api_types::users::StoredInfo {
          description: fresh.description,
          info: api_types::users::StoredInfoSummary {
            has_picture: fresh.has_picture,
            picture_url: fresh
              .has_picture
              .then(|| super::profile::picture_url(username)),
            upload_slots: fresh.upload_slots,
            queue_length: fresh.queue_length,
            has_free_upload_slot: fresh.has_free_upload_slot,
            fetched_at: now,
            stale: false,
          },
        });
      }
      (None, Some(stored)) => stored.info.stale = true,
      (None, None) => {}
    }
    Ok(profile)
  }

  /// Whether the server knows `username`: watching them is the one question it answers no to.
  /// A user the server did not watch for us before is not watched afterwards.
  async fn exists(&self, username: &str) -> bool {
    let client = self.network.client();
    match client.watch_user(username).await {
      Err(ClientError::UserNotFound(_)) => false,
      Err(e) => {
        debug!(username, error = %e, "whether the user exists not asked; taken as existing");
        true
      }
      Ok(_) => {
        if !self.directory.is_watched(username)
          && let Err(e) = client.unwatch_user(username).await
        {
          debug!(username, error = %e, "user watched to ask whether they exist not unwatched");
        }
        true
      }
    }
  }
}

#[async_trait]
impl UserService for Users {
  async fn endpoint(&self, username: &str) -> Result<UserEndpoint, ApiError> {
    self.reachable(username)?;
    let endpoint = self
      .network
      .client()
      .get_user_endpoint(username)
      .await
      .map_err(|e| unanswered(username, e))?;
    Ok(endpoint.into())
  }

  async fn browse(&self, username: &str) -> Result<BrowseResponse, ApiError> {
    self.reachable(username)?;
    self.directory.touch(username, Touch::Browse).await;
    self.directory.refetch_statistics(username);
    let browsing = self.browses.started(username);
    let browse = browsing.browse;
    let browses = self.browses.clone();
    let options = BrowseOptions {
      response_timeout: self
        .config
        .current()
        .values
        .soulseek
        .browse
        .response_timeout,
      progress_updated: Some(Arc::new(
        move |progress: &soulseek_client::BrowseProgress| {
          browses.progressed(browse, progress);
        },
      )),
    };
    let result = self
      .network
      .client()
      .browse(username, options, &CancellationToken::new())
      .await;
    drop(browsing);
    let response = result.map_err(|e| unanswered(username, e))?;
    debug!(
      username,
      directories = response.directories.len(),
      "user browsed"
    );
    Ok(listing(response))
  }

  async fn browse_status(&self, username: &str) -> Result<BrowseProgress, ApiError> {
    self
      .browses
      .status(username)
      .ok_or_else(|| ApiError::not_found(format!("No browse of {username} is in progress")))
  }

  async fn directory(
    &self,
    username: &str,
    directory: String,
  ) -> Result<Vec<SharedDirectory>, ApiError> {
    self.reachable(username)?;
    let directories = self
      .network
      .client()
      .get_directory_contents(
        username,
        directory,
        None,
        Some(FOLDER_TIMEOUT),
        &CancellationToken::new(),
      )
      .await
      .map_err(|e| unanswered(username, e))?;
    Ok(directories.into_iter().map(SharedDirectory::from).collect())
  }

  async fn info(&self, username: &str) -> Result<UserInfo, ApiError> {
    self.reachable(username)?;
    self.directory.refetch_statistics(username);
    self
      .fetch_info(username)
      .await
      .map_err(|e| unanswered(username, e))
  }

  async fn status(&self, username: &str) -> Result<UserStatus, ApiError> {
    self.reachable(username)?;
    let status = self
      .directory
      .status_of(self.network.client(), username)
      .await
      .map_err(|e| unanswered(username, e))?;
    self.directory.refetch_statistics(username);
    Ok(status)
  }

  async fn statistics(&self, username: &str) -> Result<StatisticsSnapshot, ApiError> {
    self.reachable(username)?;
    self
      .directory
      .statistics_of(self.network.client(), username)
      .await
      .map_err(|e| unanswered(username, e))
  }

  async fn watch(&self, username: &str) -> Result<(), ApiError> {
    self
      .directory
      .watch_again(self.network.client(), username)
      .await
      .map_err(|e| unanswered(username, e))
  }

  async fn unwatch(&self, username: &str) -> Result<(), ApiError> {
    let client = self.network.client();
    let uploading = client
      .uploads()
      .iter()
      .any(|t| t.username == username && !t.state.contains(TransferState::COMPLETED));
    if uploading {
      return Err(ApiError::conflict(
        format!("{username} has uploads that have not completed"),
        Some("Their upload group requires their status to be kept current".to_string()),
      ));
    }
    self
      .directory
      .unwatch(client, username)
      .await
      .map_err(|e| unanswered(username, e))
  }

  async fn watched(&self) -> Result<Vec<String>, ApiError> {
    Ok(self.directory.watched_users())
  }

  async fn cache(&self) -> Result<Vec<CachedUser>, ApiError> {
    Ok(self.directory.cached_users())
  }

  async fn list(&self, query: UserListQuery) -> Result<UserList, ApiError> {
    self.directory.list(query).await
  }

  async fn summary(&self, username: &str) -> Result<UserSummary, ApiError> {
    self.directory.summary(username).await
  }

  async fn profile(&self, username: &str, refresh: bool) -> Result<UserProfile, ApiError> {
    if refresh {
      self.refreshed_profile(username).await
    } else {
      self.directory.profile(username).await
    }
  }

  async fn picture(&self, username: &str) -> Result<Response, ApiError> {
    self.directory.picture(username).await
  }

  async fn set_notes(&self, username: &str, notes: String) -> Result<(), ApiError> {
    self.directory.set_notes(username, notes).await
  }

  async fn set_tags(&self, username: &str, tags: Vec<String>) -> Result<(), ApiError> {
    self.directory.set_tags(username, tags).await
  }

  async fn set_pinned(&self, username: &str, pinned: bool) -> Result<(), ApiError> {
    self.directory.set_pinned(username, pinned).await
  }

  async fn set_muted(&self, username: &str, muted: bool) -> Result<(), ApiError> {
    self.directory.set_muted(username, muted).await
  }

  async fn set_group(&self, username: &str, group: Option<String>) -> Result<(), ApiError> {
    self.directory.set_group(username, group).await
  }

  async fn transfers(
    &self,
    username: &str,
    query: UserTransfersQuery,
  ) -> Result<UserTransfers, ApiError> {
    self.directory.transfers(username, query).await
  }

  async fn remove(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    self.directory.remove(username, purge).await?;
    if purge {
      self.messaging.purge(username).await?;
    }
    Ok(())
  }
}

/// What asking for `what` of `username` within the refresh's time gave; `None`, with the
/// reason logged, when it failed or took too long, and the stored value is shown instead.
fn refreshed<T>(
  what: &str,
  username: &str,
  asked: Result<Result<T, ClientError>, tokio::time::error::Elapsed>,
) -> Option<T> {
  match asked {
    Ok(Ok(value)) => Some(value),
    Ok(Err(e)) => {
      debug!(username, what, error = %e, "profile part not refreshed");
      None
    }
    Err(_) => {
      debug!(username, what, "profile part not refreshed in time");
      None
    }
  }
}
