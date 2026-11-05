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
  /// who other users must be to see the share; a relay agent leaves it to the server, which
  /// fills it in from its own options
  #[serde(default)]
  #[schema(required = true)]
  pub visibility: ShareVisibility,
}

/// Who sees a shared folder in our browse listing, folder contents and search answers, and may
/// download from it. Ordered from the widest audience to the narrowest, so a user who sees
/// shares of one visibility also sees those of every wider one.
#[derive(
  Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ShareVisibility {
  /// every user
  #[default]
  Everyone,
  /// our buddies, trusted or not
  Buddies,
  /// our trusted buddies
  Trusted,
}

/// The shares of every host by host name: `local` for this instance, always present, and each
/// relay agent's instance name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SharesByHost(pub BTreeMap<String, Vec<Share>>);

/// One folder of this machine, a choice in the folder picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareFolder {
  /// the folder's own name, or a drive's letter on Windows
  pub name: String,
  /// the absolute path, what a share entry is written with
  pub path: String,
}

/// The folders directly in one folder, or the drives of a Windows machine. Files never appear.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareFolders {
  /// the folder listed; null for the drive list
  pub path: Option<String>,
  /// the folders in it, by name
  pub folders: Vec<ShareFolder>,
  /// more folders are there than were listed
  pub truncated: bool,
}

/// Which folder to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ShareFoldersQuery {
  /// the absolute path of the folder; without it the top of the machine, `/` or the drives
  #[serde(default)]
  pub path: Option<String>,
}

/// How a share scan is started.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ShareScanQuery {
  /// read every file again, even ones whose size and modification time are unchanged
  #[serde(default)]
  pub force: bool,
}
