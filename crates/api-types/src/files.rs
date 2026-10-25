//! Local files under the downloads, incomplete and log directories: listings, serving them to
//! the browser and the short-lived tokens that let a link load one.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::flags::flag_names;

/// A directory the files endpoints can reach, named in their path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileRoot {
  /// where finished downloads are moved to
  Downloads,
  /// where downloads are written while they run
  Incomplete,
}

bitflags::bitflags! {
  /// What kind of entry a file or directory is, written as the names of the set flags joined
  /// by `", "` such as `"Directory"` or `"Hidden, Directory"`. A plain file is `"Normal"`.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct FilesystemAttributes: u32 {
    /// the owner may not write it
    const ReadOnly = 1;
    /// its name starts with a dot, or the system marks it hidden
    const Hidden = 2;
    /// the system marks it as its own
    const System = 4;
    /// a directory
    const Directory = 16;
    /// the system marks it for archiving
    const Archive = 32;
    /// a file with no other flag
    const Normal = 128;
    /// a symbolic link, listed and never followed
    const ReparsePoint = 1024;
  }
}

flag_names!(
  FilesystemAttributes,
  "the names of the set attribute flags joined by \", \"",
  "Hidden, Directory"
);

/// A local file in a listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemFile {
  /// the file's name
  #[schema(example = "01 - Track.flac")]
  pub name: String,
  /// the path relative to the listed directory, with the system's separators
  #[schema(example = "Album/01 - Track.flac")]
  pub full_name: String,
  /// the size in bytes
  pub length: u64,
  /// what kind of entry it is
  pub attributes: FilesystemAttributes,
  /// when it was created, or when its metadata last changed where the system keeps no
  /// creation time
  pub created_at: DateTime<Utc>,
  /// when its contents last changed
  pub modified_at: DateTime<Utc>,
}

/// A local directory in a listing. The listed directory carries its entries, sorted by name,
/// directories first; a recursive listing holds every entry under it in the two flat lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemDirectory {
  /// the directory's name
  pub name: String,
  /// the path relative to the listed directory, empty for the listed directory itself
  pub full_name: String,
  /// what kind of entry it is
  pub attributes: FilesystemAttributes,
  /// when it was created, or when its metadata last changed where the system keeps no
  /// creation time
  pub created_at: DateTime<Utc>,
  /// when its entries last changed
  pub modified_at: DateTime<Utc>,
  /// the files in the listed directory, null for the directories listed in it
  pub files: Option<Vec<FilesystemFile>>,
  /// the directories in the listed directory, null for the directories listed in it
  #[schema(no_recursion)]
  pub directories: Option<Vec<FilesystemDirectory>>,
}

/// How a directory is listed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DirectoryListingQuery {
  /// list every entry under the directory, not only its direct entries
  #[serde(default)]
  pub recursive: bool,
}

/// Whether the browser shows a served file or saves it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ContentDisposition {
  /// show it where the browser can; HTML and SVG are always saved
  #[default]
  Inline,
  /// save it
  Attachment,
}

/// How a local file is served.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ContentQuery {
  /// whether the browser shows the file or saves it
  #[serde(default)]
  pub disposition: ContentDisposition,
  /// the name the browser saves the file under, null for the file's own name
  pub name: Option<String>,
  /// a file token for this file, for a link that carries no session
  #[serde(rename = "t")]
  pub token: Option<String>,
}

/// How a directory is served as a zip archive.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ArchiveQuery {
  /// the archive's name without `.zip`, null for the directory's name
  pub name: Option<String>,
}

/// The file a file token is asked for: a local file, or the file of a finished download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum FileTokenRequest {
  /// a local file
  Path {
    /// the root and the file's encoded path, as `<root>/<b64>` in the content URL
    #[schema(example = "downloads/QWxidW0vMDEgLSBUcmFjay5mbGFj")]
    path: String,
  },
  /// a download's file
  #[serde(rename_all = "camelCase")]
  Transfer {
    /// the download's id
    transfer_id: Uuid,
  },
}

/// A short-lived token that lets one link load one file without a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileToken {
  /// the token, given to the content URL as `t`
  pub token: String,
  /// when it stops being accepted
  pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn filesystem_attributes_are_written_by_name_in_bit_order() {
    let attributes = FilesystemAttributes::Directory | FilesystemAttributes::ReadOnly;
    assert_eq!(
      serde_json::to_string(&attributes).unwrap(),
      "\"ReadOnly, Directory\""
    );
    let read: FilesystemAttributes = serde_json::from_str("\"Directory, ReparsePoint\"").unwrap();
    assert_eq!(
      read,
      FilesystemAttributes::Directory | FilesystemAttributes::ReparsePoint
    );
  }

  #[test]
  fn file_token_requests_are_told_apart_by_their_field() {
    let path: FileTokenRequest = serde_json::from_str(r#"{"path":"downloads/YQ"}"#).unwrap();
    assert_eq!(
      path,
      FileTokenRequest::Path {
        path: "downloads/YQ".to_owned()
      }
    );
    let id = Uuid::from_u128(1);
    let transfer: FileTokenRequest =
      serde_json::from_str(&format!(r#"{{"transferId":"{id}"}}"#)).unwrap();
    assert_eq!(transfer, FileTokenRequest::Transfer { transfer_id: id });
    assert_eq!(
      serde_json::to_string(&transfer).unwrap(),
      format!(r#"{{"transferId":"{id}"}}"#)
    );
    assert!(serde_json::from_str::<FileTokenRequest>("{}").is_err());
  }
}
