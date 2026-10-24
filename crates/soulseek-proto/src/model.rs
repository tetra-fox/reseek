//! The data model and enumerations of spec 01 section 8.

use std::net::SocketAddr;

use bitflags::bitflags;

use crate::error::DecodeError;
use crate::wire::RemoteFilename;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum UserPresence {
  Offline = 0,
  Away = 1,
  Online = 2,
}

impl UserPresence {
  pub(crate) fn from_u32(value: u32) -> Result<Self, DecodeError> {
    match value {
      0 => Ok(Self::Offline),
      1 => Ok(Self::Away),
      2 => Ok(Self::Online),
      _ => Err(DecodeError::InvalidValue {
        what: "user presence",
        value,
      }),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum TransferDirection {
  /// the sender wants to receive the file
  Download = 0,
  /// the sender is about to send the file
  Upload = 1,
}

impl TransferDirection {
  pub(crate) fn from_u32(value: u32) -> Result<Self, DecodeError> {
    match value {
      0 => Ok(Self::Download),
      1 => Ok(Self::Upload),
      _ => Err(DecodeError::InvalidValue {
        what: "transfer direction",
        value,
      }),
    }
  }
}

/// The room type a `JoinRoom` request creates when the room does not exist yet
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum RoomType {
  Public = 0,
  Private = 1,
}

impl RoomType {
  pub(crate) fn from_u32(value: u32) -> Result<Self, DecodeError> {
    match value {
      0 => Ok(Self::Public),
      1 => Ok(Self::Private),
      _ => Err(DecodeError::InvalidValue {
        what: "room type",
        value,
      }),
    }
  }
}

/// The one-character connection type of `PeerInit` and `ConnectToPeer`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum PeerConnectionType {
  /// `"P"`, peer messages
  Peer,
  /// `"F"`, raw file bytes
  Transfer,
  /// `"D"`, the distributed search tree
  Distributed,
}

impl PeerConnectionType {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Peer => "P",
      Self::Transfer => "F",
      Self::Distributed => "D",
    }
  }

  pub(crate) fn parse(value: &str) -> Result<Self, DecodeError> {
    match value {
      "P" => Ok(Self::Peer),
      "F" => Ok(Self::Transfer),
      "D" => Ok(Self::Distributed),
      _ => Err(DecodeError::UnknownConnectionType),
    }
  }
}

/// `3` (encoder) is obsolete and decodes as `Unknown(3)`, as do values from third-party clients,
/// so an attribute list re-encodes unchanged
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileAttributeType {
  /// kbit/s
  BitRate,
  /// seconds, truncated
  Duration,
  /// 0 or 1
  VariableBitRate,
  /// Hz
  SampleRate,
  /// bits
  BitDepth,
  Unknown(u32),
}

impl FileAttributeType {
  pub fn from_u32(value: u32) -> Self {
    match value {
      0 => Self::BitRate,
      1 => Self::Duration,
      2 => Self::VariableBitRate,
      4 => Self::SampleRate,
      5 => Self::BitDepth,
      other => Self::Unknown(other),
    }
  }

  pub fn to_u32(self) -> u32 {
    match self {
      Self::BitRate => 0,
      Self::Duration => 1,
      Self::VariableBitRate => 2,
      Self::SampleRate => 4,
      Self::BitDepth => 5,
      Self::Unknown(other) => other,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct FileAttribute {
  pub attribute_type: FileAttributeType,
  pub value: u32,
}

/// A shared file. In search responses `filename` is the full remote path, in browse and folder
/// responses the bare name inside its directory.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct File {
  /// written as 1 by the daemon, kept as received otherwise
  pub code: u8,
  pub filename: RemoteFilename,
  #[cfg_attr(test, proptest(strategy = "crate::proptests::file_size()"))]
  pub size: u64,
  /// as the sharer computed it, often blank; derive the extension from the filename instead
  pub extension: String,
  /// in wire order; this list, not the typed getters, is what re-encodes
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<FileAttribute>()")
  )]
  pub attributes: Vec<FileAttribute>,
}

impl File {
  pub fn attribute_count(&self) -> usize {
    self.attributes.len()
  }

  /// the last attribute of this type wins when a list repeats one
  fn attribute(&self, attribute_type: FileAttributeType) -> Option<u32> {
    self
      .attributes
      .iter()
      .rev()
      .find(|a| a.attribute_type == attribute_type)
      .map(|a| a.value)
  }

  pub fn bit_rate(&self) -> Option<u32> {
    self.attribute(FileAttributeType::BitRate)
  }

  /// seconds
  pub fn length(&self) -> Option<u32> {
    self.attribute(FileAttributeType::Duration)
  }

  pub fn is_variable_bit_rate(&self) -> Option<bool> {
    self
      .attribute(FileAttributeType::VariableBitRate)
      .map(|v| v != 0)
  }

  pub fn sample_rate(&self) -> Option<u32> {
    self.attribute(FileAttributeType::SampleRate)
  }

