//! Transfers: downloads and uploads, their states, download jobs and the bodies of the
//! transfer calls.

mod actions;
mod downloads;
mod list;
mod uploads;

pub use actions::*;
pub use downloads::*;
pub use list::*;
pub use uploads::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::flags::flag_names;

bitflags::bitflags! {
  /// The state of a transfer, written as the names of the set flags in ascending bit order
  /// joined by `", "`: `"Requested"`, `"Queued, Locally"`, `"Queued, Remotely"`,
  /// `"Initializing"`, `"InProgress"`, and `"Completed, <outcome>"` with one of `Succeeded`,
  /// `Cancelled`, `TimedOut`, `Errored`, `Rejected` or `Aborted`.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct TransferStates: u32 {
    /// our request was written to the peer and is not acknowledged yet
    const Requested = 1;
    /// waiting, with `Locally` or `Remotely` saying where
    const Queued = 2;
    /// accepted, the transfer connection is being made
    const Initializing = 4;
    /// bytes are flowing
    const InProgress = 8;
    /// finished, with one outcome flag
    const Completed = 16;
    /// outcome: the whole file arrived
    const Succeeded = 32;
    /// outcome: canceled here
    const Cancelled = 64;
    /// outcome: a connect or inactivity timeout
    const TimedOut = 128;
    /// outcome: any other failure
    const Errored = 256;
    /// outcome: the peer refused the transfer
    const Rejected = 512;
    /// outcome: our size and the peer's differ
    const Aborted = 1024;
    /// with `Queued`: waiting in this process for a slot, a retry or the enqueue throttle
    const Locally = 2048;
    /// with `Queued`: the peer acknowledged and queued us
    const Remotely = 4096;
  }
}

flag_names!(
  TransferStates,
  "the names of the set state flags joined by \", \"",
  "Completed, Succeeded"
);

/// Which way a transfer moves its file, seen from this daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TransferDirection {
  /// a peer sends the file to us
  Download,
  /// we send the file to a peer
  Upload,
}

impl From<soulseek_proto::TransferDirection> for TransferDirection {
  fn from(direction: soulseek_proto::TransferDirection) -> Self {
    match direction {
      soulseek_proto::TransferDirection::Download => Self::Download,
      soulseek_proto::TransferDirection::Upload => Self::Upload,
    }
  }
}

/// One file moving in one direction with one peer. The record exists from the moment the
/// transfer is asked for until it is removed, and is updated in place as it runs; a retry by
/// enqueueing again makes a new record with a new id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
  /// the transfer's id, never reused
  pub id: Uuid,
  /// the download job the download was queued in: everything queued with it in one go; null
  /// for uploads
  pub job_id: Option<Uuid>,
  /// the peer
  #[schema(example = "alice")]
  pub username: String,
  /// which way the file moves
  pub direction: TransferDirection,
  /// the remote filename exactly as the peer shares it, usually with `\` separators
  #[schema(example = "@@abcde\\Music\\Artist\\Album\\01 - Track.flac")]
  pub filename: String,
  /// the file's size in bytes, 0 when the caller did not know it
  pub size: u64,
  /// where the transfer is
  pub state: TransferStates,
  /// when the transfer was asked for: by us for a download, by the peer for an upload
  pub requested_at: DateTime<Utc>,
  /// when the peer queued our download or the upload entered our queue, null until then
  pub enqueued_at: Option<DateTime<Utc>>,
  /// when the current attempt started moving bytes, null until it does
  pub started_at: Option<DateTime<Utc>>,
  /// when the transfer completed, null until it does
  pub ended_at: Option<DateTime<Utc>>,
  /// bytes moved so far, counting from offset 0 of a resumed file
  pub bytes_transferred: u64,
  /// the average speed in bytes per second, 0 until the first sample
  pub average_speed: f64,
  /// the last place in the peer's queue the peer reported for a download, null when not asked
  /// or once the download left the peer's queue
  pub place_in_queue: Option<u32>,
  /// why the transfer failed, null when it did not
  pub exception: Option<String>,
  /// attempts made so far for a download, 0 for an upload
  pub attempts: u32,
  /// when a download waiting to be retried starts its next attempt, null when no retry is
  /// waiting
  pub next_attempt_at: Option<DateTime<Utc>>,
  /// hidden from the listings
  pub removed: bool,
  /// bytes still to move
  pub bytes_remaining: u64,
  /// milliseconds from `startedAt` to `endedAt`, or to now while running; null before the
  /// transfer starts
  pub elapsed_time: Option<u64>,
  /// how much of the file has moved, from 0 to 100; 0 for an empty file
  pub percent_complete: f64,
  /// milliseconds still needed at the average speed, null while the speed is 0
  pub remaining_time: Option<u64>,
  /// the absolute local path of a download's file once it was moved into the downloads
  /// directory, null before that and for uploads
  pub local_path: Option<String>,
  /// the states the transfer went through, oldest first; null unless asked for
  pub state_history: Option<Vec<TransferStateChange>>,
}

/// One state a transfer entered, and when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferStateChange {
  /// the state entered
  pub state: TransferStates,
  /// when it was entered
  pub at: DateTime<Utc>,
}

/// A transfer that was removed from the listings, as a `transfers.removed` frame carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferRemoved {
  /// the transfer's id
  pub id: Uuid,
  /// the peer
  pub username: String,
  /// which way the file moved
  pub direction: TransferDirection,
}

/// The Soulseek client's own records of the transfers it runs, as `GET /debug/transfers`
/// shows them for diagnosis. Their shape is the client's and may change between versions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugTransfers {
  /// the client's records
  pub soulseek: ClientTransfers,
}

/// The transfers the Soulseek client knows of, each as the client keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientTransfers {
  /// the uploads
  pub uploads: Vec<serde_json::Value>,
  /// the downloads
  pub downloads: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn transfer_states_are_written_by_name_in_bit_order() {
    let cases = [
      (TransferStates::Requested, "Requested"),
      (
        TransferStates::Queued | TransferStates::Locally,
        "Queued, Locally",
      ),
      (
        TransferStates::Remotely | TransferStates::Queued,
        "Queued, Remotely",
      ),
      (TransferStates::InProgress, "InProgress"),
      (
        TransferStates::Succeeded | TransferStates::Completed,
        "Completed, Succeeded",
      ),
      (
        TransferStates::Completed | TransferStates::Aborted,
        "Completed, Aborted",
      ),
      (TransferStates::empty(), "None"),
    ];
    for (state, text) in cases {
      let json = format!("\"{text}\"");
      assert_eq!(serde_json::to_string(&state).unwrap(), json);
      assert_eq!(
        serde_json::from_str::<TransferStates>(&json).unwrap(),
        state
      );
    }
  }

  #[test]
  fn transfer_states_are_read_leniently_about_spacing_but_not_names() {
    let read: TransferStates = serde_json::from_str("\"Completed,Succeeded\"").unwrap();
    assert_eq!(read, TransferStates::Completed | TransferStates::Succeeded);
    assert!(serde_json::from_str::<TransferStates>("\"completed, succeeded\"").is_err());
    assert!(serde_json::from_str::<TransferStates>("\"Completed, \"").is_err());
    assert!(serde_json::from_str::<TransferStates>("\"\"").is_err());
  }
}
