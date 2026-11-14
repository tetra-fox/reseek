//! Local files under the downloads, incomplete, temporary and log directories: listings, serving
//! them to the browser and the short-lived tokens that let a link load one.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{KnownFormat, ObjectBuilder, Schema, SchemaFormat, Type};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// A directory the files endpoints can reach, named in their path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileRoot {
  /// where finished downloads are moved to
  Downloads,
  /// where downloads are written while they run
  Incomplete,
  /// files uploaded for one analysis, held in the analysis cache and never listed
  Temporary,
}

/// The root's name as a path writes it, such as `downloads`.
impl std::fmt::Display for FileRoot {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let name = serde_plain::to_string(self).expect("a root is written as a plain name");
    f.write_str(&name)
  }
}

impl std::str::FromStr for FileRoot {
  type Err = String;

  /// Reads a root by its name as a path writes it.
  fn from_str(text: &str) -> Result<Self, String> {
    serde_plain::from_str(text).map_err(|_| format!("No directory root named {text}"))
  }
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

/// The names of the set flags joined by `", "` in the order they are declared, which is
/// ascending bit order; `"None"` for the empty set.
impl std::fmt::Display for FilesystemAttributes {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if self.is_empty() {
      return f.write_str("None");
    }
    let names: Vec<&str> = self.iter_names().map(|(name, _)| name).collect();
    f.write_str(&names.join(", "))
  }
}

impl FilesystemAttributes {
  /// The attributes the written form `text` names; names are matched exactly.
  fn from_names(text: &str) -> Result<Self, String> {
    if text == "None" {
      return Ok(Self::empty());
    }
    text.split(',').try_fold(Self::empty(), |set, name| {
      let name = name.trim();
      Self::from_name(name)
        .map(|flag| set.union(flag))
        .ok_or_else(|| format!("`{name}` is not a flag name"))
    })
  }
}

impl Serialize for FilesystemAttributes {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for FilesystemAttributes {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
    Self::from_names(&text).map_err(serde::de::Error::custom)
  }
}

impl utoipa::PartialSchema for FilesystemAttributes {
  fn schema() -> RefOr<Schema> {
    ObjectBuilder::new()
      .schema_type(Type::String)
      .description(Some(
        "the names of the set attribute flags joined by \", \"",
      ))
      .examples(["Hidden, Directory"])
      .into()
  }
}

impl ToSchema for FilesystemAttributes {}

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

/// A body that is a file's bytes as they are, such as a downloaded file, an image or a zip
/// archive. It is never JSON.
pub struct FileContent;

impl utoipa::PartialSchema for FileContent {
  fn schema() -> RefOr<Schema> {
    ObjectBuilder::new()
      .schema_type(Type::String)
      .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
      .description(Some("the bytes of the file"))
      .into()
  }
}

impl ToSchema for FileContent {}

#[cfg(test)]
mod tests {
  use super::FilesystemAttributes as Fs;

  #[test]
  fn attributes_are_written_by_name_in_bit_order() {
    for (value, text) in [
      (Fs::Directory | Fs::ReadOnly, "ReadOnly, Directory"),
      (Fs::ReparsePoint | Fs::Directory, "Directory, ReparsePoint"),
    ] {
      let json = format!("\"{text}\"");
      assert_eq!(serde_json::to_string(&value).unwrap(), json);
      assert_eq!(serde_json::from_str::<Fs>(&json).unwrap(), value, "{text}");
    }
  }
}