  pub fn bit_depth(&self) -> Option<u32> {
    self.attribute(FileAttributeType::BitDepth)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Directory {
  /// the full remote path with the peer's separator
  pub name: RemoteFilename,
  #[cfg_attr(test, proptest(strategy = "crate::proptests::small_vec::<File>()"))]
  pub files: Vec<File>,
}

impl Directory {
  pub fn file_count(&self) -> usize {
    self.files.len()
  }
}

/// The body of `Peer.SearchResponse` (9)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct SearchResponse {
  /// the responder's username
  pub username: String,
  pub token: u32,
  #[cfg_attr(test, proptest(strategy = "crate::proptests::small_vec::<File>()"))]
  pub files: Vec<File>,
  pub has_free_upload_slot: bool,
  /// bytes/second
  pub upload_speed: u32,
  pub queue_length: u32,
  /// files shared only with selected users, not downloadable
  #[cfg_attr(test, proptest(strategy = "crate::proptests::small_vec::<File>()"))]
  pub locked_files: Vec<File>,
}

/// The body of `Peer.BrowseResponse` (5)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct BrowseResponse {
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<Directory>()")
  )]
  pub directories: Vec<Directory>,
  /// shown but not downloadable
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<Directory>()")
  )]
  pub locked_directories: Vec<Directory>,
}

/// `Peer.InfoResponse` (16)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserInfo {
  pub description: String,
  /// raw image file bytes
  pub picture: Option<Vec<u8>>,
  pub upload_slots: u32,
  pub queue_length: u32,
  pub has_free_upload_slot: bool,
  /// who may upload to this user: 0 no one, 1 everyone, 2 users in their list, 3 permitted
  /// users; absent from SoulseekQt
  pub upload_permitted: Option<u32>,
}

impl UserInfo {
  pub fn has_picture(&self) -> bool {
    self.picture.is_some()
  }
}

/// The statistics tuple shared by `WatchUser`, `GetUserStats` and the room messages. On the wire
/// `upload_count` is followed by a `u32` that is always 0; it is written as 0 and not kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserStats {
  /// bytes/second of recent uploads
  pub average_speed: u32,
  /// completed uploads as counted by the server, a rolling 32-bit counter
  pub upload_count: u32,
  pub file_count: u32,
  pub directory_count: u32,
}

/// A room member as `JoinRoom`, `UserJoinedRoom` and `GlobalUserList` describe one. Those
/// messages always carry `slots_full` and `country_code`, so they are not optional here.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserData {
  pub username: String,
  pub status: UserPresence,
  pub stats: UserStats,
  /// every upload slot of this user is busy
  pub slots_full: bool,
  /// ISO 3166-1 alpha-2, upper-case, may be blank
  pub country_code: String,
}

/// `Server.GetStatus` (7)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserStatus {
  pub username: String,
  pub presence: UserPresence,
  pub is_privileged: bool,
}

/// `Server.GetUserStats` (36)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserStatistics {
  pub username: String,
  pub stats: UserStats,
}

/// `Server.GetUserInterests` (57)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserInterests {
  pub username: String,
  pub likes: Vec<String>,
  pub hates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Recommendation {
  pub interest: String,
  /// signed score, negative for strongly disliked
  pub count: i32,
}

/// The two-block layout of `GetPersonalRecommendations`, `GetGlobalRecommendations` and
/// `GetInterestRecommendations`. The second block is optional on read and always written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Recommendations {
  pub recommendations: Vec<Recommendation>,
  pub unrecommendations: Vec<Recommendation>,
}

/// One entry of `RoomList`
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RoomInfo {
  pub name: String,
  pub user_count: u32,
}

/// `Server.RoomList` (64)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RoomList {
  pub public: Vec<RoomInfo>,
  /// private rooms we own
  pub owned: Vec<RoomInfo>,
  /// private rooms we are a member of but do not own
  pub private: Vec<RoomInfo>,
  /// private rooms we operate
  pub moderated_room_names: Vec<String>,
}

/// The `JoinRoom` (14) response
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RoomData {
  pub name: String,
  /// for a private room this includes the owner and the operators
  pub users: Vec<UserData>,
  /// present exactly when the room is private
  pub private: Option<PrivateRoomData>,
}

impl RoomData {
  pub fn user_count(&self) -> usize {
    self.users.len()
  }

