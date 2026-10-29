//! Reports: aggregations over the transfer history for the dashboard, with the query of each
//! report call.
//!
//! Every report counts transfers by when they ended. A transfer removed from the transfer list
//! still counts.

use std::borrow::Cow;
use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::transfers::{SortOrder, TransferDirection, TransferStates};

/// How a finished transfer ended, as the key of a report's per-outcome map: its state without
/// the `Completed` flag.
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
  /// completed without an outcome flag
  None,
}

/// Aggregate figures over a set of finished transfers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferSummary {
  /// the summed size of the files in bytes
  pub total_bytes: u64,
  /// how many transfers
  pub count: u64,
  /// how many distinct users they were with
  pub distinct_users: u64,
  /// in a histogram bucket, the bytes moved divided by the seconds spent moving them; in the
  /// other reports, the mean of each transfer's own average speed; in bytes per second
  pub average_speed: f64,
  /// the mean time from enqueued to started, in seconds
  pub average_wait: f64,
  /// the mean time from started to ended, in seconds
  pub average_duration: f64,
}

/// The per-outcome figures of both directions. Both keys are always present; an outcome
/// appears only when a transfer ended with it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DirectionSummaries {
  /// downloads by outcome
  #[serde(rename = "Download")]
  pub download: BTreeMap<TransferOutcome, TransferSummary>,
  /// uploads by outcome
  #[serde(rename = "Upload")]
  pub upload: BTreeMap<TransferOutcome, TransferSummary>,
}

/// The answer to `GET /telemetry/reports/transfers/summary`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryReport {
  /// the per-outcome figures of both directions
  #[serde(flatten)]
  pub directions: DirectionSummaries,
  /// how many distinct users we transferred with successfully, in either direction
  pub distinct_users: u64,
}

/// A user's figures, a row of `GET /telemetry/reports/transfers/leaderboard`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardEntry {
  /// the user
  pub username: String,
  /// their successful transfers; `distinctUsers` is always 1
  #[serde(flatten)]
  pub summary: TransferSummary,
}

/// A failed transfer, a row of `GET /telemetry/reports/transfers/exceptions`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferExceptionDetail {
  /// the transfer's id
  pub id: Uuid,
  /// the user it was with
  pub username: String,
  /// which way it went
  pub direction: TransferDirection,
  /// the remote filename
  pub filename: String,
  /// the file's size in bytes
  pub size: u64,
  /// the final state
  pub state: TransferStates,
  /// when it was requested
  pub requested_at: DateTime<Utc>,
  /// when it was queued, null when it never was
  pub enqueued_at: Option<DateTime<Utc>>,
  /// when bytes started to flow, null when they never did
  pub started_at: Option<DateTime<Utc>>,
  /// when it ended
  pub ended_at: DateTime<Utc>,
  /// how many bytes were moved
  pub bytes_transferred: u64,
  /// its average speed in bytes per second
  pub average_speed: f64,
  /// the error message without its error type, null when none was recorded
  #[schema(example = "File not shared.")]
  pub exception: Option<String>,
}

/// How often one error message occurred, a row of
/// `GET /telemetry/reports/transfers/exceptions/pareto` and of a user report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionCount {
  /// the error message without its error type, null for failures without one
  #[schema(example = "File not shared.")]
  pub exception: Option<String>,
  /// how many failed transfers had it
  pub count: u64,
  /// how many distinct users they were with
  pub distinct_users: u64,
}

/// How often files of one shared directory were uploaded, a row of
/// `GET /telemetry/reports/transfers/directories`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryCount {
  /// the remote directory, the filename without its last segment; empty for a file without
  /// a directory
  #[schema(example = "@@share\\Artist\\Album")]
  pub directory: String,
  /// how many successful uploads came from it
  pub count: u64,
  /// how many distinct users they went to
  pub distinct_users: u64,
}

/// The answer to `GET /telemetry/reports/transfers/users/{username}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UserReport {
  /// downloads from the user
  #[serde(rename = "Download")]
  pub download: UserDirectionReport,
  /// uploads to the user
  #[serde(rename = "Upload")]
  pub upload: UserDirectionReport,
}

/// One direction of a user report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserDirectionReport {
  /// the figures by outcome
  pub summary: BTreeMap<TransferOutcome, TransferSummary>,
  /// the outcome counts
  pub statistics: UserTransferStatistics,
  /// the most frequent error messages, at most 25
  pub exceptions: Vec<ExceptionCount>,
}

/// Outcome counts of one direction of a user report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserTransferStatistics {
  /// every finished transfer
  pub total: u64,
  /// the ones that succeeded
  pub successful: u64,
  /// the ones that errored, timed out, were rejected or aborted
  pub errored: u64,
  /// the ones canceled
  pub cancelled: u64,
}

/// The column a leaderboard is ordered by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, ToSchema)]
pub enum LeaderboardSort {
  /// the number of transfers
  #[default]
  Count,
  /// the bytes moved
  TotalBytes,
  /// the mean speed
  AverageSpeed,
}

