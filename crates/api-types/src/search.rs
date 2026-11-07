//! Searches we start on the network: the search record, its state, the responses peers send,
//! the files in them, and the bodies and queries of the search calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{ObjectBuilder, OneOfBuilder, Schema, Type};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::flags::flag_names;
use crate::transfers::SortOrder;

bitflags::bitflags! {
  /// The state of a search, written as the names of the set flags in ascending bit order
  /// joined by `", "`: `"Requested"`, `"Queued"`, `"InProgress"`, and `"Completed, <reason>"`
  /// with one of `Cancelled`, `TimedOut`, `ResponseLimitReached`, `FileLimitReached` or
  /// `Errored`.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct SearchStates: u32 {
    /// the record exists and the request is not sent yet
    const Requested = 1;
    /// the request was written to the server and responses are accepted
    const InProgress = 2;
    /// finished, with one reason flag
    const Completed = 4;
    /// reason: canceled here
    const Cancelled = 8;
    /// reason: no response was accepted for the search's timeout
    const TimedOut = 16;
    /// reason: the response limit was reached
    const ResponseLimitReached = 32;
    /// reason: the file limit was reached
    const FileLimitReached = 64;
    /// reason: the request could not be sent, or the search failed
    const Errored = 128;
    /// waiting for one of the searches that may run at once to finish
    const Queued = 256;
  }
}

flag_names!(
  SearchStates,
  "the names of the set state flags joined by \", \"",
  "Completed, TimedOut"
);

/// Where a search is sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchScopeType {
  /// the whole network, through the server and the distributed tree
  #[default]
  Network,
  /// only the users named in `subjects`
  User,
  /// only the members of the room named in `subjects`
  Room,
  /// only the members of the rooms named in `subjects`, which are rooms we are in; every room
  /// we are in when `subjects` is empty, filled in when the search starts
  Rooms,
  /// only our buddies who are online when the search starts, whom `subjects` names once it
  /// started
  Buddies,
  /// the network, as a wishlist search
  Wishlist,
}

/// Where a search is sent and to whom.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchScope {
  /// the kind of scope
  #[serde(rename = "type")]
  pub scope_type: SearchScopeType,
  /// the usernames for `user` and `buddies`, the room names for `room` and `rooms`, empty
  /// otherwise
  #[serde(default)]
  pub subjects: Vec<String>,
}

/// One search we started, as `GET /searches` lists it and the `search.*` frames carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Search {
  /// the search's id, chosen by the client or generated
  pub id: Uuid,
  /// the text sent to the network: the terms joined by single spaces, then each exclusion
  /// with a leading `-`
  #[schema(example = "pink floyd animals -live")]
  pub search_text: String,
  /// the token that ties the peers' responses to this search
  pub token: u32,
  /// where the search was sent
  pub scope: SearchScope,
  /// the search's state flags
  pub state: SearchStates,
  /// `state` has `Completed`; the responses are stored by the time this is true
  pub is_complete: bool,
  /// when the record was created
  pub started_at: DateTime<Utc>,
  /// when the search finished, null while it runs
  pub ended_at: Option<DateTime<Utc>>,
  /// responses accepted so far
  pub response_count: u32,
  /// unlocked files over the accepted responses
  pub file_count: u32,
  /// locked files over the accepted responses
  pub locked_file_count: u32,
  /// how many bytes the stored responses take, 0 until the search finished
  pub response_bytes: u64,
  /// the accepted responses; null in lists and frames and unless asked for
  pub responses: Option<Vec<SearchResponse>>,
}

fn default_search_timeout() -> SearchTimeout {
  SearchTimeout::Milliseconds(15_000)
}

/// How long a search waits for another response: a number of milliseconds, or a duration such
/// as `"15s"` or `"1m 30s"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum SearchTimeout {
  /// milliseconds
  Milliseconds(u64),
  /// a duration in words
  Text(String),
}

fn default_response_limit() -> u32 {
  250
}

fn default_file_limit() -> u32 {
  25_000
}

fn default_true() -> bool {
  true
}

fn default_minimum_response_file_count() -> u32 {
  1
}

