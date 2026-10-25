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