impl<'de> Deserialize<'de> for LeaderboardSort {
  /// Reads the name ignoring case.
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let text = <Cow<'de, str>>::deserialize(deserializer)?;
    if text.eq_ignore_ascii_case("Count") {
      Ok(Self::Count)
    } else if text.eq_ignore_ascii_case("TotalBytes") {
      Ok(Self::TotalBytes)
    } else if text.eq_ignore_ascii_case("AverageSpeed") {
      Ok(Self::AverageSpeed)
    } else {
      Err(serde::de::Error::custom(
        "invalid sortBy; expected one of: Count, TotalBytes, AverageSpeed",
      ))
    }
  }
}

/// Reads `Download` or `Upload` ignoring case.
fn direction<'de, D: Deserializer<'de>>(deserializer: D) -> Result<TransferDirection, D::Error> {
  let text = <Cow<'de, str>>::deserialize(deserializer)?;
  if text.eq_ignore_ascii_case("Download") {
    Ok(TransferDirection::Download)
  } else if text.eq_ignore_ascii_case("Upload") {
    Ok(TransferDirection::Upload)
  } else {
    Err(serde::de::Error::custom(
      "invalid direction; expected one of: Download, Upload",
    ))
  }
}

/// Reads an optional direction ignoring case.
fn optional_direction<'de, D: Deserializer<'de>>(
  deserializer: D,
) -> Result<Option<TransferDirection>, D::Error> {
  direction(deserializer).map(Some)
}

/// Reads `ASC` or `DESC` ignoring case.
fn sort_order<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SortOrder, D::Error> {
  let text = <Cow<'de, str>>::deserialize(deserializer)?;
  if text.eq_ignore_ascii_case("ASC") {
    Ok(SortOrder::Asc)
  } else if text.eq_ignore_ascii_case("DESC") {
    Ok(SortOrder::Desc)
  } else {
    Err(serde::de::Error::custom(
      "invalid sortOrder; expected one of: ASC, DESC",
    ))
  }
}

/// Reads an RFC 3339 time and converts it to UTC; a time written without an offset is taken
/// as UTC.
fn report_time<'de, D: Deserializer<'de>>(
  deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
  let text = <Cow<'de, str>>::deserialize(deserializer)?;
  if let Ok(time) = DateTime::parse_from_rfc3339(&text) {
    return Ok(Some(time.with_timezone(&Utc)));
  }
  NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S%.f")
    .map(|naive| Some(naive.and_utc()))
    .map_err(|e| serde::de::Error::custom(format!("not an RFC 3339 time: {e}")))
}

fn descending() -> SortOrder {
  SortOrder::Desc
}

fn default_limit() -> u32 {
  25
}

/// The query of `GET /telemetry/reports/transfers/summary`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SummaryQuery {
  /// the start of the window, seven days ago when left out
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, inclusive, now when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
  /// only this direction, `Download` or `Upload` in any case; both when left out
  #[serde(default, deserialize_with = "optional_direction")]
  pub direction: Option<TransferDirection>,
  /// only transfers with this user
  #[serde(default)]
  pub username: Option<String>,
}

/// The query of `GET /telemetry/reports/transfers/histogram`, which answers with the per-outcome
/// figures of both directions for each bucket, keyed by the bucket's start. Give `buckets` or
/// `interval`, not both.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct HistogramQuery {
  /// the start of the window, seven days ago when left out; a start before the first release
  /// means the oldest transfer
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, exclusive, now when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
  /// how many buckets to divide the window into, from 1 to 1000; 100 when neither this nor
  /// `interval` is given
  #[serde(default)]
  #[param(minimum = 1, maximum = 1000)]
  pub buckets: Option<u32>,
  /// the width of a bucket in minutes, at least 1
  #[serde(default)]
  #[param(minimum = 1)]
  pub interval: Option<u32>,
  /// only this direction, `Download` or `Upload` in any case; both when left out
  #[serde(default, deserialize_with = "optional_direction")]
  pub direction: Option<TransferDirection>,
  /// only transfers with this user
  #[serde(default)]
  pub username: Option<String>,
}

/// The query of `GET /telemetry/reports/transfers/leaderboard`, which ranks users by their
/// successful transfers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct LeaderboardQuery {
  /// the direction to rank, `Download` or `Upload` in any case
  #[serde(deserialize_with = "direction")]
  pub direction: TransferDirection,
  /// the start of the window, the first release when left out
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, inclusive, now when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
  /// the column to rank by, in any case; `Count` when left out
  #[serde(default)]
  pub sort_by: LeaderboardSort,
  /// `ASC` or `DESC` in any case, `DESC` when left out
  #[serde(default = "descending", deserialize_with = "sort_order")]
  pub sort_order: SortOrder,
  /// how many rows to return, at least 1; 25 when left out
  #[serde(default = "default_limit")]
  #[param(minimum = 1)]
  pub limit: u32,
  /// how many rows to skip, 0 when left out
  #[serde(default)]
  pub offset: u32,
}

