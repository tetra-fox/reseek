//! The activity log: what other users on the network did with us, such as browsing our shares,
//! getting our search results or queueing uploads, newest last.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::transfers::TransferStates;

/// One line of the activity log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
  /// grows in the order entries are made; an entry that grows, such as more files queued by the
  /// same user, keeps its id and is sent again
  pub id: u64,
  /// when it happened, or when it last grew
  pub at: DateTime<Utc>,
  /// the other user, null for a change to our own membership of a private room
  #[schema(example = "alice")]
  pub username: Option<String>,
  /// what happened
  pub kind: ActivityKind,
}

/// What another user did with us.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ActivityKind {
  /// they asked for our whole share listing
  Browse,
  /// they asked for our profile: our description, picture and upload slots
  Profile,
  /// we sent our results for their search
  Search {
    /// what they searched for
    query: String,
    /// how many files we answered with
    results: u32,
  },
  /// they queued uploads from us; more requests from them close together add to it
  Queued {
    /// how many files
    files: u32,
    /// the last file they queued, as they named it
    filename: String,
  },
  /// an upload to them started sending
  UploadStarted {
    /// the file, as they named it
    filename: String,
    /// its size in bytes
    size: u64,
  },
  /// an upload to them ended, its state saying how
  UploadEnded {
    /// the file, as they named it
    filename: String,
    /// the final state, such as `Completed, Succeeded`
    state: TransferStates,
    /// how many bytes were sent
    bytes: u64,
  },
  /// our membership of a private room changed
  PrivateRoom {
    /// the room
    room: String,
    /// what changed
    change: PrivateRoomChange,
  },
}

/// How our membership of a private room changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PrivateRoomChange {
  /// we were made a member
  Added,
  /// we stopped being a member
  Removed,
  /// we were made an operator
  Operator,
  /// we stopped being an operator
  NoLongerOperator,
}
