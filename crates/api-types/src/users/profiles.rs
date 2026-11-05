//! The known-users directory: what we store about every user we interacted with, the summary
//! and profile built from it, and the calls that list and change it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{UserEndpoint, UserPresence};
use crate::messaging::conversations::ConversationSummary;
use crate::state::UserStatistics;
use crate::transfers::{SortOrder, Transfer, TransferDirection, TransferStateCategory};
use crate::users::blocking::Block;

/// Where the effective group of a user comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum GroupSource {
  /// the configuration lists the user in a user-defined group
  Config,
  /// the group was assigned to the user from the API
  Override,
  /// one of the built-in groups: privileged, leechers or default
  Builtin,
}

/// The upload group a user is in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserGroup {
  /// the group's name
  #[schema(example = "default")]
  pub name: String,
  /// why the user is in it
  pub source: GroupSource,
  /// the group's upload priority, a lower number served first
  pub priority: u32,
  /// the color the group is shown in, a CSS color name such as `violet`; null for none
  #[schema(example = "violet")]
  pub color: Option<String>,
}

/// The statistics stored for a user, with when they were fetched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredStatistics {
  /// the statistics
  #[serde(flatten)]
  pub statistics: UserStatistics,
  /// when they were fetched
  pub fetched_at: DateTime<Utc>,
  /// the refresh asked for failed, so these are the values from before
  pub stale: bool,
}

/// The info stored for a user, without the description.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredInfoSummary {
  /// the peer sent a picture
  pub has_picture: bool,
  /// where the stored picture is fetched, a URL that changes whenever the picture does; null
  /// when none is stored, as when pictures are not kept or this one is too large
  #[schema(
    example = "/api/v0/users/alice/picture?v=2cea274d0bedc39ec4ab6ba9e59ec889e3ed6fb56a1cf088a64d9b383378dc97"
  )]
  pub picture_url: Option<String>,
  /// the peer's upload slots
  pub upload_slots: u32,
  /// how many uploads the peer had queued
  pub queue_length: u32,
  /// the peer had an upload slot free
  pub has_free_upload_slot: bool,
  /// when the info was fetched
  pub fetched_at: DateTime<Utc>,
  /// the refresh asked for failed, so these are the values from before
  pub stale: bool,
}

/// The info stored for a user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredInfo {
  /// the peer's free text, may be empty
  pub description: String,
  /// the rest of the info
  #[serde(flatten)]
  pub info: StoredInfoSummary,
}

/// Counts of a user's transfers in one direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferCounts {
  /// completed successfully, removed ones included
  pub succeeded: u32,
  /// completed otherwise, removed ones included
  pub failed: u32,
  /// moving bytes or being set up
  pub active: u32,
  /// waiting in a queue
  pub queued: u32,
}

/// Counts of a user's transfers in one direction, with the bytes moved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferFigures {
  /// the counts
  #[serde(flatten)]
  pub counts: TransferCounts,
  /// bytes moved by the succeeded transfers
  pub bytes: u64,
}

/// Counts of our private messages with a user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageCounts {
  /// from the user
  #[serde(rename = "in")]
  pub inbound: u32,
  /// to the user
  #[serde(rename = "out")]
  pub outbound: u32,
  /// from the user and not acknowledged
  pub unread: u32,
}

/// What we did with a user, computed when asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserFigures {
  /// our downloads from the user
  pub downloads: TransferFigures,
  /// our uploads to the user
  pub uploads: TransferFigures,
  /// when the latest transfer either way was asked for, null without one
  pub last_transfer_at: Option<DateTime<Utc>>,
  /// our private messages with the user
  pub messages: MessageCounts,
  /// when the latest private message either way was sent, null without one
  pub last_message_at: Option<DateTime<Utc>>,
}

/// What we did with a user, without the byte totals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryFigures {
  /// our downloads from the user
  pub downloads: TransferCounts,
  /// our uploads to the user
  pub uploads: TransferCounts,
  /// when the latest transfer either way was asked for, null without one
  pub last_transfer_at: Option<DateTime<Utc>>,
  /// our private messages with the user
  pub messages: MessageCounts,
  /// when the latest private message either way was sent, null without one
  pub last_message_at: Option<DateTime<Utc>>,
}

/// What the known-users directory holds about a user, shared by the summary and the profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnownUser {
  /// the user
  #[schema(example = "alice")]
  pub username: String,
  /// the directory has a row for the user; when false every stored field is null, zero or
  /// empty
  pub known: bool,
  /// when we first interacted with the user, null when not known
  pub first_seen_at: Option<DateTime<Utc>>,
  /// when we last interacted with the user, null when not known
  pub last_seen_at: Option<DateTime<Utc>>,
  /// the last presence observed, null when never observed
  pub presence: Option<UserPresence>,
  /// when `presence` was observed, null when never
  pub presence_at: Option<DateTime<Utc>>,
  /// the server last reported the user as privileged
  pub is_privileged: bool,
  /// the last statistics fetched, null when never
  pub statistics: Option<StoredStatistics>,
  /// the user's country as ISO 3166-1 alpha-2, from a room member list; null when never seen
  #[schema(example = "DE")]
  pub country_code: Option<String>,
  /// the upload group the user is in
  pub group: UserGroup,
  /// the user-defined group we put the user in, null when none; kept also when that group no
  /// longer exists, so a client can say the override has no effect
  pub group_override: Option<String>,
  /// our block of the user, or the configuration's; null when not blocked
  pub block: Option<Block>,
  /// our labels for the user, lower case
  pub tags: Vec<String>,
  /// chat notifications for the user are off
  pub muted: bool,
  /// the user is listed first
  pub pinned: bool,
  /// the user's address from the endpoint cache, null when not cached; never a round trip
  pub endpoint: Option<UserEndpoint>,
  /// our private conversation with the user, null when there is none
  pub conversation: Option<ConversationSummary>,
}