/// Starts a search, the body of `POST /searches`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
  /// the id to give the search, so the client can show it before the reply arrives; a new id
  /// when null
  #[serde(default)]
  pub id: Option<Uuid>,
  /// what to search for: whitespace-separated terms, a term starting with `-` excludes
  #[schema(example = "pink floyd animals -live")]
  pub search_text: String,
  /// the search completes after this long without an accepted response, at least a second;
  /// a number is milliseconds
  #[serde(default = "default_search_timeout")]
  #[schema(example = "15s")]
  pub search_timeout: SearchTimeout,
  /// the search completes once this many responses were accepted
  #[serde(default = "default_response_limit")]
  #[schema(default = 250)]
  pub response_limit: u32,
  /// the search completes once the accepted responses hold this many unlocked files
  #[serde(default = "default_file_limit")]
  #[schema(default = 25000)]
  pub file_limit: u32,
  /// whether the three response filters below apply; when false every response is accepted
  #[serde(default = "default_true")]
  #[schema(default = true)]
  pub filter_responses: bool,
  /// responses with fewer files, locked ones included, are dropped
  #[serde(default = "default_minimum_response_file_count")]
  #[schema(default = 1)]
  pub minimum_response_file_count: u32,
  /// responses from peers whose upload queue is at least this long are dropped; null for no
  /// limit
  #[serde(default)]
  pub maximum_peer_queue_length: Option<u32>,
  /// responses from peers reporting a lower upload speed, in bytes per second, are dropped
  #[serde(default)]
  pub minimum_peer_upload_speed: u32,
  /// where to send the search, the whole network when left out
  #[serde(default)]
  pub scope: SearchScope,
}

/// One peer's answer to a search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
  /// the peer that answered
  pub username: String,
  /// the peer's country as ISO 3166-1 alpha-2, from the users directory; null when the
  /// directory has none for them
  #[schema(example = "NL")]
  pub country: Option<String>,
  /// the search's token, as the peer echoed it
  pub token: u32,
  /// the peer has an upload slot free
  pub has_free_upload_slot: bool,
  /// the peer's average upload speed in bytes per second, as it reports it
  pub upload_speed: u32,
  /// how many uploads the peer has queued, as it reports it
  pub queue_length: u32,
  /// how many files are in `files`
  pub file_count: u32,
  /// the matching files we may download
  pub files: Vec<File>,
  /// how many files are in `lockedFiles`
  pub locked_file_count: u32,
  /// the matching files the peer shares only with some users
  pub locked_files: Vec<File>,
}

/// A file a peer shares, as a search response, a browse or a folder listing carries it. The
/// named attribute fields are taken from `attributes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct File {
  /// the code byte the peer sent, 1 in practice
  pub code: u8,
  /// the full remote path in a search response, the bare name in a browse or folder listing,
  /// with the peer's separators kept exactly
  #[schema(example = "@@abcd\\Music\\Pink Floyd\\Animals\\01 Dogs.flac")]
  pub filename: String,
  /// in bytes
  pub size: u64,
  /// the extension as the peer sent it, often empty
  pub extension: String,
  /// kbit/s, null when the peer did not say
  pub bit_rate: Option<u32>,
  /// the duration in whole seconds, null when the peer did not say
  pub length: Option<u32>,
  /// whether the bit rate varies, null when the peer did not say
  pub is_variable_bit_rate: Option<bool>,
  /// Hz, null when the peer did not say
  pub sample_rate: Option<u32>,
  /// bits per sample, null when the peer did not say
  pub bit_depth: Option<u32>,
  /// the file is in a locked list
  pub is_locked: bool,
  /// the size does not fit the duration and the declared format; null when it fits, when
  /// there is too little to tell, and outside search responses
  pub suspect: Option<SuspectHint>,
  /// how many attributes the peer sent, unknown types included
  pub attribute_count: u32,
  /// the attributes in the order the peer sent them
  pub attributes: Vec<FileAttribute>,
}

/// A file as a peer listed it, its named attributes read from the list, not locked and with no
/// hint.
impl From<soulseek_proto::peer::File> for File {
  fn from(file: soulseek_proto::peer::File) -> Self {
    let (bit_rate, length) = (file.bit_rate(), file.length());
    let (is_variable_bit_rate, sample_rate, bit_depth) = (
      file.is_variable_bit_rate(),
      file.sample_rate(),
      file.bit_depth(),
    );
    let attributes: Vec<FileAttribute> = file.attributes.into_iter().map(Into::into).collect();
    Self {
      code: file.code,
      bit_rate,
      length,
      is_variable_bit_rate,
      sample_rate,
      bit_depth,
      attribute_count: u32::try_from(attributes.len()).unwrap_or(u32::MAX),
      filename: file.filename.as_str().to_string(),
      size: file.size,
      extension: file.extension,
      is_locked: false,
      suspect: None,
      attributes,
    }
  }
}

/// A hint that a file's size is too small for its duration and declared format, such as a
/// "320 kbps" MP3 whose size works out to 128 kbps. It is a reason to look twice, not a
/// verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuspectHint {
  /// which rule flagged the file
  pub reason: SuspectReason,
  /// the bit rate the size and duration work out to, kbit/s rounded down
  pub implied_kbps: u32,
  /// the bit rate the format or the declared bit rate leads one to expect, kbit/s
  pub expected_kbps: u32,
}

/// Which rule flagged a file as suspect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SuspectReason {
  /// a lossy file is well below the bit rate it declares
  BelowDeclaredBitrate,
  /// a lossless file is no bigger than a lossy one would be
  LossySizedLossless,
}

