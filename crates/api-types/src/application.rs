//! The bodies and queries of the calls that control the daemon itself and its connection to
//! the Soulseek server.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The query of `GET /application/version/latest`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct VersionLatestQuery {
  /// read the release feed now instead of returning what the last check found; needs
  /// ReadWrite or above
  #[serde(default)]
  pub force_check: bool,
}

/// Why the client disconnects from the Soulseek server, the body of `DELETE /server`, written
/// as a bare JSON string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(example = "client disconnected from web UI")]
pub struct DisconnectReason(pub String);

/// One of the daemon's database files and its size on disk, as the Data tab lists them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseFile {
  /// the database's name, its file name without `.db`; what the vacuum call takes
  #[schema(example = "transfers")]
  pub name: String,
  /// the absolute path of the file
  #[schema(example = "/var/lib/reseek/transfers.db")]
  pub path: String,
  /// the file's size in bytes
  pub size_bytes: u64,
  /// the size of its write-ahead log in bytes, 0 when there is none
  pub wal_size_bytes: u64,
}

/// The body of `PUT /application/status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusChange {
  /// what we are now
  pub status: crate::state::OwnStatus,
}
