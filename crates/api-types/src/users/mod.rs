//! Other users: their presence, statistics, info and address, browsing their shares, the
//! known-users directory with its profiles, and the bodies and queries of the user calls.

pub mod blocking;
mod profiles;

pub use profiles::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::search::File;
use crate::state::UserStatistics;

/// A user's online state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum UserPresence {
  /// not logged in
  Offline,
  /// logged in and marked away
  Away,
  /// logged in and not away
  Online,
}

impl From<soulseek_proto::UserPresence> for UserPresence {
  fn from(presence: soulseek_proto::UserPresence) -> Self {
    match presence {
      soulseek_proto::UserPresence::Offline => Self::Offline,
      soulseek_proto::UserPresence::Away => Self::Away,
      soulseek_proto::UserPresence::Online => Self::Online,
    }
  }
}

/// A user's presence and privilege state as the server reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserStatus {
  /// whether the user is online, away or offline
  pub presence: UserPresence,
  /// the user holds privileges
  pub is_privileged: bool,
}

/// A user's statistics with when the server sent them, as `GET /users/{username}/statistics`
/// and the user cache return them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsSnapshot {
  /// the statistics
  #[serde(flatten)]
  pub statistics: UserStatistics,
  /// when the server's reply or push arrived; statistics of a watched user go stale between
  /// presence changes
  pub updated_at: DateTime<Utc>,
}

/// What a peer says about itself, as `GET /users/{username}/info` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
  /// the peer's free text, may be empty
  pub description: String,
  /// the peer sent a picture
  pub has_picture: bool,
  /// the picture's bytes as the peer sent them, base64; null without one
  pub picture: Option<String>,
  /// the peer's upload slots
  pub upload_slots: u32,
  /// how many uploads the peer has queued
  pub queue_length: u32,
  /// the peer has an upload slot free
  pub has_free_upload_slot: bool,
  /// who may upload to the peer: 0 no one, 1 everyone, 2 users in their list, 3 users they
  /// permitted; null when the peer did not say
  pub upload_permitted: Option<u32>,
}

/// The address a user's client listens on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserEndpoint {
  /// the IPv4 address, dotted
  #[schema(example = "203.0.113.9")]
  pub address: String,
  /// the listen port
  #[schema(example = 2234)]
  pub port: u16,
  /// the obfuscation type the server reported, 0 for none and 1 for rotated; null when it
  /// sent none. Shown for diagnostics, never dialed
  pub obfuscation_type: Option<u32>,
  /// the obfuscated listen port, null when the server sent none
  pub obfuscated_port: Option<u16>,
}

/// An address and port, with no obfuscation known: the client keeps only those of a user.
impl From<std::net::SocketAddr> for UserEndpoint {
  fn from(endpoint: std::net::SocketAddr) -> Self {
    Self {
      address: endpoint.ip().to_string(),
      port: endpoint.port(),
      obfuscation_type: None,
      obfuscated_port: None,
    }
  }
}

/// How far the running browse of a user has received its reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowseProgress {
  /// the user being browsed
  pub username: String,
  /// bytes of the reply received
  pub bytes_transferred: u64,
  /// bytes of the reply still to come
  pub bytes_remaining: u64,
  /// how much of the reply arrived, from 0 to 100
  pub percent_complete: f64,
  /// the size of the whole reply in bytes
  pub size: u64,
}

/// A user's whole share listing, as `GET /users/{username}/browse` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowseResponse {
  /// the directories we may download from, in the peer's order, each subdirectory its own
  /// entry
  pub directories: Vec<Directory>,
  /// how many entries `directories` has
  pub directory_count: u32,
  /// the directories the peer shares only with some users
  pub locked_directories: Vec<Directory>,
  /// how many entries `lockedDirectories` has
  pub locked_directory_count: u32,
}

/// A shared directory and the files directly in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
  /// the full remote path with the peer's separators kept exactly; send it back unchanged
  #[schema(example = "@@abcd\\Music\\Pink Floyd\\Animals")]
  pub name: String,
  /// how many entries `files` has
  pub file_count: u32,
  /// the files, each `filename` the bare name
  pub files: Vec<File>,
}

/// A directory as a peer listed it.
impl From<soulseek_proto::peer::Directory> for Directory {
  fn from(directory: soulseek_proto::peer::Directory) -> Self {
    let files: Vec<File> = directory.files.into_iter().map(File::from).collect();
    Self {
      name: directory.name.as_str().to_string(),
      file_count: u32::try_from(files.len()).unwrap_or(u32::MAX),
      files,
    }
  }
}

/// Asks a user for the contents of one directory, the body of
/// `POST /users/{username}/directory`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryRequest {
  /// the directory's `name` exactly as a browse or search response gave it
  #[schema(example = "@@abcd\\Music\\Pink Floyd\\Animals")]
  pub directory: String,
}

/// A user the in-memory user cache holds, as `GET /users/cache` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CachedUser {
  /// the user
  pub username: String,
  /// the user-defined group the configuration lists the user in, null when none
  pub group: Option<String>,
  /// the last statistics fetched or pushed, null before any
  pub statistics: Option<StatisticsSnapshot>,
  /// the last status fetched or pushed, null before any
  pub status: Option<UserStatus>,
}

/// A user's presence changed, the data of a `users.status` frame. Sent only for our buddies and
/// users with an open conversation or a transfer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserStatusChanged {
  /// the user
  pub username: String,
  /// the new status
  pub status: UserStatus,
}

/// Names a user by query parameter instead of by path segment, for usernames such as `.` and
/// `..` that a path cannot carry. When present it wins over the path's username.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct UsernameQuery {
  /// the username, exactly
  pub username: Option<String>,
}