  pub fn is_private(&self) -> bool {
    self.private.is_some()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct PrivateRoomData {
  pub owner: String,
  pub operators: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RoomTicker {
  pub username: String,
  pub message: String,
}

/// Server tuning values, all `None` until their message arrives after login
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerInfo {
  pub parent_min_speed: Option<u32>,
  pub parent_speed_ratio: Option<u32>,
  /// seconds
  pub wishlist_interval: Option<u32>,
  /// from the login response
  pub is_supporter: Option<bool>,
}

/// The distributed network state the API exposes; the distributed section produces it
#[derive(Clone, Debug, PartialEq)]
pub struct DistributedNetworkInfo {
  pub average_broadcast_latency: Option<f64>,
  pub branch_level: u32,
  pub branch_root: String,
  pub is_branch_root: bool,
  pub child_limit: u32,
  pub can_accept_children: bool,
  pub children: Vec<(String, SocketAddr)>,
  pub parent: Option<(String, SocketAddr)>,
}

impl DistributedNetworkInfo {
  pub fn has_parent(&self) -> bool {
    self.parent.is_some()
  }
}

/// A free-text search query split into terms and exclusions (spec 01 section 8.15)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
  /// in order, including empty terms from consecutive spaces and a lone `-`
  pub terms: Vec<String>,
  /// leading `-`s stripped, duplicates removed; matched case-insensitively
  pub exclusions: Vec<String>,
}

impl SearchQuery {
  /// splits on single spaces; a token longer than one character that starts with `-` is an
  /// exclusion, anything else a term
  pub fn parse(text: &str) -> Self {
    let mut query = Self::default();
    for token in text.split(' ') {
      if token.len() > 1 && token.starts_with('-') {
        let exclusion = token.trim_start_matches('-');
        if !query.exclusions.iter().any(|e| e == exclusion) {
          query.exclusions.push(exclusion.to_owned());
        }
      } else {
        query.terms.push(token.to_owned());
      }
    }
    query
  }

  pub fn query(&self) -> String {
    self.terms.join(" ")
  }

  pub fn search_text(&self) -> String {
    let mut text = self.query();
    for exclusion in &self.exclusions {
      text.push_str(" -");
      text.push_str(exclusion);
    }
    text
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionKind {
  Server,
  Peer,
  Distributed,
  Transfer,
}

bitflags! {
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct ConnectionTypes: u32 {
    const OUTBOUND = 1;
    const INBOUND = 2;
    const DIRECT = 4;
    const INDIRECT = 8;
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionState {
  Pending = 0,
  Connecting = 1,
  Connected = 2,
  Disconnecting = 3,
  Disconnected = 4,
}

bitflags! {
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct TransferStates: u32 {
    const REQUESTED = 1;
    const QUEUED = 2;
    const INITIALIZING = 4;
    const IN_PROGRESS = 8;
    const COMPLETED = 16;
    const SUCCEEDED = 32;
    const CANCELLED = 64;
    const TIMED_OUT = 128;
    const ERRORED = 256;
    const REJECTED = 512;
    const ABORTED = 1024;
    const LOCALLY = 2048;
    const REMOTELY = 4096;
  }
}

bitflags! {
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct SearchStates: u32 {
    const REQUESTED = 1;
    const IN_PROGRESS = 2;
    const COMPLETED = 4;
    const CANCELLED = 8;
    const TIMED_OUT = 16;
    const RESPONSE_LIMIT_REACHED = 32;
    const FILE_LIMIT_REACHED = 64;
    const ERRORED = 128;
    const QUEUED = 256;
  }
}

bitflags! {
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct SoulseekClientStates: u32 {
    const DISCONNECTED = 1;
    const CONNECTED = 2;
    const LOGGED_IN = 4;
    const CONNECTING = 8;
    const LOGGING_IN = 16;
    const DISCONNECTING = 32;
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchScopeType {
  Network = 0,
  User = 1,
  Room = 2,
  Wishlist = 3,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn last_attribute_of_a_type_wins() {
    let attribute = |attribute_type, value| FileAttribute {
      attribute_type,
      value,
    };
    let file = File {
      code: 1,
      filename: "a.flac".into(),
      size: 1,
      extension: "flac".into(),
      attributes: vec![
        attribute(FileAttributeType::Duration, 10),
        attribute(FileAttributeType::Unknown(3), 7),
        attribute(FileAttributeType::SampleRate, 44100),
        attribute(FileAttributeType::Duration, 11),
      ],
    };
    assert_eq!(file.length(), Some(11));
    assert_eq!(file.sample_rate(), Some(44100));
    assert_eq!(file.bit_rate(), None);
    assert_eq!(file.attribute_count(), 4);
  }

  #[test]
  fn attribute_types_round_trip_through_u32() {
    for value in 0..8 {
      assert_eq!(FileAttributeType::from_u32(value).to_u32(), value);
    }
    assert_eq!(
      FileAttributeType::from_u32(3),
      FileAttributeType::Unknown(3)
    );
  }

  #[test]
  fn search_query_terms_and_exclusions() {
    let query = SearchQuery::parse("a  b - -x --y -x");
    assert_eq!(query.terms, ["a", "", "b", "-"]);
    assert_eq!(query.exclusions, ["x", "y"]);
    assert_eq!(query.query(), "a  b -");
    assert_eq!(query.search_text(), "a  b - -x -y");
  }

  #[test]
  fn search_query_without_exclusions() {
    let query = SearchQuery::parse("one two");
    assert_eq!(query.search_text(), "one two");
    assert!(query.exclusions.is_empty());
    assert_eq!(SearchQuery::parse("").terms, [""]);
  }

  #[test]
  fn search_query_dashes_only_is_an_empty_exclusion() {
    assert_eq!(SearchQuery::parse("--").exclusions, [""]);
  }
}