/// The query of `GET /telemetry/reports/transfers/exceptions`, which lists failed transfers by
/// when they ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ExceptionsQuery {
  /// the direction to list, `Download` or `Upload` in any case
  #[serde(deserialize_with = "direction")]
  pub direction: TransferDirection,
  /// the start of the window, unbounded when left out
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, inclusive, unbounded when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
  /// only transfers with this user
  #[serde(default)]
  pub username: Option<String>,
  /// only transfers that failed with exactly this error message
  #[serde(default)]
  pub exception: Option<String>,
  /// `ASC` or `DESC` by end time, in any case; `DESC` when left out
  #[serde(default = "descending", deserialize_with = "sort_order")]
  pub sort_order: SortOrder,
  /// how many rows to return, at least 1; 25 when left out
  #[serde(default = "default_limit")]
  #[param(minimum = 1)]
  pub limit: u32,
  /// how many rows to skip, 0 when left out
  #[serde(default)]
  pub offset: u32,
}

/// The query of `GET /telemetry/reports/transfers/exceptions/pareto`, which counts failed
/// transfers by error message, most frequent first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ParetoQuery {
  /// the direction to count, `Download` or `Upload` in any case
  #[serde(deserialize_with = "direction")]
  pub direction: TransferDirection,
  /// the start of the window, unbounded when left out
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, inclusive, unbounded when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
  /// only transfers with this user
  #[serde(default)]
  pub username: Option<String>,
  /// how many rows to return, at least 1; 25 when left out
  #[serde(default = "default_limit")]
  #[param(minimum = 1)]
  pub limit: u32,
  /// how many rows to skip, 0 when left out
  #[serde(default)]
  pub offset: u32,
}

/// The query of `GET /telemetry/reports/transfers/directories`, which ranks our shared
/// directories by how many users downloaded from them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DirectoriesQuery {
  /// the start of the window, the first release when left out
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, inclusive, unbounded when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
  /// only uploads to this user
  #[serde(default)]
  pub username: Option<String>,
  /// how many rows to return, at least 1; 25 when left out
  #[serde(default = "default_limit")]
  #[param(minimum = 1)]
  pub limit: u32,
  /// how many rows to skip, 0 when left out
  #[serde(default)]
  pub offset: u32,
}

/// The query of `GET /telemetry/reports/transfers/users/{username}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct UserReportQuery {
  /// the start of the window, the first release when left out
  #[serde(default, deserialize_with = "report_time")]
  pub start: Option<DateTime<Utc>>,
  /// the end of the window, inclusive, now when left out
  #[serde(default, deserialize_with = "report_time")]
  pub end: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn leaderboard_query_reads_names_in_any_case() {
    let query: LeaderboardQuery =
      serde_json::from_str(r#"{"direction":"upload","sortBy":"totalbytes","sortOrder":"asc"}"#)
        .unwrap();
    assert_eq!(query.direction, TransferDirection::Upload);
    assert_eq!(query.sort_by, LeaderboardSort::TotalBytes);
    assert_eq!(query.sort_order, SortOrder::Asc);
    assert_eq!(query.limit, 25);
    let defaults: LeaderboardQuery = serde_json::from_str(r#"{"direction":"Download"}"#).unwrap();
    assert_eq!(defaults.sort_by, LeaderboardSort::Count);
    assert_eq!(defaults.sort_order, SortOrder::Desc);
    assert!(serde_json::from_str::<LeaderboardQuery>(r#"{"direction":"sideways"}"#).is_err());
    assert!(serde_json::from_str::<LeaderboardQuery>("{}").is_err());
  }

  #[test]
  fn report_times_take_an_offset_or_none_as_utc() {
    let read = |text: &str| -> Option<DateTime<Utc>> {
      serde_json::from_value::<SummaryQuery>(serde_json::json!({ "start": text }))
        .unwrap()
        .start
    };
    let utc: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
    assert_eq!(read("2026-01-01T00:00:00Z"), Some(utc));
    assert_eq!(read("2026-01-01T02:00:00+02:00"), Some(utc));
    assert_eq!(read("2026-01-01T00:00:00"), Some(utc), "naive is UTC");
    assert_eq!(
      read("2026-01-01T00:00:00.5").map(|t| t.timestamp_subsec_millis()),
      Some(500)
    );
    assert!(serde_json::from_value::<SummaryQuery>(serde_json::json!({ "start": "abc" })).is_err());
  }

  #[test]
  fn summary_report_puts_directions_beside_distinct_users() {
    let mut directions = DirectionSummaries::default();
    directions.upload.insert(
      TransferOutcome::Succeeded,
      TransferSummary {
        total_bytes: 10,
        count: 1,
        distinct_users: 1,
        average_speed: 2.5,
        average_wait: 0.0,
        average_duration: 4.0,
      },
    );
    let report = SummaryReport {
      directions,
      distinct_users: 1,
    };
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(
      json,
      serde_json::json!({
        "Download": {},
        "Upload": { "Succeeded": {
          "totalBytes": 10, "count": 1, "distinctUsers": 1,
          "averageSpeed": 2.5, "averageWait": 0.0, "averageDuration": 4.0
        } },
        "distinctUsers": 1
      })
    );
    assert_eq!(
      serde_json::from_value::<SummaryReport>(json).unwrap(),
      report
    );
  }
}
