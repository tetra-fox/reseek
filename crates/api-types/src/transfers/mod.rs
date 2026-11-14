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

/// Where a transfer is, written as one of `"Requested"`, `"Queued, Locally"`,
/// `"Queued, Remotely"`, `"Initializing"`, `"InProgress"`, or `"Completed, <outcome>"` with one
/// of the outcomes `Succeeded`, `Cancelled`, `TimedOut`, `Errored`, `Rejected` or `Aborted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferStates {
  /// our request was written to the peer and is not acknowledged yet
  Requested,
  /// waiting in this process for a slot, a retry or the enqueue throttle
  QueuedLocally,
  /// the peer acknowledged and queued us
  QueuedRemotely,
  /// accepted, the transfer connection is being made
  Initializing,
  /// bytes are flowing
  InProgress,
  /// finished, with how it ended
  Completed(TransferOutcome),
}

/// How a finished transfer ended.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
pub enum TransferOutcome {
  /// the whole file arrived
  Succeeded,
  /// canceled here
  Cancelled,
  /// a connect or inactivity timeout
  TimedOut,
  /// any other failure
  Errored,
  /// the peer refused the transfer
  Rejected,
  /// our size and the peer's differ
  Aborted,
}

impl TransferStates {
  /// Every state, in the order a transfer goes through them.
  pub const ALL: [Self; 11] = [
    Self::Requested,
    Self::QueuedLocally,
    Self::QueuedRemotely,
    Self::Initializing,
    Self::InProgress,
    Self::Completed(TransferOutcome::Succeeded),
    Self::Completed(TransferOutcome::Cancelled),
    Self::Completed(TransferOutcome::TimedOut),
    Self::Completed(TransferOutcome::Errored),
    Self::Completed(TransferOutcome::Rejected),
    Self::Completed(TransferOutcome::Aborted),
  ];

  /// The state as it is written.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Requested => "Requested",
      Self::QueuedLocally => "Queued, Locally",
      Self::QueuedRemotely => "Queued, Remotely",
      Self::Initializing => "Initializing",
      Self::InProgress => "InProgress",
      Self::Completed(TransferOutcome::Succeeded) => "Completed, Succeeded",
      Self::Completed(TransferOutcome::Cancelled) => "Completed, Cancelled",
      Self::Completed(TransferOutcome::TimedOut) => "Completed, TimedOut",
      Self::Completed(TransferOutcome::Errored) => "Completed, Errored",
      Self::Completed(TransferOutcome::Rejected) => "Completed, Rejected",
      Self::Completed(TransferOutcome::Aborted) => "Completed, Aborted",
    }
  }

  /// Whether the transfer has finished, whatever the outcome.
  pub fn is_completed(self) -> bool {
    matches!(self, Self::Completed(_))
  }
}

impl std::fmt::Display for TransferStates {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.as_str())
  }
}

impl std::str::FromStr for TransferStates {
  type Err = String;

  fn from_str(text: &str) -> Result<Self, Self::Err> {
    Self::ALL
      .into_iter()
      .find(|state| state.as_str() == text)
      .ok_or_else(|| unknown("transfer state", text, Self::ALL.map(Self::as_str)))
  }
}

/// The refusal of `text`, which is no `what`, naming every one of `valid`.
fn unknown<'a>(what: &str, text: &str, valid: impl IntoIterator<Item = &'a str>) -> String {
  let valid: Vec<String> = valid.into_iter().map(|name| format!("`{name}`")).collect();
  format!(
    "unknown {what} `{text}`, expected one of {}",
    valid.join(", ")
  )
}

impl Serialize for TransferStates {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for TransferStates {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
    text.parse().map_err(serde::de::Error::custom)
  }
}

impl utoipa::PartialSchema for TransferStates {
  fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::openapi::ObjectBuilder::new()
      .schema_type(utoipa::openapi::schema::Type::String)
      .description(Some("where a transfer is"))
      .enum_values(Some(Self::ALL.map(Self::as_str)))
      .into()
  }
}

impl utoipa::ToSchema for TransferStates {}

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
  /// or once the download left the peer's queue. Null for an upload, whose place in our queue
  /// moves whenever an upload starts or ends: `GET /api/v0/transfers/uploads/{username}/{id}/why`
  /// gives it
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
  /// a download the user paused: it waits here, keeping what arrived, until resumed
  // events stored by a build without pausing hold transfers with no such field
  #[serde(default)]
  #[schema(required = true)]
  pub paused: bool,
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

/// The downloads waiting in the local queue, in the order they start, as
/// `GET /transfers/downloads/queue` answers and a `transfers.queue` frame carries them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadQueue {
  /// the waiting downloads' ids, first to start first
  pub ids: Vec<Uuid>,
}

/// Moves waiting downloads in the local queue, as `POST /transfers/downloads/queue` takes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveQueuedDownloads {
  /// the downloads to move, in the order they are to start
  pub ids: Vec<Uuid>,
  /// the waiting download they go just before; null for the end of the queue
  #[serde(default)]
  pub before: Option<Uuid>,
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

/// Which speed limits are in force, the body of `PUT /transfers/speed-limits`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpeedLimitsSwitch {
  /// the alternative speed limits in place of the normal ones
  pub alternative: bool,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_transfer_stored_before_pausing_existed_reads_as_not_paused() {
    let stored = serde_json::json!({
      "id": "6f1c1bb4-3c56-4a49-9d3c-2b8e8a7f1d10",
      "jobId": null,
      "username": "alice",
      "direction": "Download",
      "filename": "@@a\\Album\\01.flac",
      "size": 1,
      "state": "Completed, Succeeded",
      "requestedAt": "2026-11-05T04:05:00Z",
      "enqueuedAt": null,
      "startedAt": null,
      "endedAt": "2026-11-05T04:06:00Z",
      "bytesTransferred": 1,
      "averageSpeed": 0.0,
      "placeInQueue": null,
      "exception": null,
      "attempts": 1,
      "nextAttemptAt": null,
      "removed": false,
      "bytesRemaining": 0,
      "elapsedTime": null,
      "percentComplete": 100.0,
      "remainingTime": null,
      "localPath": null,
      "stateHistory": null
    });
    let read: Transfer = serde_json::from_value(stored).unwrap();
    assert!(!read.paused);
  }

  #[test]
  fn transfer_states_are_written_exactly_and_read_back() {
    let names = [
      "Requested",
      "Queued, Locally",
      "Queued, Remotely",
      "Initializing",
      "InProgress",
      "Completed, Succeeded",
      "Completed, Cancelled",
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted",
    ];
    for (state, name) in TransferStates::ALL.into_iter().zip(names) {
      let json = format!("\"{name}\"");
      assert_eq!(serde_json::to_string(&state).unwrap(), json);
      assert_eq!(
        serde_json::from_str::<TransferStates>(&json).unwrap(),
        state
      );
    }
    for text in [
      "None",
      "Queued",
      "Completed",
      "Completed,Succeeded",
      "completed, succeeded",
      "",
    ] {
      assert!(
        serde_json::from_str::<TransferStates>(&format!("\"{text}\"")).is_err(),
        "{text}"
      );
    }
  }
}
