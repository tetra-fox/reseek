//! The server connection (spec 01 section 5, 01a section 2): [`ServerRequest`] is what the client
//! sends, [`ServerResponse`] what the server sends, pushed or in reply. A code whose layout
//! differs between the two directions has a `...Request` and a `...Response` struct; a code with
//! one layout both ways has one struct used by both enums.

use std::net::Ipv4Addr;

use md5::{Digest, Md5};

use crate::codec::{Wire, family, wire_struct};
use crate::codes::ServerCode;
use crate::distributed::Embedded;
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::{PeerConnectionType, RoomType, UserPresence};
use crate::wire::{Reader, RemoteFilename, Secret, Writer};

/// The major version sent in `Login`. 177 is the number the version registry keeps for
/// experimental clients; 170 belongs to Soulseek.NET and must not be reused.
pub const PROTOCOL_MAJOR_VERSION: u32 = 177;
/// The minor version sent in `Login`; the server only requires it to be non-zero
pub const PROTOCOL_MINOR_VERSION: u32 = 760;

/// Lowercase hex md5 of the utf-8 bytes of `username + password`, the `Login` hash field
pub fn login_hash(username: &str, password: &str) -> String {
  let mut md5 = Md5::new();
  md5.update(username.as_bytes());
  md5.update(password.as_bytes());
  format!("{:x}", md5.finalize())
}

family! {
  pub enum ServerRequest {
    code: ServerCode,
    frame: Server,
    label: "server request",
    quiet: [],
    messages: {
      Login(LoginRequest),
      SetListenPort(SetListenPort),
      GetPeerAddress(GetPeerAddressRequest),
      WatchUser(WatchUserRequest),
      UnwatchUser(UnwatchUser),
      GetStatus(GetStatusRequest),
      /// obsolete
      IgnoreUser(IgnoreUser),
      /// obsolete
      UnignoreUser(UnignoreUser),
      SayInChatRoom(SayInChatRoomRequest),
      JoinRoom(JoinRoomRequest),
      LeaveRoom(LeaveRoom),
      ConnectToPeer(ConnectToPeerRequest),
      PrivateMessage(PrivateMessageRequest),
      AcknowledgePrivateMessage(AcknowledgePrivateMessage),
      /// obsolete
      FileSearchRoom(FileSearchRoom),
      FileSearch(FileSearchRequest),
      SetOnlineStatus(SetOnlineStatus),
      /// a write-liveness probe; the server never answers
      Ping,
      /// obsolete
      SendConnectToken(SendConnectToken),
      /// obsolete `SendDownloadSpeed`
      SendSpeed(SendSpeed),
      SharedFoldersAndFiles(SharedFoldersAndFiles),
      GetUserStats(GetUserStatsRequest),
      /// obsolete `UploadSlotsFull`
      QueuedDownloads(QueuedDownloadsRequest),
      UserSearch(UserSearch),
      /// obsolete
      SimilarRecommendations(SimilarRecommendationsRequest),
      LikedInterestAdd(LikedInterestAdd),
      LikedInterestRemove(LikedInterestRemove),
      GetPersonalRecommendations,
      /// obsolete
      MyRecommendations,
      GetGlobalRecommendations,
      GetUserInterests(GetUserInterestsRequest),
      /// obsolete
      AdminCommand(AdminCommand),
      /// obsolete
      PlaceInLineRequest(PlaceInLineRequest),
      /// obsolete
      PlaceInLineResponse(PlaceInLineResponse),
      RoomList,
      /// obsolete
      ExactFileSearch(ExactFileSearchRequest),
      /// obsolete
      GlobalUserList,
      /// obsolete
      TunneledMessage(TunneledMessageRequest),
      HaveNoParents(HaveNoParents),
      /// deprecated, never sent
      ParentsIP(ParentsIP),
      CheckPrivileges,
      AcceptChildren(AcceptChildren),
      WishlistSearch(WishlistSearch),
      GetPersonalSimilarUsers,
      GetInterestRecommendations(GetInterestRecommendationsRequest),
      GetInterestSimilarUsers(GetInterestSimilarUsersRequest),
      SetRoomTicker(SetRoomTicker),
      HatedInterestAdd(HatedInterestAdd),
      HatedInterestRemove(HatedInterestRemove),
      RoomSearch(RoomSearch),
      SendUploadSpeed(SendUploadSpeed),
      /// deprecated
      UserPrivileges(UserPrivilegesRequest),
      GivePrivileges(GivePrivileges),
      /// deprecated
      AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges),
      BranchLevel(BranchLevel),
      BranchRoot(BranchRoot),
      /// obsolete, never sent
      ChildDepth(ChildDepth),
      PrivateRoomAddUser(PrivateRoomAddUser),
      PrivateRoomRemoveUser(PrivateRoomRemoveUser),
      PrivateRoomDropMembership(PrivateRoomDropMembership),
      PrivateRoomDropOwnership(PrivateRoomDropOwnership),
      /// obsolete
      PrivateRoomUnknown(PrivateRoomUnknown),
      PrivateRoomToggle(PrivateRoomToggle),
      NewPassword(NewPassword),
      PrivateRoomAddOperator(PrivateRoomAddOperator),
      PrivateRoomRemoveOperator(PrivateRoomRemoveOperator),
      /// codec only
      MessageUsers(MessageUsers),
      AskPublicChat,
      StopPublicChat,
      /// obsolete
      RelatedSearch(RelatedSearchRequest),
      CannotConnect(CannotConnectRequest),
    }
  }
}

family! {
  pub enum ServerResponse {
    code: ServerCode,
    frame: Server,
    label: "server response",
    // embedded messages arrive for every network search while we are a branch root
    quiet: [EmbeddedMessage],
    messages: {
      Login(LoginResponse),
      GetPeerAddress(GetPeerAddressResponse),
      WatchUser(WatchUserResponse),
      GetStatus(UserStatus),
      /// obsolete
      IgnoreUser(IgnoreUser),
      /// obsolete
      UnignoreUser(UnignoreUser),
      SayInChatRoom(SayInChatRoomResponse),
      JoinRoom(RoomData),
      LeaveRoom(LeaveRoom),
      UserJoinedRoom(UserJoinedRoom),
      UserLeftRoom(UserLeftRoom),
      ConnectToPeer(ConnectToPeerResponse),
      PrivateMessage(PrivateMessageResponse),
      /// a search for us to answer
      FileSearch(FileSearchResponse),
      /// the live server never sends this; an unsolicited one is dropped
      Ping,
      /// obsolete
      SendConnectToken(SendConnectToken),
      GetUserStats(UserStatistics),
      /// obsolete `UploadSlotsFull`
      QueuedDownloads(QueuedDownloadsResponse),
      /// another session logged in with our credentials
      KickedFromServer,
      /// obsolete
      SimilarRecommendations(SimilarRecommendationsResponse),
      GetPersonalRecommendations(Recommendations),
      /// obsolete
      MyRecommendations(MyRecommendationsResponse),
      GetGlobalRecommendations(Recommendations),
      GetUserInterests(UserInterests),
      /// obsolete
      PlaceInLineRequest(PlaceInLineRequest),
      /// obsolete
      PlaceInLineResponse(PlaceInLineResponse),
      /// obsolete
      RoomAdded(RoomAdded),
      /// obsolete
      RoomRemoved(RoomRemoved),
      RoomList(RoomList),
      /// obsolete
      ExactFileSearch(ExactFileSearchResponse),
      GlobalAdminMessage(GlobalAdminMessage),
      /// obsolete
      GlobalUserList(GlobalUserListResponse),
      /// obsolete
      TunneledMessage(TunneledMessageResponse),
      PrivilegedUsers(PrivilegedUsers),
      ParentMinSpeed(ParentMinSpeed),
      ParentSpeedRatio(ParentSpeedRatio),
      /// obsolete
      ParentInactivityTimeout(ParentInactivityTimeout),
      /// obsolete
      SearchInactivityTimeout(SearchInactivityTimeout),
      /// obsolete
      MinimumParentsInCache(MinimumParentsInCache),
      /// obsolete
      DistributedAliveInterval(DistributedAliveInterval),
      /// obsolete, the parser is kept
      AddPrivilegedUser(AddPrivilegedUser),
      CheckPrivileges(CheckPrivilegesResponse),
      /// a distributed message the server wraps for a branch root
      EmbeddedMessage(Embedded),
      NetInfo(NetInfo),
      WishlistInterval(WishlistInterval),
      GetPersonalSimilarUsers(GetPersonalSimilarUsersResponse),
      GetInterestRecommendations(GetInterestRecommendationsResponse),
      GetInterestSimilarUsers(GetInterestSimilarUsersResponse),
      RoomTickers(RoomTickers),
      RoomTickerAdd(RoomTickerAdd),
      RoomTickerRemove(RoomTickerRemove),
      /// deprecated
      UserPrivileges(UserPrivilegesResponse),
      /// deprecated, acknowledge with `AcknowledgeNotifyPrivileges`
      NotifyPrivileges(NotifyPrivileges),
      DistributedReset,
      PrivateRoomUsers(PrivateRoomUsers),
      PrivateRoomAddUser(PrivateRoomAddUser),
      PrivateRoomRemoveUser(PrivateRoomRemoveUser),
      /// obsolete
      PrivateRoomUnknown(PrivateRoomUnknown),
      PrivateRoomAdded(PrivateRoomAdded),
      PrivateRoomRemoved(PrivateRoomRemoved),
      PrivateRoomToggle(PrivateRoomToggle),
      NewPassword(NewPassword),
      PrivateRoomAddOperator(PrivateRoomAddOperator),
      PrivateRoomRemoveOperator(PrivateRoomRemoveOperator),
      PrivateRoomOperatorAdded(PrivateRoomOperatorAdded),
      PrivateRoomOperatorRemoved(PrivateRoomOperatorRemoved),
      PrivateRoomOwned(PrivateRoomOwned),
      PublicChat(PublicChat),
      /// obsolete
      RelatedSearch(RelatedSearchResponse),
      ExcludedSearchPhrases(ExcludedSearchPhrases),
      CannotConnect(CannotConnectResponse),
      CannotJoinRoom(CannotJoinRoom),
    }
  }
}

