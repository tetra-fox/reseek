//! Blocking users: the block record, what a block applies to, and the bodies of the blocking
//! calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// What a block stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BlockScope {
  /// the user's download requests from us are refused and their queued uploads cancelled
  Uploads,
  /// our shares do not answer the user's searches
  Searches,
  /// the user's browse and folder requests get an empty listing
  Browse,
  /// the user's private messages are acknowledged and discarded, and we can not message them
  Messages,
  /// the user's room messages are stored hidden and not pushed
  Rooms,
  /// we can not download from the user, and our downloads from them are cancelled
  Downloads,
  /// the user can not be our child in the distributed network
  Distributed,
}

/// Where a block comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BlockSource {
  /// set from the API, and removable there
  Api,
  /// the configuration's blacklisted group or managed blocklist; changed only there
  Config,
}

/// A blocked user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Block {
  /// the user
  #[schema(example = "alice")]
  pub username: String,
  /// why, may be empty
  pub reason: String,
  /// what the block stops; every scope for a block from the configuration
  pub scopes: Vec<BlockScope>,
  /// when the block was made, null for a block from the configuration
  pub created_at: Option<DateTime<Utc>>,
  /// when the block ends on its own, null for never
  pub expires_at: Option<DateTime<Utc>>,
  /// the name of the principal that made the block, null for a block from the configuration
  pub created_by: Option<String>,
  /// where the block comes from
  pub source: BlockSource,
}

fn default_true() -> bool {
  true
}

/// Blocks a user or replaces their block, the body of `PUT /users/{username}/block`. At most
/// one of `expiresIn` and `expiresAt` is given; with neither the block is permanent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockRequest {
  /// why, at most 500 characters
  #[serde(default)]
  pub reason: String,
  /// what the block stops, the configured default scopes when null
  #[serde(default)]
  #[schema(example = json!(["uploads", "searches", "browse", "messages", "rooms"]))]
  pub scopes: Option<Vec<BlockScope>>,
  /// how long the block lasts, such as `"7d"`
  #[serde(default)]
  #[schema(example = "7d")]
  pub expires_in: Option<String>,
  /// when the block ends
  #[serde(default)]
  pub expires_at: Option<DateTime<Utc>>,
  /// cancel the user's queued uploads now, and with `downloads` our downloads from them
  #[serde(default = "default_true")]
  #[schema(default = true)]
  pub cancel_transfers: bool,
}

fn default_include_config() -> bool {
  true
}

/// The query of `GET /users/blocked`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct BlockedQuery {
  /// also list the users the configuration's blacklisted group names, and its rules
  #[serde(default = "default_include_config")]
  #[param(default = true)]
  pub include_config: bool,
}

/// Every blocked user, and the configuration's rules that block users without naming them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockedUsers {
  /// the blocked users
  pub users: Vec<Block>,
  /// the configuration's rules, empty when they were not asked for
  pub rules: BlacklistRules,
}

/// The configuration's rules that block users by pattern, address or file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlacklistRules {
  /// regular expressions matched against usernames
  pub patterns: Vec<String>,
  /// networks whose addresses are blocked
  #[schema(example = json!(["203.0.113.0/24"]))]
  pub cidrs: Vec<String>,
  /// the managed blocklist file, null when none is configured
  pub blocklist_file: Option<BlocklistFile>,
}

/// The managed blocklist file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlocklistFile {
  /// where it is
  pub path: String,
  /// how many entries it holds
  pub entries: u32,
}

/// What `POST /users/blocked/import` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockImportResult {
  /// blocks created
  pub added: u32,
  /// entries that made no block
  pub skipped: u32,
}
