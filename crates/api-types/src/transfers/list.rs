//! Listing transfers: grouped by user and remote directory, or flat and paged, and the queries
//! that choose them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Transfer, TransferStates};

/// One peer's transfers in one direction, grouped by remote directory, in the order the
/// transfers were first asked for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
  /// the peer
  pub username: String,
  /// the remote directories the peer's transfers are in, empty when there are none
  pub directories: Vec<DirectoryResponse>,
}

/// The transfers of one remote directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryResponse {
  /// the remote filename up to its last separator, `/` when the filename has one and `\`
  /// otherwise; empty for a filename without a separator
  #[schema(example = "@@abcde\\Music\\Artist\\Album")]
  pub directory: String,
  /// how many entries `files` has
  pub file_count: u32,
  /// the transfers, in the order they were asked for
  pub files: Vec<Transfer>,
}

/// Whether removed transfers are listed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct IncludeRemovedQuery {
  /// also list transfers that were removed
  #[serde(default)]
  pub include_removed: bool,
}

/// Which uploads the grouped upload listing holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct UploadsQuery {
  /// also list uploads that were removed
  #[serde(default)]
  pub include_removed: bool,
  /// only uploads requested at or after this time, null for no lower bound
  pub since: Option<DateTime<Utc>>,
  /// only uploads requested before this time, null for no upper bound
  pub until: Option<DateTime<Utc>>,
  /// at most this many uploads, null for every one
  pub limit: Option<u32>,
}

/// Whether one transfer is returned with its state history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct TransferQuery {
  /// fill `stateHistory`
  #[serde(default)]
  pub include_history: bool,
}

/// A named set of transfer states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TransferStateCategory {
  /// `Requested` and every `Queued` state
  Queued,
  /// `Initializing` and `InProgress`
  InProgress,
  /// every `Completed` state
  Completed,
  /// `Completed, Succeeded`
  Successful,
  /// every `Completed` state other than `Completed, Succeeded`
  Failed,
  /// `Completed` with `TimedOut`, `Errored`, `Rejected` or `Aborted`
  Errored,
  /// `Completed, Cancelled`
  Cancelled,
}

/// A transfer state filter: a category name such as `"Failed"`, or an exact state such as
/// `"Queued, Remotely"`. A name that is both, such as `"InProgress"`, is the category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum TransferStateFilter {
  /// every state in the category
  Category(TransferStateCategory),
  /// exactly this state
  State(TransferStates),
}

/// What the flat transfer listing is sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransferSort {
  /// when the transfer was asked for
  #[default]
  RequestedAt,
  /// when the transfer completed
  EndedAt,
  /// the peer's username
  Username,
  /// the remote filename
  Filename,
  /// the file's size
  Size,
}

/// Which way a listing is sorted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
  /// smallest or earliest first
  #[default]
  Asc,
  /// largest or latest first
  Desc,
}

/// Which transfers a page of the flat listing holds and in what order. Ties are broken by
/// `requestedAt`, then id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct TransferListQuery {
  /// only transfers in one of these states or categories, any state when empty; repeat the
  /// parameter for several
  #[serde(default)]
  pub state: Vec<TransferStateFilter>,
  /// only transfers with this peer
  pub username: Option<String>,
  /// also list transfers that were removed
  #[serde(default)]
  pub include_removed: bool,
  /// only transfers requested at or after this time
  pub since: Option<DateTime<Utc>>,
  /// what the listing is sorted by
  #[serde(default)]
  pub sort: TransferSort,
  /// which way it is sorted
  #[serde(default)]
  pub order: SortOrder,
  /// how many matching transfers to skip
  #[serde(default)]
  pub offset: u64,
  /// how many transfers the page holds at most, up to 1000
  #[serde(default = "default_list_limit")]
  #[param(maximum = 1000)]
  pub limit: u32,
}

fn default_list_limit() -> u32 {
  100
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn state_filters_read_categories_before_exact_states() {
    let read = |text: &str| serde_json::from_str::<TransferStateFilter>(text).unwrap();
    assert_eq!(
      read("\"Failed\""),
      TransferStateFilter::Category(TransferStateCategory::Failed)
    );
    assert_eq!(
      read("\"InProgress\""),
      TransferStateFilter::Category(TransferStateCategory::InProgress)
    );
    assert_eq!(
      read("\"Queued, Remotely\""),
      TransferStateFilter::State(TransferStates::QueuedRemotely)
    );
    assert_eq!(
      read("\"Requested\""),
      TransferStateFilter::State(TransferStates::Requested)
    );
    assert!(serde_json::from_str::<TransferStateFilter>("\"failed\"").is_err());
  }
}