/// The statistics tuple of `WatchUser`, `GetUserStats` and the room messages. On the wire
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

impl Wire for UserStats {
  const MIN_BYTES: usize = 20;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let average_speed = r.u32()?;
    let upload_count = r.u32()?;
    // always 0 on the live server
    r.u32()?;
    Ok(Self {
      average_speed,
      upload_count,
      file_count: r.u32()?,
      directory_count: r.u32()?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.average_speed);
    w.u32(self.upload_count);
    w.u32(0);
    w.u32(self.file_count);
    w.u32(self.directory_count);
  }
}

wire_struct! {
  pub struct Recommendation {
    pub interest: String,
    /// signed score, negative for strongly disliked
    pub count: i32,
  }
}

/// The two-block layout of `GetPersonalRecommendations`, `GetGlobalRecommendations` and
/// `GetInterestRecommendations`. The second block is optional on read and always written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Recommendations {
  pub recommendations: Vec<Recommendation>,
  pub unrecommendations: Vec<Recommendation>,
}

impl Wire for Recommendations {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let recommendations = Wire::read_field(r, "recommendations")?;
    let unrecommendations = if r.has_more() {
      Wire::read_field(r, "unrecommendations")?
    } else {
      Vec::new()
    };
    Ok(Self {
      recommendations,
      unrecommendations,
    })
  }

  fn write(&self, w: &mut Writer) {
    self.recommendations.write(w);
    self.unrecommendations.write(w);
  }
}

/// A room member as `JoinRoom`, `UserJoinedRoom` and `GlobalUserList` describe one; those
/// messages always carry `slots_full` and `country_code`
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

fn read_parallel_count(
  r: &mut Reader,
  min_bytes: usize,
  expected: usize,
  what: &'static str,
) -> Result<(), DecodeError> {
  if r.count(min_bytes, what)? != expected {
    return Err(DecodeError::ListLengthMismatch { what });
  }
  Ok(())
}

/// The five parallel per-user lists of `JoinRoom` and `GlobalUserList`, which must agree in
/// length
fn read_user_lists(r: &mut Reader) -> Result<Vec<UserData>, DecodeError> {
  let usernames: Vec<String> = Wire::read_field(r, "room users")?;
  let n = usernames.len();
  read_parallel_count(r, UserPresence::MIN_BYTES, n, "user statuses")?;
  let statuses = (0..n)
    .map(|_| UserPresence::read(r))
    .collect::<Result<Vec<_>, _>>()?;
  read_parallel_count(r, UserStats::MIN_BYTES, n, "user stats")?;
  let stats = (0..n)
    .map(|_| UserStats::read(r))
    .collect::<Result<Vec<_>, _>>()?;
  read_parallel_count(r, 4, n, "user slots")?;
  let slots = (0..n)
    .map(|_| Ok(r.u32()? != 0))
    .collect::<Result<Vec<_>, DecodeError>>()?;
  read_parallel_count(r, String::MIN_BYTES, n, "user countries")?;
  let countries = (0..n).map(|_| r.string()).collect::<Result<Vec<_>, _>>()?;
  Ok(
    usernames
      .into_iter()
      .zip(statuses)
      .zip(stats)
      .zip(slots)
      .zip(countries)
      .map(
        |((((username, status), stats), slots_full), country_code)| UserData {
          username,
          status,
          stats,
          slots_full,
          country_code,
        },
      )
      .collect(),
  )
}

fn write_user_lists(w: &mut Writer, users: &[UserData]) {
  w.count(users.len());
  for user in users {
    w.string(&user.username);
  }
  w.count(users.len());
  for user in users {
    user.status.write(w);
  }
  w.count(users.len());
  for user in users {
    user.stats.write(w);
  }
  w.count(users.len());
  for user in users {
    w.u32(u32::from(user.slots_full));
  }
  w.count(users.len());
  for user in users {
    w.string(&user.country_code);
  }
}

/// One entry of `RoomList`
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RoomInfo {
  pub name: String,
  pub user_count: u32,
}

/// the names and counts of one `RoomList` block, which must agree in length
fn read_room_block(r: &mut Reader, what: &'static str) -> Result<Vec<RoomInfo>, DecodeError> {
  let names: Vec<String> = Wire::read_field(r, what)?;
  read_parallel_count(r, 4, names.len(), what)?;
  names
    .into_iter()
    .map(|name| {
      Ok(RoomInfo {
        name,
        user_count: r.u32()?,
      })
    })
    .collect()
}

fn write_room_block(w: &mut Writer, rooms: &[RoomInfo]) {
  w.count(rooms.len());
  for room in rooms {
    w.string(&room.name);
  }
  w.count(rooms.len());
  for room in rooms {
    w.u32(room.user_count);
  }
}

wire_struct! {
  pub struct LoginRequest {
    pub username: String,
    pub password: Secret,
    pub major_version: u32,
    /// [`login_hash`] of the username and password
    pub hash: Secret,
    pub minor_version: u32,
  }
}