/// One typed value of a shared file, such as its bit rate or duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileAttribute {
  /// what the value is
  #[serde(rename = "type")]
  pub attribute_type: FileAttributeType,
  /// the value, in the type's unit
  pub value: u32,
}

impl From<soulseek_proto::peer::FileAttribute> for FileAttribute {
  fn from(attribute: soulseek_proto::peer::FileAttribute) -> Self {
    Self {
      attribute_type: attribute.attribute_type.into(),
      value: attribute.value,
    }
  }
}

/// What a file attribute's value is, written as its name, or as the bare type number for a
/// type this client does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileAttributeType {
  /// kbit/s
  BitRate,
  /// the duration in whole seconds
  Length,
  /// 1 when the bit rate varies, else 0
  VariableBitRate,
  /// Hz
  SampleRate,
  /// bits per sample
  BitDepth,
  /// a type number this client does not know
  #[serde(untagged)]
  Unknown(u32),
}

impl From<soulseek_proto::FileAttributeType> for FileAttributeType {
  fn from(attribute_type: soulseek_proto::FileAttributeType) -> Self {
    use soulseek_proto::FileAttributeType as Proto;
    match attribute_type {
      Proto::BitRate => Self::BitRate,
      Proto::Duration => Self::Length,
      Proto::VariableBitRate => Self::VariableBitRate,
      Proto::SampleRate => Self::SampleRate,
      Proto::BitDepth => Self::BitDepth,
      Proto::Unknown(code) => Self::Unknown(code),
    }
  }
}

// utoipa does not read `#[serde(untagged)]` on a single variant, so the schema is written here
impl utoipa::PartialSchema for FileAttributeType {
  fn schema() -> RefOr<Schema> {
    OneOfBuilder::new()
      .description(Some(
        "the attribute type's name, or its number when the type is not known",
      ))
      .item(
        ObjectBuilder::new()
          .schema_type(Type::String)
          .enum_values(Some([
            "BitRate",
            "Length",
            "VariableBitRate",
            "SampleRate",
            "BitDepth",
          ])),
      )
      .item(
        ObjectBuilder::new()
          .schema_type(Type::Integer)
          .minimum(Some(0)),
      )
      .into()
  }
}

impl ToSchema for FileAttributeType {}

/// The query of `GET /searches`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SearchListQuery {
  /// how many searches to return at most, all when left out
  #[serde(default)]
  pub limit: Option<u32>,
  /// how many searches to skip
  #[serde(default)]
  pub offset: u32,
  /// the order by `startedAt`, oldest first when left out
  #[serde(default)]
  pub order: SortOrder,
}

/// The query of `GET /searches/{id}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SearchGetQuery {
  /// include the responses
  #[serde(default)]
  pub include_responses: bool,
}

/// One response accepted by a running search, the data of a `search.response` frame, sent
/// only to streams subscribed to that search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponseReceived {
  /// the search the response belongs to
  pub search_id: Uuid,
  /// the response
  pub response: SearchResponse,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn file_attribute_types_are_names_or_numbers() {
    let attributes = [
      FileAttribute {
        attribute_type: FileAttributeType::Length,
        value: 245,
      },
      FileAttribute {
        attribute_type: FileAttributeType::Unknown(3),
        value: 7,
      },
    ];
    let json = r#"[{"type":"Length","value":245},{"type":3,"value":7}]"#;
    assert_eq!(serde_json::to_string(&attributes).unwrap(), json);
    assert_eq!(
      serde_json::from_str::<[FileAttribute; 2]>(json).unwrap(),
      attributes
    );
    assert!(serde_json::from_str::<FileAttributeType>("\"Duration\"").is_err());
  }
}

/// A saved search the wishlist runs again on the server's wishlist interval, one item a turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WishlistItem {
  /// the item's id
  pub id: Uuid,
  /// what it searches for, as typed
  #[schema(example = "boards of canada tomorrow's harvest")]
  pub search_text: String,
  /// when it was added
  pub added_at: DateTime<Utc>,
  /// when it was last searched, null until it was
  pub last_searched_at: Option<DateTime<Utc>>,
  /// the search it last ran, null until it ran one
  pub last_search_id: Option<Uuid>,
  /// how many files its last search found that it had not found before
  pub last_new_files: u32,
}

/// The body of `POST /searches/wishlist`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddWishlistItem {
  /// what to search for
  #[schema(example = "boards of canada tomorrow's harvest")]
  pub search_text: String,
}

/// A wishlist search found files it had not found before, as a `search.wishlist_found` frame
/// carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WishlistFound {
  /// the wishlist item
  pub item_id: Uuid,
  /// what it searches for
  pub search_text: String,
  /// the search that found them
  pub search_id: Uuid,
  /// how many files are new
  pub new_files: u32,
  /// the users the new files are from, at most five
  pub usernames: Vec<String>,
}
