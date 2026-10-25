//! Shares: the shared directories of every host and how a scan of them is started. A share's
//! contents are listed as `users::Directory`, the shape a browse of another user has.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// A shared directory, or a directory excluded from the shares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Share {
  /// the upper-case hex SHA-1 of `remotePath`
  #[schema(example = "6B86B273FF34FCE19D6B804EFF5A3F5747ADA4EA")]
  pub id: String,
  /// the name the directory is shared under
  #[schema(example = "Music")]
  pub alias: String,
  /// the entry excludes its directory from the shares
  pub is_excluded: bool,
  /// the absolute local path, without trailing separators
  #[schema(example = "/srv/music")]
  pub local_path: String,
  /// the entry as configured, without trailing separators, with its exclusion prefix; the
  /// downloads path for a share made from an option
  #[schema(example = "[Music]/srv/music")]
  pub raw: String,
  /// the path other users see, the same as `alias`
  pub remote_path: String,
  /// directories in the share, its root included; null until the share was counted
  pub directories: Option<u32>,
  /// files in the share; null until the share was counted
  pub files: Option<u32>,
  /// the option that made this share when it is not an entry of the configured directories,
  /// such as `shares.share_downloads`; null for a configured share
  pub derived_from: Option<String>,
}

/// The shares of every host by host name: `local` for this instance, always present, and each
/// relay agent's instance name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SharesByHost(pub BTreeMap<String, Vec<Share>>);

/// How a share scan is started.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ShareScanQuery {
  /// read every file again, even ones whose size and modification time are unchanged
  #[serde(default)]
  pub force: bool,
}