impl LoginRequest {
  /// a `Login` with the hash computed from the credentials
  pub fn new(username: String, password: String, major_version: u32, minor_version: u32) -> Self {
    let hash = Secret::new(login_hash(&username, &password));
    Self {
      username,
      password: Secret::new(password),
      major_version,
      hash,
      minor_version,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum LoginResponse {
  Success {
    /// the server's multi-line greeting
    greeting: String,
    /// our address as the server sees it
    ip: Ipv4Addr,
    /// md5 of the password we sent, informational
    hash: Secret,
    /// has ever bought privileges
    is_supporter: bool,
  },
  Failure {
    /// `INVALIDUSERNAME`, `INVALIDPASS`, `SVRFULL`, ... shown verbatim
    reason: String,
    detail: Option<String>,
  },
}

impl Wire for LoginResponse {
  const MIN_BYTES: usize = 5;

  /// `success` and `is_supporter` are true only for exactly 1
  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(if r.bool_exact()? {
      Self::Success {
        greeting: r.description()?,
        ip: r.ip4()?,
        hash: Secret::read(r)?,
        is_supporter: r.bool_exact()?,
      }
    } else {
      Self::Failure {
        reason: r.string()?,
        detail: if r.has_more() {
          Some(r.string()?)
        } else {
          None
        },
      }
    })
  }

  fn write(&self, w: &mut Writer) {
    match self {
      Self::Success {
        greeting,
        ip,
        hash,
        is_supporter,
      } => {
        w.bool(true);
        w.string(greeting);
        w.ip4(*ip);
        hash.write(w);
        w.bool(*is_supporter);
      }
      Self::Failure { reason, detail } => {
        w.bool(false);
        w.string(reason);
        if let Some(detail) = detail {
          w.string(detail);
        }
      }
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct SetListenPort {
  pub port: u32,
  /// `(obfuscation_type, obfuscated_port)`; never sent by this client
  pub obfuscation: Option<(u32, u32)>,
}

impl Wire for SetListenPort {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      port: r.u32()?,
      obfuscation: if r.remaining() >= 8 {
        Some((r.u32()?, r.u32()?))
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.port);
    if let Some((obfuscation_type, obfuscated_port)) = self.obfuscation {
      w.u32(obfuscation_type);
      w.u32(obfuscated_port);
    }
  }
}

wire_struct! {
  pub struct GetPeerAddressRequest {
    pub username: String,
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct GetPeerAddressResponse {
  pub username: String,
  /// `0.0.0.0` when the user is offline
  pub ip: Ipv4Addr,
  pub port: u32,
  /// `(obfuscation_type, obfuscated_port)`, absent from older servers; never dialled
  pub obfuscation: Option<(u32, u16)>,
}

impl Wire for GetPeerAddressResponse {
  const MIN_BYTES: usize = 12;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string()?,
      ip: r.ip4()?,
      port: r.u32()?,
      // the obfuscated port is two bytes here, four in ConnectToPeer
      obfuscation: if r.remaining() >= 6 {
        Some((r.u32()?, r.u16()?))
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    w.ip4(self.ip);
    w.u32(self.port);
    if let Some((obfuscation_type, obfuscated_port)) = self.obfuscation {
      w.u32(obfuscation_type);
      w.u16(obfuscated_port);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

wire_struct! {
  pub struct WatchUserRequest {
    pub username: String,
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct WatchUserResponse {
  pub username: String,
  /// `None` when the user does not exist
  pub user: Option<WatchedUser>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct WatchedUser {
  pub status: UserPresence,
  pub stats: UserStats,
  /// present only when the user is not offline, and may be blank
  pub country_code: Option<String>,
}

impl Wire for WatchUserResponse {
  const MIN_BYTES: usize = 5;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let username = r.string()?;
    let user = if r.bool()? {
      Some(WatchedUser {
        status: UserPresence::read(r)?,
        stats: UserStats::read(r)?,
        country_code: if r.has_more() {
          Some(r.string()?)
        } else {
          None
        },
      })
    } else {
      None
    };
    Ok(Self { username, user })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    w.bool(self.user.is_some());
    if let Some(user) = &self.user {
      user.status.write(w);
      user.stats.write(w);
      if let Some(country_code) = &user.country_code {
        w.string(country_code);
      }
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

wire_struct! {
  pub struct UnwatchUser {
    pub username: String,
  }
}

wire_struct! {
  pub struct GetStatusRequest {
    pub username: String,
  }
}

wire_struct! {
  /// The `GetStatus` reply, also pushed for watched users and room members
  pub struct UserStatus {
    pub username: String,
    pub presence: UserPresence,
    pub is_privileged: bool,
  }
}

wire_struct! {
  /// obsolete
  pub struct IgnoreUser {
    pub username: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct UnignoreUser {
    pub username: String,
  }
}

wire_struct! {
  pub struct SayInChatRoomRequest {
    pub room: String,
    pub message: String,
  }
}

wire_struct! {
  pub struct SayInChatRoomResponse {
    pub room: String,
    pub username: String,
    pub message: String,
  }
}

wire_struct! {
  pub struct JoinRoomRequest {
    pub room: String,
    /// the type of room to create if it does not exist
    pub room_type: RoomType,
  }
}

/// The `JoinRoom` reply
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RoomData {
  pub name: String,
  /// for a private room this includes the owner and the operators
  pub users: Vec<UserData>,
  /// present exactly when the room is private
  pub private: Option<PrivateRoomData>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct PrivateRoomData {
  pub owner: String,
  pub operators: Vec<String>,
}

impl RoomData {
  pub fn user_count(&self) -> usize {
    self.users.len()
  }

  pub fn is_private(&self) -> bool {
    self.private.is_some()
  }
}

impl Wire for RoomData {
  const MIN_BYTES: usize = 24;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let name = r.string()?;
    let users = read_user_lists(r)?;
    // only private rooms carry the owner and operators
    let private = if r.has_more() {
      Some(PrivateRoomData {
        owner: r.string()?,
        operators: Wire::read_field(r, "operators")?,
      })
    } else {
      None
    };
    Ok(Self {
      name,
      users,
      private,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.name);
    write_user_lists(w, &self.users);
    if let Some(private) = &self.private {
      w.string(&private.owner);
      private.operators.write(w);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.room = Some(&self.name);
    fields.count = Some(self.users.len());
  }
}

wire_struct! {
  pub struct LeaveRoom {
    pub room: String,
  }
}

/// Pushed when someone joins a room we are in, ourselves included
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserJoinedRoom {
  pub room: String,
  pub user: UserData,
}

impl Wire for UserJoinedRoom {
  const MIN_BYTES: usize = 40;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      room: r.string()?,
      user: UserData {
        username: r.string()?,
        status: UserPresence::read(r)?,
        stats: UserStats::read(r)?,
        slots_full: r.u32()? != 0,
        country_code: r.string()?,
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.room);
    w.string(&self.user.username);
    self.user.status.write(w);
    self.user.stats.write(w);
    w.u32(u32::from(self.user.slots_full));
    w.string(&self.user.country_code);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.room = Some(&self.room);
    fields.username = Some(&self.user.username);
  }
}

wire_struct! {
  /// Pushed when someone else leaves a room we are in
  pub struct UserLeftRoom {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  pub struct ConnectToPeerRequest {
    /// fresh; the peer sends it back in `PierceFirewall`
    pub token: u32,
    pub username: String,
    pub connection_type: PeerConnectionType,
  }
}

/// A peer asked the server to make us connect to it
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ConnectToPeerResponse {
  pub username: String,
  pub connection_type: PeerConnectionType,
  pub ip: Ipv4Addr,
  pub port: u32,
  /// send back in `PierceFirewall`
  pub token: u32,
  pub privileged: bool,
  /// `(obfuscation_type, obfuscated_port)`; never dialled
  pub obfuscation: Option<(u32, u32)>,
}

impl Wire for ConnectToPeerResponse {
  const MIN_BYTES: usize = 21;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string()?,
      connection_type: PeerConnectionType::read(r)?,
      ip: r.ip4()?,
      port: r.u32()?,
      token: r.u32()?,
      privileged: r.bool()?,
      obfuscation: if r.remaining() >= 8 {
        Some((r.u32()?, r.u32()?))
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    self.connection_type.write(w);
    w.ip4(self.ip);
    w.u32(self.port);
    w.u32(self.token);
    w.bool(self.privileged);
    if let Some((obfuscation_type, obfuscated_port)) = self.obfuscation {
      w.u32(obfuscation_type);
      w.u32(obfuscated_port);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
    fields.token = Some(self.token);
  }
}

wire_struct! {
  pub struct PrivateMessageRequest {
    pub username: String,
    pub message: String,
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct PrivateMessageResponse {
  /// acknowledge with `AcknowledgePrivateMessage`
  pub id: u32,
  /// unix seconds, UTC
  pub timestamp: u32,
  pub username: String,
  pub message: String,
  /// false when replayed from the server's offline store
  pub is_new: bool,
}

impl Wire for PrivateMessageResponse {
  const MIN_BYTES: usize = 17;

  /// `is_new` is true only for exactly 1
  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      id: r.u32()?,
      timestamp: r.u32()?,
      username: r.string()?,
      message: r.string()?,
      is_new: r.bool_exact()?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.id);
    w.u32(self.timestamp);
    w.string(&self.username);
    w.string(&self.message);
    w.bool(self.is_new);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
    fields.token = Some(self.id);
  }
}

wire_struct! {
  pub struct AcknowledgePrivateMessage {
    pub id: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct FileSearchRoom {
    pub token: u32,
    pub room_id: u32,
    pub query: String,
  }
}

wire_struct! {
  pub struct FileSearchRequest {
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// A search the server asks us to answer
  pub struct FileSearchResponse {
    pub username: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  pub struct SetOnlineStatus {
    pub status: UserPresence,
  }
}

wire_struct! {
  /// obsolete
  pub struct SendConnectToken {
    pub username: String,
    pub token: u32,
  }
}

wire_struct! {
  /// obsolete `SendDownloadSpeed`
  pub struct SendSpeed {
    pub username: String,
    pub speed: u32,
  }
}

wire_struct! {
  pub struct SharedFoldersAndFiles {
    pub directory_count: u32,
    pub file_count: u32,
  }
}

wire_struct! {
  pub struct GetUserStatsRequest {
    pub username: String,
  }
}

wire_struct! {
  /// The `GetUserStats` reply, also pushed for users who share a room with us
  pub struct UserStatistics {
    pub username: String,
    pub stats: UserStats,
  }
}

/// obsolete `UploadSlotsFull`, a boolean carried as `u32`
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct QueuedDownloadsRequest {
  pub slots_full: bool,
}

impl Wire for QueuedDownloadsRequest {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      slots_full: r.u32()? != 0,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(u32::from(self.slots_full));
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct QueuedDownloadsResponse {
  pub username: String,
  pub slots_full: bool,
}

impl Wire for QueuedDownloadsResponse {
  const MIN_BYTES: usize = 8;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string()?,
      slots_full: r.u32()? != 0,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    w.u32(u32::from(self.slots_full));
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

wire_struct! {
  pub struct UserSearch {
    pub username: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct SimilarRecommendationsRequest {
    pub recommendation: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct SimilarRecommendationsResponse {
    pub recommendation: String,
    pub similar: Vec<String>,
  }
}

wire_struct! {
  pub struct LikedInterestAdd {
    pub interest: String,
  }
}

wire_struct! {
  pub struct LikedInterestRemove {
    pub interest: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct MyRecommendationsResponse {
    pub likes: Vec<String>,
  }
}

wire_struct! {
  pub struct GetUserInterestsRequest {
    pub username: String,
  }
}

wire_struct! {
  /// The `GetUserInterests` reply
  pub struct UserInterests {
    pub username: String,
    pub likes: Vec<String>,
    pub hates: Vec<String>,
  }
}

wire_struct! {
  /// obsolete
  pub struct AdminCommand {
    pub command: String,
    pub args: Vec<String>,
  }
}

wire_struct! {
  /// obsolete
  pub struct PlaceInLineRequest {
    pub username: String,
    pub token: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct PlaceInLineResponse {
    pub username: String,
    pub token: u32,
    pub place: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct RoomAdded {
    pub room: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct RoomRemoved {
    pub room: String,
  }
}

/// The `RoomList` reply. Each block is a list of names followed by a list of user counts of
/// the same length.
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

impl Wire for RoomList {
  const MIN_BYTES: usize = 28;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      public: read_room_block(r, "public rooms")?,
      owned: read_room_block(r, "owned rooms")?,
      private: read_room_block(r, "private rooms")?,
      moderated_room_names: Wire::read_field(r, "operated rooms")?,
    })
  }

  fn write(&self, w: &mut Writer) {
    write_room_block(w, &self.public);
    write_room_block(w, &self.owned);
    write_room_block(w, &self.private);
    self.moderated_room_names.write(w);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.count = Some(self.public.len());
  }
}

wire_struct! {
  /// obsolete
  pub struct ExactFileSearchRequest {
    pub token: u32,
    pub filename: RemoteFilename,
    pub path: RemoteFilename,
    pub file_size: u64,
    pub checksum: u32,
    pub unknown: u8,
  }
}

wire_struct! {
  /// obsolete
  pub struct ExactFileSearchResponse {
    pub username: String,
    pub token: u32,
    pub filename: RemoteFilename,
    pub path: RemoteFilename,
    pub file_size: u64,
    pub checksum: u32,
  }
}

wire_struct! {
  pub struct GlobalAdminMessage {
    pub message: String,
  }
}

/// obsolete: the same five parallel lists as `JoinRoom`, without a room name
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct GlobalUserListResponse {
  pub users: Vec<UserData>,
}

impl Wire for GlobalUserListResponse {
  const MIN_BYTES: usize = 20;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      users: read_user_lists(r)?,
    })
  }

  fn write(&self, w: &mut Writer) {
    write_user_lists(w, &self.users);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.count = Some(self.users.len());
  }
}

wire_struct! {
  /// obsolete; the field order differs from the response
  pub struct TunneledMessageRequest {
    pub username: String,
    pub token: u32,
    pub code: u32,
    pub message: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct TunneledMessageResponse {
    pub username: String,
    pub code: u32,
    pub token: u32,
    pub ip: Ipv4Addr,
    pub port: u32,
    pub message: String,
  }
}

wire_struct! {
  pub struct PrivilegedUsers {
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  pub struct HaveNoParents {
    pub have_no_parents: bool,
  }
}

/// deprecated, never sent; an empty payload means no parent
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ParentsIP {
  pub ip: Option<Ipv4Addr>,
}

impl Wire for ParentsIP {
  const MIN_BYTES: usize = 0;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      ip: if r.has_more() { Some(r.ip4()?) } else { None },
    })
  }

  fn write(&self, w: &mut Writer) {
    if let Some(ip) = self.ip {
      w.ip4(ip);
    }
  }
}

wire_struct! {
  pub struct ParentMinSpeed {
    pub speed: u32,
  }
}

wire_struct! {
  pub struct ParentSpeedRatio {
    pub ratio: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct ParentInactivityTimeout {
    pub seconds: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct SearchInactivityTimeout {
    pub seconds: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct MinimumParentsInCache {
    pub count: u32,
  }
}

wire_struct! {
  /// obsolete
  pub struct DistributedAliveInterval {
    pub seconds: u32,
  }
}

wire_struct! {
  /// obsolete, the parser is kept
  pub struct AddPrivilegedUser {
    pub username: String,
  }
}

wire_struct! {
  pub struct CheckPrivilegesResponse {
    pub seconds_remaining: u32,
  }
}

wire_struct! {
  pub struct AcceptChildren {
    pub accept: bool,
  }
}

wire_struct! {
  /// Parent candidates, sent after `HaveNoParents(true)`
  pub struct NetInfo {
    pub parents: Vec<ParentCandidate>,
  }
}

wire_struct! {
  pub struct ParentCandidate {
    pub username: String,
    pub ip: Ipv4Addr,
    pub port: u32,
  }
}

wire_struct! {
  pub struct WishlistSearch {
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  pub struct WishlistInterval {
    pub seconds: u32,
  }
}

wire_struct! {
  pub struct GetPersonalSimilarUsersResponse {
    pub users: Vec<SimilarUser>,
  }
}

wire_struct! {
  pub struct SimilarUser {
    pub username: String,
    pub rating: u32,
  }
}

wire_struct! {
  pub struct GetInterestRecommendationsRequest {
    pub interest: String,
  }
}

wire_struct! {
  pub struct GetInterestRecommendationsResponse {
    pub interest: String,
    pub recommendations: Recommendations,
  }
}

wire_struct! {
  pub struct GetInterestSimilarUsersRequest {
    pub interest: String,
  }
}

wire_struct! {
  pub struct GetInterestSimilarUsersResponse {
    pub interest: String,
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  /// All tickers of a room, pushed on join
  pub struct RoomTickers {
    pub room: String,
    pub tickers: Vec<RoomTicker>,
  }
}

wire_struct! {
  pub struct RoomTicker {
    pub username: String,
    pub message: String,
  }
}

wire_struct! {
  pub struct RoomTickerAdd {
    pub room: String,
    pub ticker: RoomTicker,
  }
}

wire_struct! {
  pub struct RoomTickerRemove {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  pub struct SetRoomTicker {
    pub room: String,
    /// empty clears the ticker
    pub ticker: String,
  }
}

wire_struct! {
  pub struct HatedInterestAdd {
    pub interest: String,
  }
}

wire_struct! {
  pub struct HatedInterestRemove {
    pub interest: String,
  }
}

wire_struct! {
  pub struct RoomSearch {
    pub room: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  pub struct SendUploadSpeed {
    /// bytes/second
    pub speed: u32,
  }
}

wire_struct! {
  /// deprecated
  pub struct UserPrivilegesRequest {
    pub username: String,
  }
}

wire_struct! {
  /// deprecated
  pub struct UserPrivilegesResponse {
    pub username: String,
    pub privileged: bool,
  }
}

wire_struct! {
  pub struct GivePrivileges {
    pub username: String,
    pub days: u32,
  }
}

wire_struct! {
  /// deprecated
  pub struct NotifyPrivileges {
    pub id: u32,
    pub username: String,
  }
}

wire_struct! {
  /// deprecated
  pub struct AcknowledgeNotifyPrivileges {
    pub id: u32,
  }
}

wire_struct! {
  pub struct BranchLevel {
    pub level: u32,
  }
}

wire_struct! {
  pub struct BranchRoot {
    pub root: String,
  }
}

wire_struct! {
  /// obsolete, never sent
  pub struct ChildDepth {
    pub depth: u32,
  }
}

wire_struct! {
  /// Members of a private room, excluding the owner
  pub struct PrivateRoomUsers {
    pub room: String,
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  pub struct PrivateRoomAddUser {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  pub struct PrivateRoomRemoveUser {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  pub struct PrivateRoomDropMembership {
    pub room: String,
  }
}

wire_struct! {
  pub struct PrivateRoomDropOwnership {
    pub room: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct PrivateRoomUnknown {
    pub room: String,
  }
}

wire_struct! {
  pub struct PrivateRoomAdded {
    pub room: String,
  }
}

wire_struct! {
  pub struct PrivateRoomRemoved {
    pub room: String,
  }
}

wire_struct! {
  pub struct PrivateRoomToggle {
    pub accept_invitations: bool,
  }
}

wire_struct! {
  pub struct NewPassword {
    pub password: Secret,
  }
}

wire_struct! {
  pub struct PrivateRoomAddOperator {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  pub struct PrivateRoomRemoveOperator {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  pub struct PrivateRoomOperatorAdded {
    pub room: String,
  }
}

wire_struct! {
  pub struct PrivateRoomOperatorRemoved {
    pub room: String,
  }
}

wire_struct! {
  /// The operators of a private room we are in
  pub struct PrivateRoomOwned {
    pub room: String,
    pub operators: Vec<String>,
  }
}

wire_struct! {
  /// codec only
  pub struct MessageUsers {
    pub usernames: Vec<String>,
    pub message: String,
  }
}

wire_struct! {
  pub struct PublicChat {
    pub room: String,
    pub username: String,
    pub message: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct RelatedSearchRequest {
    pub query: String,
  }
}

wire_struct! {
  /// obsolete
  pub struct RelatedSearchResponse {
    pub query: String,
    pub terms: Vec<RelatedTerm>,
  }
}

wire_struct! {
  pub struct RelatedTerm {
    pub term: String,
    pub score: u32,
  }
}

wire_struct! {
  pub struct ExcludedSearchPhrases {
    pub phrases: Vec<String>,
  }
}

wire_struct! {
  pub struct CannotConnectRequest {
    pub token: u32,
    /// the peer we could not reach
    pub username: String,
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct CannotConnectResponse {
  pub token: u32,
  /// the live server sends only the token
  pub username: Option<String>,
}

impl Wire for CannotConnectResponse {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      token: r.u32()?,
      username: if r.has_more() {
        Some(r.string()?)
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.token);
    if let Some(username) = &self.username {
      w.string(username);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.token = Some(self.token);
    fields.username = self.username.as_deref();
  }
}

wire_struct! {
  /// A `JoinRoom` would have created a room that clashes with an existing private room
  pub struct CannotJoinRoom {
    pub room: String,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::testing::{P, decodes, first, p, vectors};
  use ServerRequest as Q;
  use ServerResponse as R;

  const MD5_USERNAME_PASSWORD: &str = "d51c9a7e9353746a6020f9602d452929";
  const IP: [u8; 4] = [192, 168, 1, 20];

  fn ip() -> Ipv4Addr {
    Ipv4Addr::from(IP)
  }

  fn stats() -> UserStats {
    UserStats {
      average_speed: 51_200,
      upload_count: 12,
      file_count: 3_400,
      directory_count: 210,
    }
  }

  /// the stats tuple with its always-zero third field
  fn stats_bytes(b: P) -> P {
    b.u32(51_200).u32(12).u32(0).u32(3_400).u32(210)
  }

  fn user(username: &str, status: UserPresence, slots_full: bool, country_code: &str) -> UserData {
    UserData {
      username: username.into(),
      status,
      stats: stats(),
      slots_full,
      country_code: country_code.into(),
    }
  }

  fn embedded_search() -> Vec<u8> {
    p().u32(49).s("bob").u32(5).s("query").0
  }

  #[test]
  fn login_hash_is_md5_of_username_and_password() {
    assert_eq!(login_hash("username", "password"), MD5_USERNAME_PASSWORD);
  }

  #[test]
  fn login_request_debug_never_shows_credentials() {
    let login = LoginRequest::new("alice".into(), "hunter2".into(), 1, 2);
    let debug = format!("{login:?}");
    assert!(!debug.contains("hunter2"));
    assert!(!debug.contains(&login_hash("alice", "hunter2")));
    assert!(debug.contains("alice"));
  }

  vectors! {
    login_request:
      Q::Login(LoginRequest::new("username".into(), "password".into(), 177, 760))
      => p().s("username").s("password").u32(177).s(MD5_USERNAME_PASSWORD).u32(760).frame(1);
    login_response_success:
      R::Login(LoginResponse::Success {
        greeting: "welcome\nto soulseek".into(), ip: ip(), hash: Secret::new(MD5_USERNAME_PASSWORD), is_supporter: true,
      })
      => p().u8(1).s("welcome\nto soulseek").ip(IP).s(MD5_USERNAME_PASSWORD).u8(1).frame(1);
    login_response_failure:
      R::Login(LoginResponse::Failure { reason: "INVALIDPASS".into(), detail: None }) => p().u8(0).s("INVALIDPASS").frame(1);
    login_response_failure_with_detail:
      R::Login(LoginResponse::Failure { reason: "INVALIDUSERNAME".into(), detail: Some("Nick too long.".into()) })
      => p().u8(0).s("INVALIDUSERNAME").s("Nick too long.").frame(1);
    set_listen_port_request: Q::SetListenPort(SetListenPort { port: 50_000, obfuscation: None }) => p().u32(50_000).frame(2);
    set_listen_port_request_obfuscated:
      Q::SetListenPort(SetListenPort { port: 50_000, obfuscation: Some((1, 50_001)) })
      => p().u32(50_000).u32(1).u32(50_001).frame(2);
    get_peer_address_request: Q::GetPeerAddress(GetPeerAddressRequest { username: "bob".into() }) => p().s("bob").frame(3);
    get_peer_address_response:
      R::GetPeerAddress(GetPeerAddressResponse { username: "bob".into(), ip: ip(), port: 2234, obfuscation: Some((1, 2235)) })
      => p().s("bob").ip(IP).u32(2234).u32(1).u16(2235).frame(3);
    get_peer_address_response_without_trailer:
      R::GetPeerAddress(GetPeerAddressResponse { username: "bob".into(), ip: Ipv4Addr::UNSPECIFIED, port: 0, obfuscation: None })
      => p().s("bob").ip([0, 0, 0, 0]).u32(0).frame(3);
    watch_user_request: Q::WatchUser(WatchUserRequest { username: "bob".into() }) => p().s("bob").frame(5);
    watch_user_response:
      R::WatchUser(WatchUserResponse {
        username: "bob".into(),
        user: Some(WatchedUser { status: UserPresence::Online, stats: stats(), country_code: Some("NL".into()) }),
      })
      => stats_bytes(p().s("bob").u8(1).u32(2)).s("NL").frame(5);
    watch_user_response_blank_country:
      R::WatchUser(WatchUserResponse {
        username: "bob".into(),
        user: Some(WatchedUser { status: UserPresence::Away, stats: stats(), country_code: Some(String::new()) }),
      })
      => stats_bytes(p().s("bob").u8(1).u32(1)).s("").frame(5);
    watch_user_response_offline:
      R::WatchUser(WatchUserResponse {
        username: "bob".into(),
        user: Some(WatchedUser { status: UserPresence::Offline, stats: stats(), country_code: None }),
      })
      => stats_bytes(p().s("bob").u8(1).u32(0)).frame(5);
    watch_user_response_missing:
      R::WatchUser(WatchUserResponse { username: "nobody".into(), user: None }) => p().s("nobody").u8(0).frame(5);
    unwatch_user_request: Q::UnwatchUser(UnwatchUser { username: "bob".into() }) => p().s("bob").frame(6);
    get_status_request: Q::GetStatus(GetStatusRequest { username: "bob".into() }) => p().s("bob").frame(7);
    get_status_response:
      R::GetStatus(UserStatus { username: "bob".into(), presence: UserPresence::Away, is_privileged: true })
      => p().s("bob").u32(1).u8(1).frame(7);
    ignore_user_request: Q::IgnoreUser(IgnoreUser { username: "bob".into() }) => p().s("bob").frame(11);
    ignore_user_response: R::IgnoreUser(IgnoreUser { username: "bob".into() }) => p().s("bob").frame(11);
    unignore_user_request: Q::UnignoreUser(UnignoreUser { username: "bob".into() }) => p().s("bob").frame(12);
    unignore_user_response: R::UnignoreUser(UnignoreUser { username: "bob".into() }) => p().s("bob").frame(12);
    say_in_chat_room_request:
      Q::SayInChatRoom(SayInChatRoomRequest { room: "indie".into(), message: "hi all".into() })
      => p().s("indie").s("hi all").frame(13);
    say_in_chat_room_response:
      R::SayInChatRoom(SayInChatRoomResponse { room: "indie".into(), username: "bob".into(), message: "hi".into() })
      => p().s("indie").s("bob").s("hi").frame(13);
    join_room_request:
      Q::JoinRoom(JoinRoomRequest { room: "indie".into(), room_type: RoomType::Private }) => p().s("indie").u32(1).frame(14);
    join_room_response:
      R::JoinRoom(RoomData {
        name: "indie".into(),
        users: vec![user("bob", UserPresence::Online, false, "NL"), user("eve", UserPresence::Away, true, "")],
        private: None,
      })
      => stats_bytes(stats_bytes(p().s("indie").u32(2).s("bob").s("eve").u32(2).u32(2).u32(1).u32(2)))
        .u32(2).u32(0).u32(1).u32(2).s("NL").s("").frame(14);
    join_room_response_private:
      R::JoinRoom(RoomData {
        name: "club".into(),
        users: vec![user("owner", UserPresence::Online, false, "DE")],
        private: Some(PrivateRoomData { owner: "owner".into(), operators: vec!["op".into()] }),
      })
      => stats_bytes(p().s("club").u32(1).s("owner").u32(1).u32(2).u32(1))
        .u32(1).u32(0).u32(1).s("DE").s("owner").u32(1).s("op").frame(14);
    leave_room_request: Q::LeaveRoom(LeaveRoom { room: "indie".into() }) => p().s("indie").frame(15);
    leave_room_response: R::LeaveRoom(LeaveRoom { room: "indie".into() }) => p().s("indie").frame(15);
    user_joined_room_response:
      R::UserJoinedRoom(UserJoinedRoom { room: "indie".into(), user: user("bob", UserPresence::Online, true, "FR") })
      => stats_bytes(p().s("indie").s("bob").u32(2)).u32(1).s("FR").frame(16);
    user_left_room_response:
      R::UserLeftRoom(UserLeftRoom { room: "indie".into(), username: "bob".into() }) => p().s("indie").s("bob").frame(17);
    connect_to_peer_request:
      Q::ConnectToPeer(ConnectToPeerRequest { token: 7, username: "bob".into(), connection_type: PeerConnectionType::Peer })
      => p().u32(7).s("bob").s("P").frame(18);
    connect_to_peer_response:
      R::ConnectToPeer(ConnectToPeerResponse {
        username: "bob".into(), connection_type: PeerConnectionType::Transfer, ip: ip(), port: 2234, token: 77,
        privileged: false, obfuscation: Some((1, 2235)),
      })
      => p().s("bob").s("F").ip(IP).u32(2234).u32(77).u8(0).u32(1).u32(2235).frame(18);
    connect_to_peer_response_without_trailer:
      R::ConnectToPeer(ConnectToPeerResponse {
        username: "bob".into(), connection_type: PeerConnectionType::Distributed, ip: ip(), port: 2234, token: 77,
        privileged: true, obfuscation: None,
      })
      => p().s("bob").s("D").ip(IP).u32(2234).u32(77).u8(1).frame(18);
    private_message_request:
      Q::PrivateMessage(PrivateMessageRequest { username: "bob".into(), message: "hello\nthere".into() })
      => p().s("bob").s("hello\nthere").frame(22);
    private_message_response:
      R::PrivateMessage(PrivateMessageResponse {
        id: 5, timestamp: 1_700_000_000, username: "bob".into(), message: "yo".into(), is_new: true,
      })
      => p().u32(5).u32(1_700_000_000).s("bob").s("yo").u8(1).frame(22);
    private_message_response_replayed:
      R::PrivateMessage(PrivateMessageResponse {
        id: 5, timestamp: 1_700_000_000, username: "server".into(), message: "x".into(), is_new: false,
      })
      => p().u32(5).u32(1_700_000_000).s("server").s("x").u8(0).frame(22);
    acknowledge_private_message_request:
      Q::AcknowledgePrivateMessage(AcknowledgePrivateMessage { id: 99 }) => p().u32(99).frame(23);
    file_search_room_request:
      Q::FileSearchRoom(FileSearchRoom { token: 8, room_id: 3, query: "a b".into() }) => p().u32(8).u32(3).s("a b").frame(25);
    file_search_request:
      Q::FileSearch(FileSearchRequest { token: 8, query: "boards of canada".into() })
      => p().u32(8).s("boards of canada").frame(26);
    file_search_response:
      R::FileSearch(FileSearchResponse { username: "bob".into(), token: 3, query: "aphex".into() })
      => p().s("bob").u32(3).s("aphex").frame(26);
    set_online_status_request:
      Q::SetOnlineStatus(SetOnlineStatus { status: UserPresence::Away }) => p().u32(1).frame(28);
    ping_request: Q::Ping => p().frame(32);
    ping_response: R::Ping => p().frame(32);
    send_connect_token_request:
      Q::SendConnectToken(SendConnectToken { username: "bob".into(), token: 5 }) => p().s("bob").u32(5).frame(33);
    send_connect_token_response:
      R::SendConnectToken(SendConnectToken { username: "bob".into(), token: 5 }) => p().s("bob").u32(5).frame(33);
    send_speed_request: Q::SendSpeed(SendSpeed { username: "bob".into(), speed: 1000 }) => p().s("bob").u32(1000).frame(34);
    shared_folders_and_files_request:
      Q::SharedFoldersAndFiles(SharedFoldersAndFiles { directory_count: 12, file_count: 345 }) => p().u32(12).u32(345).frame(35);
    get_user_stats_request: Q::GetUserStats(GetUserStatsRequest { username: "bob".into() }) => p().s("bob").frame(36);
    get_user_stats_response:
      R::GetUserStats(UserStatistics { username: "bob".into(), stats: stats() }) => stats_bytes(p().s("bob")).frame(36);
    queued_downloads_request: Q::QueuedDownloads(QueuedDownloadsRequest { slots_full: true }) => p().u32(1).frame(40);
    queued_downloads_response:
      R::QueuedDownloads(QueuedDownloadsResponse { username: "bob".into(), slots_full: false }) => p().s("bob").u32(0).frame(40);
    kicked_from_server_response: R::KickedFromServer => p().frame(41);
    user_search_request:
      Q::UserSearch(UserSearch { username: "bob".into(), token: 9, query: "flac".into() }) => p().s("bob").u32(9).s("flac").frame(42);
    similar_recommendations_request:
      Q::SimilarRecommendations(SimilarRecommendationsRequest { recommendation: "jazz".into() }) => p().s("jazz").frame(50);
    similar_recommendations_response:
      R::SimilarRecommendations(SimilarRecommendationsResponse { recommendation: "jazz".into(), similar: vec![] })
      => p().s("jazz").u32(0).frame(50);
    liked_interest_add_request: Q::LikedInterestAdd(LikedInterestAdd { interest: "jazz".into() }) => p().s("jazz").frame(51);
    liked_interest_remove_request:
      Q::LikedInterestRemove(LikedInterestRemove { interest: "jazz".into() }) => p().s("jazz").frame(52);
    get_personal_recommendations_request: Q::GetPersonalRecommendations => p().frame(54);
    get_personal_recommendations_response:
      R::GetPersonalRecommendations(Recommendations {
        recommendations: vec![Recommendation { interest: "jazz".into(), count: 4 }],
        unrecommendations: vec![Recommendation { interest: "noise".into(), count: -2 }],
      })
      => p().u32(1).s("jazz").i32(4).u32(1).s("noise").i32(-2).frame(54);
    my_recommendations_request: Q::MyRecommendations => p().frame(55);
    my_recommendations_response:
      R::MyRecommendations(MyRecommendationsResponse { likes: vec!["jazz".into()] }) => p().u32(1).s("jazz").frame(55);
    get_global_recommendations_request: Q::GetGlobalRecommendations => p().frame(56);
    get_global_recommendations_response:
      R::GetGlobalRecommendations(Recommendations {
        recommendations: vec![Recommendation { interest: "pop".into(), count: 900 }],
        unrecommendations: vec![],
      })
      => p().u32(1).s("pop").i32(900).u32(0).frame(56);
    get_user_interests_request: Q::GetUserInterests(GetUserInterestsRequest { username: "bob".into() }) => p().s("bob").frame(57);
    get_user_interests_response:
      R::GetUserInterests(UserInterests { username: "bob".into(), likes: vec!["jazz".into()], hates: vec!["noise".into()] })
      => p().s("bob").u32(1).s("jazz").u32(1).s("noise").frame(57);
    admin_command_request:
      Q::AdminCommand(AdminCommand { command: "ban".into(), args: vec!["bob".into(), "1d".into()] })
      => p().s("ban").u32(2).s("bob").s("1d").frame(58);
    place_in_line_request_request:
      Q::PlaceInLineRequest(PlaceInLineRequest { username: "bob".into(), token: 4 }) => p().s("bob").u32(4).frame(59);
    place_in_line_request_response:
      R::PlaceInLineRequest(PlaceInLineRequest { username: "bob".into(), token: 4 }) => p().s("bob").u32(4).frame(59);
    place_in_line_response_request:
      Q::PlaceInLineResponse(PlaceInLineResponse { username: "bob".into(), token: 4, place: 2 })
      => p().s("bob").u32(4).u32(2).frame(60);
    place_in_line_response_response:
      R::PlaceInLineResponse(PlaceInLineResponse { username: "bob".into(), token: 4, place: 9 })
      => p().s("bob").u32(4).u32(9).frame(60);
    room_added_response: R::RoomAdded(RoomAdded { room: "new".into() }) => p().s("new").frame(62);
    room_removed_response: R::RoomRemoved(RoomRemoved { room: "old".into() }) => p().s("old").frame(63);
    room_list_request: Q::RoomList => p().frame(64);
    room_list_response:
      R::RoomList(RoomList {
        public: vec![RoomInfo { name: "indie".into(), user_count: 40 }, RoomInfo { name: "jazz".into(), user_count: 7 }],
        owned: vec![RoomInfo { name: "mine".into(), user_count: 2 }],
        private: vec![],
        moderated_room_names: vec!["club".into()],
      })
      => p().u32(2).s("indie").s("jazz").u32(2).u32(40).u32(7).u32(1).s("mine").u32(1).u32(2)
        .u32(0).u32(0).u32(1).s("club").frame(64);
    exact_file_search_request:
      Q::ExactFileSearch(ExactFileSearchRequest {
        token: 3, filename: "a.mp3".into(), path: "music\\a".into(), file_size: 1 << 33, checksum: 77, unknown: 1,
      })
      => p().u32(3).s("a.mp3").s("music\\a").u64(1 << 33).u32(77).u8(1).frame(65);
    exact_file_search_response:
      R::ExactFileSearch(ExactFileSearchResponse {
        username: "bob".into(), token: 3, filename: "a.mp3".into(), path: "music".into(), file_size: 99, checksum: 1,
      })
      => p().s("bob").u32(3).s("a.mp3").s("music").u64(99).u32(1).frame(65);
    global_admin_message_response:
      R::GlobalAdminMessage(GlobalAdminMessage { message: "maintenance".into() }) => p().s("maintenance").frame(66);
    global_user_list_request: Q::GlobalUserList => p().frame(67);
    global_user_list_response:
      R::GlobalUserList(GlobalUserListResponse { users: vec![user("bob", UserPresence::Online, false, "NL")] })
      => stats_bytes(p().u32(1).s("bob").u32(1).u32(2).u32(1)).u32(1).u32(0).u32(1).s("NL").frame(67);
    tunneled_message_request:
      Q::TunneledMessage(TunneledMessageRequest { username: "bob".into(), token: 1, code: 2, message: "m".into() })
      => p().s("bob").u32(1).u32(2).s("m").frame(68);
    tunneled_message_response:
      R::TunneledMessage(TunneledMessageResponse {
        username: "bob".into(), code: 2, token: 1, ip: ip(), port: 2234, message: "m".into(),
      })
      => p().s("bob").u32(2).u32(1).ip(IP).u32(2234).s("m").frame(68);
    privileged_users_response:
      R::PrivilegedUsers(PrivilegedUsers { usernames: vec!["bob".into(), "eve".into()] }) => p().u32(2).s("bob").s("eve").frame(69);
    have_no_parents_request: Q::HaveNoParents(HaveNoParents { have_no_parents: true }) => p().u8(1).frame(71);
    parents_ip_request: Q::ParentsIP(ParentsIP { ip: Some(ip()) }) => p().ip(IP).frame(73);
    parents_ip_request_without_parent: Q::ParentsIP(ParentsIP { ip: None }) => p().frame(73);
    parent_min_speed_response: R::ParentMinSpeed(ParentMinSpeed { speed: 1 }) => p().u32(1).frame(83);
    parent_speed_ratio_response: R::ParentSpeedRatio(ParentSpeedRatio { ratio: 50 }) => p().u32(50).frame(84);
    parent_inactivity_timeout_response:
      R::ParentInactivityTimeout(ParentInactivityTimeout { seconds: 300 }) => p().u32(300).frame(86);
    search_inactivity_timeout_response:
      R::SearchInactivityTimeout(SearchInactivityTimeout { seconds: 0 }) => p().u32(0).frame(87);
    minimum_parents_in_cache_response: R::MinimumParentsInCache(MinimumParentsInCache { count: 10 }) => p().u32(10).frame(88);
    distributed_alive_interval_response:
      R::DistributedAliveInterval(DistributedAliveInterval { seconds: 0 }) => p().u32(0).frame(90);
    add_privileged_user_response: R::AddPrivilegedUser(AddPrivilegedUser { username: "bob".into() }) => p().s("bob").frame(91);
    check_privileges_request: Q::CheckPrivileges => p().frame(92);
    check_privileges_response:
      R::CheckPrivileges(CheckPrivilegesResponse { seconds_remaining: 86_400 }) => p().u32(86_400).frame(92);
    embedded_message_response:
      R::EmbeddedMessage(Embedded { distributed_code: 3, payload: embedded_search() })
      => p().u8(3).raw(&embedded_search()).frame(93);
    accept_children_request: Q::AcceptChildren(AcceptChildren { accept: false }) => p().u8(0).frame(100);
    net_info_response:
      R::NetInfo(NetInfo { parents: vec![ParentCandidate { username: "fast".into(), ip: ip(), port: 2234 }] })
      => p().u32(1).s("fast").ip(IP).u32(2234).frame(102);
    wishlist_search_request:
      Q::WishlistSearch(WishlistSearch { token: 10, query: "rare".into() }) => p().u32(10).s("rare").frame(103);
    wishlist_interval_response: R::WishlistInterval(WishlistInterval { seconds: 720 }) => p().u32(720).frame(104);
    get_personal_similar_users_request: Q::GetPersonalSimilarUsers => p().frame(110);
    get_personal_similar_users_response:
      R::GetPersonalSimilarUsers(GetPersonalSimilarUsersResponse { users: vec![SimilarUser { username: "bob".into(), rating: 3 }] })
      => p().u32(1).s("bob").u32(3).frame(110);
    get_interest_recommendations_request:
      Q::GetInterestRecommendations(GetInterestRecommendationsRequest { interest: "jazz".into() }) => p().s("jazz").frame(111);
    get_interest_recommendations_response:
      R::GetInterestRecommendations(GetInterestRecommendationsResponse {
        interest: "jazz".into(),
        recommendations: Recommendations {
          recommendations: vec![Recommendation { interest: "bebop".into(), count: 2 }],
          unrecommendations: vec![Recommendation { interest: "metal".into(), count: -1 }],
        },
      })
      => p().s("jazz").u32(1).s("bebop").i32(2).u32(1).s("metal").i32(-1).frame(111);
    get_interest_similar_users_request:
      Q::GetInterestSimilarUsers(GetInterestSimilarUsersRequest { interest: "jazz".into() }) => p().s("jazz").frame(112);
    get_interest_similar_users_response:
      R::GetInterestSimilarUsers(GetInterestSimilarUsersResponse { interest: "jazz".into(), usernames: vec!["bob".into()] })
      => p().s("jazz").u32(1).s("bob").frame(112);
    room_tickers_response:
      R::RoomTickers(RoomTickers {
        room: "indie".into(),
        tickers: vec![RoomTicker { username: "bob".into(), message: "listening".into() }],
      })
      => p().s("indie").u32(1).s("bob").s("listening").frame(113);
    room_ticker_add_response:
      R::RoomTickerAdd(RoomTickerAdd { room: "indie".into(), ticker: RoomTicker { username: "bob".into(), message: "t".into() } })
      => p().s("indie").s("bob").s("t").frame(114);
    room_ticker_remove_response:
      R::RoomTickerRemove(RoomTickerRemove { room: "indie".into(), username: "bob".into() }) => p().s("indie").s("bob").frame(115);
    set_room_ticker_request:
      Q::SetRoomTicker(SetRoomTicker { room: "indie".into(), ticker: "".into() }) => p().s("indie").s("").frame(116);
    hated_interest_add_request: Q::HatedInterestAdd(HatedInterestAdd { interest: "noise".into() }) => p().s("noise").frame(117);
    hated_interest_remove_request:
      Q::HatedInterestRemove(HatedInterestRemove { interest: "noise".into() }) => p().s("noise").frame(118);
    room_search_request:
      Q::RoomSearch(RoomSearch { room: "indie".into(), token: 11, query: "ep".into() }) => p().s("indie").u32(11).s("ep").frame(120);
    send_upload_speed_request: Q::SendUploadSpeed(SendUploadSpeed { speed: 123_456 }) => p().u32(123_456).frame(121);
    user_privileges_request: Q::UserPrivileges(UserPrivilegesRequest { username: "bob".into() }) => p().s("bob").frame(122);
    user_privileges_response:
      R::UserPrivileges(UserPrivilegesResponse { username: "bob".into(), privileged: true }) => p().s("bob").u8(1).frame(122);
    give_privileges_request:
      Q::GivePrivileges(GivePrivileges { username: "bob".into(), days: 30 }) => p().s("bob").u32(30).frame(123);
    notify_privileges_response:
      R::NotifyPrivileges(NotifyPrivileges { id: 4, username: "bob".into() }) => p().u32(4).s("bob").frame(124);
    acknowledge_notify_privileges_request:
      Q::AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges { id: 6 }) => p().u32(6).frame(125);
    branch_level_request: Q::BranchLevel(BranchLevel { level: 2 }) => p().u32(2).frame(126);
    branch_root_request: Q::BranchRoot(BranchRoot { root: "root".into() }) => p().s("root").frame(127);
    child_depth_request: Q::ChildDepth(ChildDepth { depth: 1 }) => p().u32(1).frame(129);
    distributed_reset_response: R::DistributedReset => p().frame(130);
    private_room_users_response:
      R::PrivateRoomUsers(PrivateRoomUsers { room: "club".into(), usernames: vec!["bob".into()] })
      => p().s("club").u32(1).s("bob").frame(133);
    private_room_add_user_request:
      Q::PrivateRoomAddUser(PrivateRoomAddUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(134);
    private_room_add_user_response:
      R::PrivateRoomAddUser(PrivateRoomAddUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(134);
    private_room_remove_user_request:
      Q::PrivateRoomRemoveUser(PrivateRoomRemoveUser { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(135);
    private_room_remove_user_response:
      R::PrivateRoomRemoveUser(PrivateRoomRemoveUser { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(135);
    private_room_drop_membership_request:
      Q::PrivateRoomDropMembership(PrivateRoomDropMembership { room: "club".into() }) => p().s("club").frame(136);
    private_room_drop_ownership_request:
      Q::PrivateRoomDropOwnership(PrivateRoomDropOwnership { room: "club".into() }) => p().s("club").frame(137);
    private_room_unknown_request: Q::PrivateRoomUnknown(PrivateRoomUnknown { room: "club".into() }) => p().s("club").frame(138);
    private_room_unknown_response: R::PrivateRoomUnknown(PrivateRoomUnknown { room: "club".into() }) => p().s("club").frame(138);
    private_room_added_response: R::PrivateRoomAdded(PrivateRoomAdded { room: "club".into() }) => p().s("club").frame(139);
    private_room_removed_response: R::PrivateRoomRemoved(PrivateRoomRemoved { room: "club".into() }) => p().s("club").frame(140);
    private_room_toggle_request:
      Q::PrivateRoomToggle(PrivateRoomToggle { accept_invitations: true }) => p().u8(1).frame(141);
    private_room_toggle_response:
      R::PrivateRoomToggle(PrivateRoomToggle { accept_invitations: false }) => p().u8(0).frame(141);
    new_password_request: Q::NewPassword(NewPassword { password: Secret::new("n3w") }) => p().s("n3w").frame(142);
    new_password_response: R::NewPassword(NewPassword { password: Secret::new("n3w") }) => p().s("n3w").frame(142);
    private_room_add_operator_request:
      Q::PrivateRoomAddOperator(PrivateRoomAddOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(143);
    private_room_add_operator_response:
      R::PrivateRoomAddOperator(PrivateRoomAddOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(143);
    private_room_remove_operator_request:
      Q::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(144);
    private_room_remove_operator_response:
      R::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(144);
    private_room_operator_added_response:
      R::PrivateRoomOperatorAdded(PrivateRoomOperatorAdded { room: "club".into() }) => p().s("club").frame(145);
    private_room_operator_removed_response:
      R::PrivateRoomOperatorRemoved(PrivateRoomOperatorRemoved { room: "club".into() }) => p().s("club").frame(146);
    private_room_owned_response:
      R::PrivateRoomOwned(PrivateRoomOwned { room: "club".into(), operators: vec!["op".into()] })
      => p().s("club").u32(1).s("op").frame(148);
    message_users_request:
      Q::MessageUsers(MessageUsers { usernames: vec!["bob".into(), "eve".into()], message: "hey".into() })
      => p().u32(2).s("bob").s("eve").s("hey").frame(149);
    ask_public_chat_request: Q::AskPublicChat => p().frame(150);
    stop_public_chat_request: Q::StopPublicChat => p().frame(151);
    public_chat_response:
      R::PublicChat(PublicChat { room: "indie".into(), username: "bob".into(), message: "hi".into() })
      => p().s("indie").s("bob").s("hi").frame(152);
    related_search_request: Q::RelatedSearch(RelatedSearchRequest { query: "jazz".into() }) => p().s("jazz").frame(153);
    related_search_response:
      R::RelatedSearch(RelatedSearchResponse { query: "jazz".into(), terms: vec![RelatedTerm { term: "bebop".into(), score: 5 }] })
      => p().s("jazz").u32(1).s("bebop").u32(5).frame(153);
    excluded_search_phrases_response:
      R::ExcludedSearchPhrases(ExcludedSearchPhrases { phrases: vec!["bad phrase".into()] }) => p().u32(1).s("bad phrase").frame(160);
    cannot_connect_request:
      Q::CannotConnect(CannotConnectRequest { token: 12, username: "bob".into() }) => p().u32(12).s("bob").frame(1001);
    cannot_connect_response: R::CannotConnect(CannotConnectResponse { token: 12, username: None }) => p().u32(12).frame(1001);
    cannot_connect_response_with_username:
      R::CannotConnect(CannotConnectResponse { token: 12, username: Some("bob".into()) }) => p().u32(12).s("bob").frame(1001);
    cannot_join_room_response: R::CannotJoinRoom(CannotJoinRoom { room: "club".into() }) => p().s("club").frame(1003);
  }

  decodes! {
    login_response_success_must_be_exactly_one: ServerResponse, p().u8(2).s("INVALIDPASS").frame(1)
      => Ok(R::Login(LoginResponse::Failure { reason: "INVALIDPASS".into(), detail: None }));
    get_peer_address_response_ignores_a_partial_trailer: ServerResponse, p().s("bob").ip(IP).u32(1).u32(1).frame(3)
      => Ok(R::GetPeerAddress(GetPeerAddressResponse { username: "bob".into(), ip: ip(), port: 1, obfuscation: None }));
    get_status_response_bool_accepts_any_non_zero: ServerResponse, p().s("bob").u32(2).u8(7).frame(7)
      => Ok(R::GetStatus(UserStatus { username: "bob".into(), presence: UserPresence::Online, is_privileged: true }));
    get_status_response_rejects_unknown_presence: ServerResponse, p().s("bob").u32(3).u8(0).frame(7)
      => Err(DecodeError::InvalidValue { what: "user presence", value: 3 });
    join_room_response_rejects_mismatched_lists: ServerResponse, p().s("r").u32(2).s("a").s("b").u32(1).u32(2).frame(14)
      => Err(DecodeError::ListLengthMismatch { what: "user statuses" });
    leave_room_response_ignores_trailing_fields: ServerResponse, p().s("room").raw(&[1, 2, 3]).frame(15)
      => Ok(R::LeaveRoom(LeaveRoom { room: "room".into() }));
    leave_room_response_rejects_a_string_past_the_payload: ServerResponse, p().u32(10).raw(b"abc").frame(15)
      => Err(DecodeError::Truncated { needed: 10, remaining: 3 });
    leave_room_response_rejects_a_cut_frame: ServerResponse, first(p().s("room").frame(15), 10)
      => Err(DecodeError::IncompleteFrame { needed: 16, available: 10 });
    connect_to_peer_response_rejects_unknown_type: ServerResponse,
      p().s("bob").s("X").ip(IP).u32(1).u32(1).u8(0).frame(18)
      => Err(DecodeError::UnknownConnectionType);
    private_message_response_is_new_must_be_exactly_one: ServerResponse, p().u32(1).u32(2).s("bob").s("m").u8(2).frame(22)
      => Ok(R::PrivateMessage(PrivateMessageResponse {
        id: 1, timestamp: 2, username: "bob".into(), message: "m".into(), is_new: false,
      }));
    ping_response_rejects_bytes_after_the_frame: ServerResponse, [p().frame(32), vec![0]].concat()
      => Err(DecodeError::TrailingBytes { extra: 1 });
    get_personal_recommendations_response_without_second_block: ServerResponse, p().u32(1).s("jazz").i32(-3).frame(54)
      => Ok(R::GetPersonalRecommendations(Recommendations {
        recommendations: vec![Recommendation { interest: "jazz".into(), count: -3 }],
        unrecommendations: vec![],
      }));
    room_list_response_rejects_mismatched_lists: ServerResponse, p().u32(1).s("a").u32(2).u32(1).u32(1).frame(64)
      => Err(DecodeError::ListLengthMismatch { what: "public rooms" });
    room_list_response_rejects_a_count_past_the_payload: ServerResponse, p().u32(u32::MAX).frame(64)
      => Err(DecodeError::CountTooLarge { what: "public rooms", count: u32::MAX, remaining: 0 });
    set_listen_port_response_is_not_a_server_message: ServerResponse, p().u32(1).frame(2)
      => Err(DecodeError::UnknownCode { code: 2 });
    unknown_code_response: ServerResponse, p().u32(1).frame(9999) => Err(DecodeError::UnknownCode { code: 9999 });
  }
}