/// A user as the directory lists them, the data of a `users.updated` frame and the reply of
/// `GET /users/{username}/summary`. Built from stored data only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
  /// what the directory holds
  #[serde(flatten)]
  pub user: KnownUser,
  /// the last info fetched without the description, null when never
  pub info: Option<StoredInfoSummary>,
  /// the first three lines of the description in the last info fetched, null when never
  /// fetched
  pub description: Option<String>,
  /// the first two lines of our private notes about the user, null when there are none
  pub notes: Option<String>,
  /// what we did with the user
  pub figures: SummaryFigures,
  /// private messages from the user not acknowledged
  pub unread_messages: u32,
  /// transfers with the user that are queued or moving
  pub active_transfers: u32,
}

/// Everything known about a user, the reply of `GET /users/{username}/profile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
  /// what the directory holds
  #[serde(flatten)]
  pub user: KnownUser,
  /// the last info fetched, null when never
  pub info: Option<StoredInfo>,
  /// our private notes about the user
  pub notes: String,
  /// what we did with the user
  pub figures: UserFigures,
}

/// What the user directory is sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum UserSort {
  /// the last interaction, latest first by default
  #[default]
  LastSeen,
  /// the username
  Username,
  /// the first interaction
  FirstSeen,
  /// the succeeded uploads
  Uploads,
  /// the succeeded downloads
  Downloads,
}

fn default_user_limit() -> u32 {
  50
}

/// The query of `GET /users`, the known-users directory. Every filter given must match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct UserListQuery {
  /// a substring of the username, a tag or the notes, matched ignoring case
  pub q: Option<String>,
  /// only blocked users, or only users not blocked
  pub blocked: Option<bool>,
  /// only users with this tag
  pub tag: Option<String>,
  /// only users in this group
  pub group: Option<String>,
  /// only pinned users, or only users not pinned
  pub pinned: Option<bool>,
  /// only users last seen with this presence
  pub presence: Option<UserPresence>,
  /// only users with transfers, or only users without
  pub has_transfers: Option<bool>,
  /// what to sort by
  #[serde(default)]
  pub sort: UserSort,
  /// the order, the sort's own default when left out
  pub order: Option<SortOrder>,
  /// how many users to skip
  #[serde(default)]
  pub offset: u32,
  /// how many users to return, at most 500
  #[serde(default = "default_user_limit")]
  #[param(default = 50, maximum = 500)]
  pub limit: u32,
}

/// One page of the known-users directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserList {
  /// the users on this page
  pub users: Vec<UserSummary>,
  /// how many users match, on every page together
  pub total: u64,
}

/// The query of `GET /users/{username}/profile`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ProfileQuery {
  /// fetch the status, statistics and info from the network first, waiting 5 s at most
  #[serde(default)]
  pub refresh: bool,
}

/// Our private notes about a user, the body of `PUT /users/{username}/notes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserNotes {
  /// the notes, at most 64 KiB
  pub notes: String,
}

/// Our labels for a user, the body of `PUT /users/{username}/tags`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserTags {
  /// at most 32 tags, each 1 to 32 of `a-z`, `0-9`, `_` and `-` after lower-casing
  #[schema(example = json!(["friend", "jazz"]))]
  pub tags: Vec<String>,
}

/// Assigns a user to a user-defined group, the body of `PUT /users/{username}/group`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupAssignment {
  /// a user-defined group from the configuration, null to remove the assignment
  pub group: Option<String>,
}

fn default_user_transfer_limit() -> u32 {
  100
}

/// The query of `GET /users/{username}/transfers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct UserTransfersQuery {
  /// only transfers in this direction, both when left out
  pub direction: Option<TransferDirection>,
  /// only transfers in this category, all when left out
  pub state: Option<TransferStateCategory>,
  /// how many transfers to skip
  #[serde(default)]
  pub offset: u32,
  /// how many transfers to return
  #[serde(default = "default_user_transfer_limit")]
  #[param(default = 100)]
  pub limit: u32,
}

/// One page of a user's transfers, newest first, removed ones included.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserTransfers {
  /// the transfers on this page
  pub transfers: Vec<Transfer>,
  /// how many transfers match, on every page together
  pub total: u64,
}

/// The query of `DELETE /users/{username}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DeleteUserQuery {
  /// also remove the user's transfers, close the conversation and delete its messages
  #[serde(default)]
  pub purge: bool,
}

/// A user was deleted from the directory, the data of a `users.removed` frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserRemoved {
  /// the user
  pub username: String,
}
