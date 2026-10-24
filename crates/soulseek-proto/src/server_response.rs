//! Messages the server sends to the client (spec 01 section 5, 01a section 2), pushed or in reply
//! to a [`crate::ServerRequest`] with the same code.

use std::net::Ipv4Addr;

use crate::codes::ServerCode;
use crate::distributed::Embedded;
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::Limits;
use crate::log::{self, KeyFields};
use crate::model::{
  PeerConnectionType, PrivateRoomData, Recommendation, Recommendations, RoomData, RoomInfo,
  RoomList, RoomTicker, UserData, UserInterests, UserPresence, UserStatistics, UserStats,
  UserStatus,
};
use crate::wire::{Reader, RemoteFilename, Secret, Writer};

/// five `u32`: speed, upload count, an unknown field that is always 0, files, directories
const USER_STATS_BYTES: usize = 20;

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

/// A `WatchUser` reply for a user that exists
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct WatchedUser {
  pub status: UserPresence,
  pub stats: UserStats,
  /// present only when the user is not offline, and may be blank
  pub country_code: Option<String>,
}

/// One `NetInfo` entry
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ParentCandidate {
  pub username: String,
  pub ip: Ipv4Addr,
  pub port: u32,
}

/// One `GetPersonalSimilarUsers` entry
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct SimilarUser {
  pub username: String,
  pub rating: u32,
}

/// One `RelatedSearch` entry
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct RelatedTerm {
  pub term: String,
  pub score: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum ServerResponse {
  Login(LoginResponse),
  GetPeerAddress {
    username: String,
    /// `0.0.0.0` when the user is offline
    ip: Ipv4Addr,
    port: u32,
    /// `(obfuscation_type, obfuscated_port)`, absent from older servers; never dialled
    obfuscation: Option<(u32, u16)>,
  },
  WatchUser {
    username: String,
    /// `None` when the user does not exist
    user: Option<WatchedUser>,
  },
  GetStatus(UserStatus),
  /// obsolete
  IgnoreUser {
    username: String,
  },
  /// obsolete
  UnignoreUser {
    username: String,
  },
  SayInChatRoom {
    room: String,
    username: String,
    message: String,
  },
  JoinRoom(RoomData),
  LeaveRoom {
    room: String,
  },
  UserJoinedRoom {
    room: String,
    user: UserData,
  },
  UserLeftRoom {
    room: String,
    username: String,
  },
  ConnectToPeer {
    username: String,
    connection_type: PeerConnectionType,
    ip: Ipv4Addr,
    port: u32,
    token: u32,
    privileged: bool,
    /// `(obfuscation_type, obfuscated_port)`; never dialled
    obfuscation: Option<(u32, u32)>,
  },
  PrivateMessage {
    /// acknowledge with `AcknowledgePrivateMessage`
    id: u32,
    /// unix seconds, UTC
    timestamp: u32,
    username: String,
    message: String,
    /// false when replayed from the server's offline store
    is_new: bool,
  },
  /// a search for us to answer
  FileSearch {
    username: String,
    token: u32,
    query: String,
  },
  /// the live server never sends this; an unsolicited one is dropped
  Ping,
  /// obsolete
  SendConnectToken {
    username: String,
    token: u32,
  },
  GetUserStats(UserStatistics),
  /// obsolete `UploadSlotsFull`
  QueuedDownloads {
    username: String,
    slots_full: bool,
  },
  /// another session logged in with our credentials
  KickedFromServer,
  /// obsolete
  SimilarRecommendations {
    recommendation: String,
    similar: Vec<String>,
  },
  GetPersonalRecommendations(Recommendations),
  /// obsolete
  MyRecommendations {
    likes: Vec<String>,
  },
  GetGlobalRecommendations(Recommendations),
  GetUserInterests(UserInterests),
  /// obsolete
  PlaceInLineRequest {
    username: String,
    token: u32,
  },
  /// obsolete
  PlaceInLineResponse {
    username: String,
    token: u32,
    place: u32,
  },
  /// obsolete
  RoomAdded {
    room: String,
  },
  /// obsolete
  RoomRemoved {
    room: String,
  },
  RoomList(RoomList),
  /// obsolete
  ExactFileSearch {
    username: String,
    token: u32,
    filename: RemoteFilename,
    path: RemoteFilename,
    file_size: u64,
    checksum: u32,
  },
  GlobalAdminMessage {
    message: String,
  },
  /// obsolete
  GlobalUserList {
    users: Vec<UserData>,
  },
  /// obsolete
  TunneledMessage {
    username: String,
    code: u32,
    token: u32,
    ip: Ipv4Addr,
    port: u32,
    message: String,
  },
  PrivilegedUsers {
    usernames: Vec<String>,
  },
  ParentMinSpeed {
    speed: u32,
  },
  ParentSpeedRatio {
    ratio: u32,
  },
  /// obsolete
  ParentInactivityTimeout {
    seconds: u32,
  },
  /// obsolete
  SearchInactivityTimeout {
    seconds: u32,
  },
  /// obsolete
  MinimumParentsInCache {
    count: u32,
  },
  /// obsolete
  DistributedAliveInterval {
    seconds: u32,
  },
  /// obsolete, the parser is kept
  AddPrivilegedUser {
    username: String,
  },
  CheckPrivileges {
    seconds_remaining: u32,
  },
  /// a distributed message the server wraps for a branch root
  EmbeddedMessage(Embedded),
  NetInfo {
    parents: Vec<ParentCandidate>,
  },
  WishlistInterval {
    seconds: u32,
  },
  GetPersonalSimilarUsers {
    users: Vec<SimilarUser>,
  },
  GetInterestRecommendations {
    interest: String,
    recommendations: Recommendations,
  },
  GetInterestSimilarUsers {
    interest: String,
    usernames: Vec<String>,
  },
  RoomTickers {
    room: String,
    tickers: Vec<RoomTicker>,
  },
  RoomTickerAdd {
    room: String,
    ticker: RoomTicker,
  },
  RoomTickerRemove {
    room: String,
    username: String,
  },
  /// deprecated
  UserPrivileges {
    username: String,
    privileged: bool,
  },
  /// deprecated, acknowledge with `AcknowledgeNotifyPrivileges`
  NotifyPrivileges {
    id: u32,
    username: String,
  },
  DistributedReset,
  /// members of a private room, excluding the owner
  PrivateRoomUsers {
    room: String,
    usernames: Vec<String>,
  },
  PrivateRoomAddUser {
    room: String,
    username: String,
  },
  PrivateRoomRemoveUser {
    room: String,
    username: String,
  },
  /// obsolete
  PrivateRoomUnknown {
    room: String,
  },
  PrivateRoomAdded {
    room: String,
  },
  PrivateRoomRemoved {
    room: String,
  },
  PrivateRoomToggle {
    accept_invitations: bool,
  },
  NewPassword {
    password: Secret,
  },
  PrivateRoomAddOperator {
    room: String,
    username: String,
  },
  PrivateRoomRemoveOperator {
    room: String,
    username: String,
  },
  PrivateRoomOperatorAdded {
    room: String,
  },
  PrivateRoomOperatorRemoved {
    room: String,
  },
  /// the operators of a private room we are in
  PrivateRoomOwned {
    room: String,
    operators: Vec<String>,
  },
  PublicChat {
    room: String,
    username: String,
    message: String,
  },
  /// obsolete
  RelatedSearch {
    query: String,
    terms: Vec<RelatedTerm>,
  },
  ExcludedSearchPhrases {
    phrases: Vec<String>,
  },
  CannotConnect {
    token: u32,
    /// the live server sends only the token
    username: Option<String>,
  },
  /// a `JoinRoom` would have created a room that clashes with an existing private room
  CannotJoinRoom {
    room: String,
  },
}

fn read_user_stats(r: &mut Reader) -> Result<UserStats, DecodeError> {
  let average_speed = r.u32()?;
  let upload_count = r.u32()?;
  // always 0 on the live server
  r.u32()?;
  Ok(UserStats {
    average_speed,
    upload_count,
    file_count: r.u32()?,
    directory_count: r.u32()?,
  })
}

fn write_user_stats(w: &mut Writer, stats: &UserStats) {
  w.u32(stats.average_speed);
  w.u32(stats.upload_count);
  w.u32(0);
  w.u32(stats.file_count);
  w.u32(stats.directory_count);
}

fn read_parallel_count(
  r: &mut Reader,
  min_size: usize,
  expected: usize,
  what: &'static str,
) -> Result<usize, DecodeError> {
  let count = r.count(min_size, what)?;
  if count != expected {
    return Err(DecodeError::ListLengthMismatch { what });
  }
  Ok(count)
}

/// The five parallel per-user lists of `JoinRoom` and `GlobalUserList`, which must agree in
/// length
fn read_user_lists(r: &mut Reader) -> Result<Vec<UserData>, DecodeError> {
  let usernames = r.strings("room users")?;
  let n = usernames.len();
  read_parallel_count(r, 4, n, "user statuses")?;
  let statuses = (0..n)
    .map(|_| UserPresence::from_u32(r.u32()?))
    .collect::<Result<Vec<_>, _>>()?;
  read_parallel_count(r, USER_STATS_BYTES, n, "user stats")?;
  let stats = (0..n)
    .map(|_| read_user_stats(r))
    .collect::<Result<Vec<_>, _>>()?;
  read_parallel_count(r, 4, n, "user slots")?;
  let slots = (0..n)
    .map(|_| Ok(r.u32()? != 0))
    .collect::<Result<Vec<_>, DecodeError>>()?;
  read_parallel_count(r, 4, n, "user countries")?;
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
    w.u32(user.status as u32);
  }
  w.count(users.len());
  for user in users {
    write_user_stats(w, &user.stats);
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

fn read_recommendation_block(r: &mut Reader) -> Result<Vec<Recommendation>, DecodeError> {
  let count = r.count(8, "recommendations")?;
  (0..count)
    .map(|_| {
      Ok(Recommendation {
        interest: r.string()?,
        count: r.i32()?,
      })
    })
    .collect()
}

/// the second block is read only when bytes remain
fn read_recommendations(r: &mut Reader) -> Result<Recommendations, DecodeError> {
  let recommendations = read_recommendation_block(r)?;
  let unrecommendations = if r.has_more() {
    read_recommendation_block(r)?
  } else {
    Vec::new()
  };
  Ok(Recommendations {
    recommendations,
    unrecommendations,
  })
}

fn write_recommendations(w: &mut Writer, recommendations: &Recommendations) {
  for block in [
    &recommendations.recommendations,
    &recommendations.unrecommendations,
  ] {
    w.count(block.len());
    for recommendation in block {
      w.string(&recommendation.interest);
      w.i32(recommendation.count);
    }
  }
}

/// the names and counts of one `RoomList` block, which must agree in length
fn read_room_block(r: &mut Reader, what: &'static str) -> Result<Vec<RoomInfo>, DecodeError> {
  let names = r.strings(what)?;
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

impl ServerResponse {
  pub fn code(&self) -> ServerCode {
    match self {
      Self::Login(_) => ServerCode::Login,
      Self::GetPeerAddress { .. } => ServerCode::GetPeerAddress,
      Self::WatchUser { .. } => ServerCode::WatchUser,
      Self::GetStatus(_) => ServerCode::GetStatus,
      Self::IgnoreUser { .. } => ServerCode::IgnoreUser,
      Self::UnignoreUser { .. } => ServerCode::UnignoreUser,
      Self::SayInChatRoom { .. } => ServerCode::SayInChatRoom,
      Self::JoinRoom(_) => ServerCode::JoinRoom,
      Self::LeaveRoom { .. } => ServerCode::LeaveRoom,
      Self::UserJoinedRoom { .. } => ServerCode::UserJoinedRoom,
      Self::UserLeftRoom { .. } => ServerCode::UserLeftRoom,
      Self::ConnectToPeer { .. } => ServerCode::ConnectToPeer,
      Self::PrivateMessage { .. } => ServerCode::PrivateMessage,
      Self::FileSearch { .. } => ServerCode::FileSearch,
      Self::Ping => ServerCode::Ping,
      Self::SendConnectToken { .. } => ServerCode::SendConnectToken,
      Self::GetUserStats(_) => ServerCode::GetUserStats,
      Self::QueuedDownloads { .. } => ServerCode::QueuedDownloads,
      Self::KickedFromServer => ServerCode::KickedFromServer,
      Self::SimilarRecommendations { .. } => ServerCode::SimilarRecommendations,
      Self::GetPersonalRecommendations(_) => ServerCode::GetPersonalRecommendations,
      Self::MyRecommendations { .. } => ServerCode::MyRecommendations,
      Self::GetGlobalRecommendations(_) => ServerCode::GetGlobalRecommendations,
      Self::GetUserInterests(_) => ServerCode::GetUserInterests,
      Self::PlaceInLineRequest { .. } => ServerCode::PlaceInLineRequest,
      Self::PlaceInLineResponse { .. } => ServerCode::PlaceInLineResponse,
      Self::RoomAdded { .. } => ServerCode::RoomAdded,
      Self::RoomRemoved { .. } => ServerCode::RoomRemoved,
      Self::RoomList(_) => ServerCode::RoomList,
      Self::ExactFileSearch { .. } => ServerCode::ExactFileSearch,
      Self::GlobalAdminMessage { .. } => ServerCode::GlobalAdminMessage,
      Self::GlobalUserList { .. } => ServerCode::GlobalUserList,
      Self::TunneledMessage { .. } => ServerCode::TunneledMessage,
      Self::PrivilegedUsers { .. } => ServerCode::PrivilegedUsers,
      Self::ParentMinSpeed { .. } => ServerCode::ParentMinSpeed,
      Self::ParentSpeedRatio { .. } => ServerCode::ParentSpeedRatio,
      Self::ParentInactivityTimeout { .. } => ServerCode::ParentInactivityTimeout,
      Self::SearchInactivityTimeout { .. } => ServerCode::SearchInactivityTimeout,
      Self::MinimumParentsInCache { .. } => ServerCode::MinimumParentsInCache,
      Self::DistributedAliveInterval { .. } => ServerCode::DistributedAliveInterval,
      Self::AddPrivilegedUser { .. } => ServerCode::AddPrivilegedUser,
      Self::CheckPrivileges { .. } => ServerCode::CheckPrivileges,
      Self::EmbeddedMessage(_) => ServerCode::EmbeddedMessage,
      Self::NetInfo { .. } => ServerCode::NetInfo,
      Self::WishlistInterval { .. } => ServerCode::WishlistInterval,
      Self::GetPersonalSimilarUsers { .. } => ServerCode::GetPersonalSimilarUsers,
      Self::GetInterestRecommendations { .. } => ServerCode::GetInterestRecommendations,
      Self::GetInterestSimilarUsers { .. } => ServerCode::GetInterestSimilarUsers,
      Self::RoomTickers { .. } => ServerCode::RoomTickers,
      Self::RoomTickerAdd { .. } => ServerCode::RoomTickerAdd,
      Self::RoomTickerRemove { .. } => ServerCode::RoomTickerRemove,
      Self::UserPrivileges { .. } => ServerCode::UserPrivileges,
      Self::NotifyPrivileges { .. } => ServerCode::NotifyPrivileges,
      Self::DistributedReset => ServerCode::DistributedReset,
      Self::PrivateRoomUsers { .. } => ServerCode::PrivateRoomUsers,
      Self::PrivateRoomAddUser { .. } => ServerCode::PrivateRoomAddUser,
      Self::PrivateRoomRemoveUser { .. } => ServerCode::PrivateRoomRemoveUser,
      Self::PrivateRoomUnknown { .. } => ServerCode::PrivateRoomUnknown,
      Self::PrivateRoomAdded { .. } => ServerCode::PrivateRoomAdded,
      Self::PrivateRoomRemoved { .. } => ServerCode::PrivateRoomRemoved,
      Self::PrivateRoomToggle { .. } => ServerCode::PrivateRoomToggle,
      Self::NewPassword { .. } => ServerCode::NewPassword,
      Self::PrivateRoomAddOperator { .. } => ServerCode::PrivateRoomAddOperator,
      Self::PrivateRoomRemoveOperator { .. } => ServerCode::PrivateRoomRemoveOperator,
      Self::PrivateRoomOperatorAdded { .. } => ServerCode::PrivateRoomOperatorAdded,
      Self::PrivateRoomOperatorRemoved { .. } => ServerCode::PrivateRoomOperatorRemoved,
      Self::PrivateRoomOwned { .. } => ServerCode::PrivateRoomOwned,
      Self::PublicChat { .. } => ServerCode::PublicChat,
      Self::RelatedSearch { .. } => ServerCode::RelatedSearch,
      Self::ExcludedSearchPhrases { .. } => ServerCode::ExcludedSearchPhrases,
      Self::CannotConnect { .. } => ServerCode::CannotConnect,
      Self::CannotJoinRoom { .. } => ServerCode::CannotJoinRoom,
    }
  }

  /// the complete frame, length prefix included
  pub fn encode(&self) -> Vec<u8> {
    let mut w = Writer::frame(self.code().code());
    match self {
      Self::Login(LoginResponse::Success {
        greeting,
        ip,
        hash,
        is_supporter,
      }) => {
        w.bool(true);
        w.string(greeting);
        w.ip4(*ip);
        w.string(hash.expose());
        w.bool(*is_supporter);
      }
      Self::Login(LoginResponse::Failure { reason, detail }) => {
        w.bool(false);
        w.string(reason);
        if let Some(detail) = detail {
          w.string(detail);
        }
      }
      Self::GetPeerAddress {
        username,
        ip,
        port,
        obfuscation,
      } => {
        w.string(username);
        w.ip4(*ip);
        w.u32(*port);
        if let Some((obfuscation_type, obfuscated_port)) = obfuscation {
          w.u32(*obfuscation_type);
          w.u16(*obfuscated_port);
        }
      }
      Self::WatchUser { username, user } => {
        w.string(username);
        w.bool(user.is_some());
        if let Some(user) = user {
          w.u32(user.status as u32);
          write_user_stats(&mut w, &user.stats);
          if let Some(country_code) = &user.country_code {
            w.string(country_code);
          }
        }
      }
      Self::GetStatus(status) => {
        w.string(&status.username);
        w.u32(status.presence as u32);
        w.bool(status.is_privileged);
      }
      Self::IgnoreUser { username }
      | Self::UnignoreUser { username }
      | Self::AddPrivilegedUser { username } => w.string(username),
      Self::SayInChatRoom {
        room,
        username,
        message,
      }
      | Self::PublicChat {
        room,
        username,
        message,
      } => {
        w.string(room);
        w.string(username);
        w.string(message);
      }
      Self::JoinRoom(data) => {
        w.string(&data.name);
        write_user_lists(&mut w, &data.users);
        if let Some(private) = &data.private {
          w.string(&private.owner);
          w.strings(&private.operators);
        }
      }
      Self::LeaveRoom { room }
      | Self::RoomAdded { room }
      | Self::RoomRemoved { room }
      | Self::PrivateRoomUnknown { room }
      | Self::PrivateRoomAdded { room }
      | Self::PrivateRoomRemoved { room }
      | Self::PrivateRoomOperatorAdded { room }
      | Self::PrivateRoomOperatorRemoved { room }
      | Self::CannotJoinRoom { room } => w.string(room),
      Self::UserJoinedRoom { room, user } => {
        w.string(room);
        w.string(&user.username);
        w.u32(user.status as u32);
        write_user_stats(&mut w, &user.stats);
        w.u32(u32::from(user.slots_full));
        w.string(&user.country_code);
      }
      Self::UserLeftRoom { room, username }
      | Self::RoomTickerRemove { room, username }
      | Self::PrivateRoomAddUser { room, username }
      | Self::PrivateRoomRemoveUser { room, username }
      | Self::PrivateRoomAddOperator { room, username }
      | Self::PrivateRoomRemoveOperator { room, username } => {
        w.string(room);
        w.string(username);
      }
      Self::ConnectToPeer {
        username,
        connection_type,
        ip,
        port,
        token,
        privileged,
        obfuscation,
      } => {
        w.string(username);
        w.string(connection_type.as_str());
        w.ip4(*ip);
        w.u32(*port);
        w.u32(*token);
        w.bool(*privileged);
        if let Some((obfuscation_type, obfuscated_port)) = obfuscation {
          w.u32(*obfuscation_type);
          w.u32(*obfuscated_port);
        }
      }
      Self::PrivateMessage {
        id,
        timestamp,
        username,
        message,
        is_new,
      } => {
        w.u32(*id);
        w.u32(*timestamp);
        w.string(username);
        w.string(message);
        w.bool(*is_new);
      }
      Self::FileSearch {
        username,
        token,
        query,
      } => {
        w.string(username);
        w.u32(*token);
        w.string(query);
      }
      Self::Ping | Self::KickedFromServer | Self::DistributedReset => {}
      Self::SendConnectToken { username, token } | Self::PlaceInLineRequest { username, token } => {
        w.string(username);
        w.u32(*token);
      }
      Self::GetUserStats(statistics) => {
        w.string(&statistics.username);
        write_user_stats(&mut w, &statistics.stats);
      }
      Self::QueuedDownloads {
        username,
        slots_full,
      } => {
        w.string(username);
        w.u32(u32::from(*slots_full));
      }
      Self::SimilarRecommendations {
        recommendation,
        similar,
      } => {
        w.string(recommendation);
        w.strings(similar);
      }
      Self::GetPersonalRecommendations(recommendations)
      | Self::GetGlobalRecommendations(recommendations) => {
        write_recommendations(&mut w, recommendations)
      }
      Self::MyRecommendations { likes: names }
      | Self::PrivilegedUsers { usernames: names }
      | Self::ExcludedSearchPhrases { phrases: names } => w.strings(names),
      Self::GetUserInterests(interests) => {
        w.string(&interests.username);
        w.strings(&interests.likes);
        w.strings(&interests.hates);
      }
      Self::PlaceInLineResponse {
        username,
        token,
        place,
      } => {
        w.string(username);
        w.u32(*token);
        w.u32(*place);
      }
      Self::RoomList(list) => {
        write_room_block(&mut w, &list.public);
        write_room_block(&mut w, &list.owned);
        write_room_block(&mut w, &list.private);
        w.strings(&list.moderated_room_names);
      }
      Self::ExactFileSearch {
        username,
        token,
        filename,
        path,
        file_size,
        checksum,
      } => {
        w.string(username);
        w.u32(*token);
        w.filename(filename);
        w.filename(path);
        w.u64(*file_size);
        w.u32(*checksum);
      }
      Self::GlobalAdminMessage { message } => w.string(message),
      Self::GlobalUserList { users } => write_user_lists(&mut w, users),
      Self::TunneledMessage {
        username,
        code,
        token,
        ip,
        port,
        message,
      } => {
        w.string(username);
        w.u32(*code);
        w.u32(*token);
        w.ip4(*ip);
        w.u32(*port);
        w.string(message);
      }
      Self::ParentMinSpeed { speed: value }
      | Self::ParentSpeedRatio { ratio: value }
      | Self::ParentInactivityTimeout { seconds: value }
      | Self::SearchInactivityTimeout { seconds: value }
      | Self::MinimumParentsInCache { count: value }
      | Self::DistributedAliveInterval { seconds: value }
      | Self::CheckPrivileges {
        seconds_remaining: value,
      }
      | Self::WishlistInterval { seconds: value } => w.u32(*value),
      Self::EmbeddedMessage(embedded) => {
        w.u8(embedded.distributed_code);
        w.bytes(&embedded.payload);
      }
      Self::NetInfo { parents } => {
        w.count(parents.len());
        for parent in parents {
          w.string(&parent.username);
          w.ip4(parent.ip);
          w.u32(parent.port);
        }
      }
      Self::GetPersonalSimilarUsers { users } => {
        w.count(users.len());
        for user in users {
          w.string(&user.username);
          w.u32(user.rating);
        }
      }
      Self::GetInterestRecommendations {
        interest,
        recommendations,
      } => {
        w.string(interest);
        write_recommendations(&mut w, recommendations);
      }
      Self::GetInterestSimilarUsers {
        interest: name,
        usernames,
      }
      | Self::PrivateRoomUsers {
        room: name,
        usernames,
      }
      | Self::PrivateRoomOwned {
        room: name,
        operators: usernames,
      } => {
        w.string(name);
        w.strings(usernames);
      }
      Self::RoomTickers { room, tickers } => {
        w.string(room);
        w.count(tickers.len());
        for ticker in tickers {
          w.string(&ticker.username);
          w.string(&ticker.message);
        }
      }
      Self::RoomTickerAdd { room, ticker } => {
        w.string(room);
        w.string(&ticker.username);
        w.string(&ticker.message);
      }
      Self::UserPrivileges {
        username,
        privileged,
      } => {
        w.string(username);
        w.bool(*privileged);
      }
      Self::NotifyPrivileges { id, username } => {
        w.u32(*id);
        w.string(username);
      }
      Self::PrivateRoomToggle { accept_invitations } => w.bool(*accept_invitations),
      Self::NewPassword { password } => w.string(password.expose()),
      Self::RelatedSearch { query, terms } => {
        w.string(query);
        w.count(terms.len());
        for term in terms {
          w.string(&term.term);
          w.u32(term.score);
        }
      }
      Self::CannotConnect { token, username } => {
        w.u32(*token);
        if let Some(username) = username {
          w.string(username);
        }
      }
    }
    w.finish()
  }

  /// decodes one complete frame, length prefix included
  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    let frame = frame::exact(FrameKind::Server, bytes, &Limits::DEFAULT)?;
    let code = ServerCode::from_code(frame.code);
    let message = match code {
      Some(code) => Self::read(code, frame.payload),
      None => Err(DecodeError::UnknownCode { code: frame.code }),
    }
    .inspect_err(|error| {
      log::rejected(
        "server response",
        frame.code,
        code.map(ServerCode::name),
        error,
      )
    })?;
    // embedded messages arrive for every network search while we are a branch root
    if !matches!(message, Self::EmbeddedMessage(_)) {
      log::decoded(
        "server response",
        frame.code,
        message.code().name(),
        message.key_fields(),
      );
    }
    Ok(message)
  }

  fn read(code: ServerCode, payload: &[u8]) -> Result<Self, DecodeError> {
    let r = &mut Reader::new(payload);
    Ok(match code {
      ServerCode::Login => Self::Login(if r.bool_exact()? {
        LoginResponse::Success {
          greeting: r.description()?,
          ip: r.ip4()?,
          hash: Secret::new(r.string()?),
          is_supporter: r.bool_exact()?,
        }
      } else {
        LoginResponse::Failure {
          reason: r.string()?,
          detail: if r.has_more() {
            Some(r.string()?)
          } else {
            None
          },
        }
      }),
      ServerCode::GetPeerAddress => Self::GetPeerAddress {
        username: r.string()?,
        ip: r.ip4()?,
        port: r.u32()?,
        obfuscation: if r.remaining() >= 6 {
          Some((r.u32()?, r.u16()?))
        } else {
          None
        },
      },
      ServerCode::WatchUser => {
        let username = r.string()?;
        let user = if r.bool()? {
          Some(WatchedUser {
            status: UserPresence::from_u32(r.u32()?)?,
            stats: read_user_stats(r)?,
            country_code: if r.has_more() {
              Some(r.string()?)
            } else {
              None
            },
          })
        } else {
          None
        };
        Self::WatchUser { username, user }
      }
      ServerCode::GetStatus => Self::GetStatus(UserStatus {
        username: r.string()?,
        presence: UserPresence::from_u32(r.u32()?)?,
        is_privileged: r.bool()?,
      }),
      ServerCode::IgnoreUser => Self::IgnoreUser {
        username: r.string()?,
      },
      ServerCode::UnignoreUser => Self::UnignoreUser {
        username: r.string()?,
      },
      ServerCode::SayInChatRoom => Self::SayInChatRoom {
        room: r.string()?,
        username: r.string()?,
        message: r.string()?,
      },
      ServerCode::JoinRoom => {
        let name = r.string()?;
        let users = read_user_lists(r)?;
        // only private rooms carry the owner and operators
        let private = if r.has_more() {
          Some(PrivateRoomData {
            owner: r.string()?,
            operators: r.strings("room operators")?,
          })
        } else {
          None
        };
        Self::JoinRoom(RoomData {
          name,
          users,
          private,
        })
      }
      ServerCode::LeaveRoom => Self::LeaveRoom { room: r.string()? },
      ServerCode::UserJoinedRoom => Self::UserJoinedRoom {
        room: r.string()?,
        user: UserData {
          username: r.string()?,
          status: UserPresence::from_u32(r.u32()?)?,
          stats: read_user_stats(r)?,
          slots_full: r.u32()? != 0,
          country_code: r.string()?,
        },
      },
      ServerCode::UserLeftRoom => Self::UserLeftRoom {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::ConnectToPeer => Self::ConnectToPeer {
        username: r.string()?,
        connection_type: PeerConnectionType::parse(&r.string()?)?,
        ip: r.ip4()?,
        port: r.u32()?,
        token: r.u32()?,
        privileged: r.bool()?,
        obfuscation: if r.remaining() >= 8 {
          Some((r.u32()?, r.u32()?))
        } else {
          None
        },
      },
      ServerCode::PrivateMessage => Self::PrivateMessage {
        id: r.u32()?,
        timestamp: r.u32()?,
        username: r.string()?,
        message: r.string()?,
        is_new: r.bool_exact()?,
      },
      ServerCode::FileSearch => Self::FileSearch {
        username: r.string()?,
        token: r.u32()?,
        query: r.string()?,
      },
      ServerCode::Ping => Self::Ping,
      ServerCode::SendConnectToken => Self::SendConnectToken {
        username: r.string()?,
        token: r.u32()?,
      },
      ServerCode::GetUserStats => Self::GetUserStats(UserStatistics {
        username: r.string()?,
        stats: read_user_stats(r)?,
      }),
      ServerCode::QueuedDownloads => Self::QueuedDownloads {
        username: r.string()?,
        slots_full: r.u32()? != 0,
      },
      ServerCode::KickedFromServer => Self::KickedFromServer,
      ServerCode::SimilarRecommendations => Self::SimilarRecommendations {
        recommendation: r.string()?,
        similar: r.strings("similar recommendations")?,
      },
      ServerCode::GetPersonalRecommendations => {
        Self::GetPersonalRecommendations(read_recommendations(r)?)
      }
      ServerCode::MyRecommendations => Self::MyRecommendations {
        likes: r.strings("likes")?,
      },
      ServerCode::GetGlobalRecommendations => {
        Self::GetGlobalRecommendations(read_recommendations(r)?)
      }
      ServerCode::GetUserInterests => Self::GetUserInterests(UserInterests {
        username: r.string()?,
        likes: r.strings("likes")?,
        hates: r.strings("hates")?,
      }),
      ServerCode::PlaceInLineRequest => Self::PlaceInLineRequest {
        username: r.string()?,
        token: r.u32()?,
      },
      ServerCode::PlaceInLineResponse => Self::PlaceInLineResponse {
        username: r.string()?,
        token: r.u32()?,
        place: r.u32()?,
      },
      ServerCode::RoomAdded => Self::RoomAdded { room: r.string()? },
      ServerCode::RoomRemoved => Self::RoomRemoved { room: r.string()? },
      ServerCode::RoomList => Self::RoomList(RoomList {
        public: read_room_block(r, "public rooms")?,
        owned: read_room_block(r, "owned rooms")?,
        private: read_room_block(r, "private rooms")?,
        moderated_room_names: r.strings("operated rooms")?,
      }),
      ServerCode::ExactFileSearch => Self::ExactFileSearch {
        username: r.string()?,
        token: r.u32()?,
        filename: r.filename()?,
        path: r.filename()?,
        file_size: r.u64()?,
        checksum: r.u32()?,
      },
      ServerCode::GlobalAdminMessage => Self::GlobalAdminMessage {
        message: r.string()?,
      },
      ServerCode::GlobalUserList => Self::GlobalUserList {
        users: read_user_lists(r)?,
      },
      ServerCode::TunneledMessage => Self::TunneledMessage {
        username: r.string()?,
        code: r.u32()?,
        token: r.u32()?,
        ip: r.ip4()?,
        port: r.u32()?,
        message: r.string()?,
      },
      ServerCode::PrivilegedUsers => Self::PrivilegedUsers {
        usernames: r.strings("privileged users")?,
      },
      ServerCode::ParentMinSpeed => Self::ParentMinSpeed { speed: r.u32()? },
      ServerCode::ParentSpeedRatio => Self::ParentSpeedRatio { ratio: r.u32()? },
      ServerCode::ParentInactivityTimeout => Self::ParentInactivityTimeout { seconds: r.u32()? },
      ServerCode::SearchInactivityTimeout => Self::SearchInactivityTimeout { seconds: r.u32()? },
      ServerCode::MinimumParentsInCache => Self::MinimumParentsInCache { count: r.u32()? },
      ServerCode::DistributedAliveInterval => Self::DistributedAliveInterval { seconds: r.u32()? },
      ServerCode::AddPrivilegedUser => Self::AddPrivilegedUser {
        username: r.string()?,
      },
      ServerCode::CheckPrivileges => Self::CheckPrivileges {
        seconds_remaining: r.u32()?,
      },
      ServerCode::EmbeddedMessage => Self::EmbeddedMessage(Embedded {
        distributed_code: r.u8()?,
        payload: r.rest().to_vec(),
      }),
      ServerCode::NetInfo => {
        let count = r.count(12, "parent candidates")?;
        Self::NetInfo {
          parents: (0..count)
            .map(|_| {
              Ok(ParentCandidate {
                username: r.string()?,
                ip: r.ip4()?,
                port: r.u32()?,
              })
            })
            .collect::<Result<_, DecodeError>>()?,
        }
      }
      ServerCode::WishlistInterval => Self::WishlistInterval { seconds: r.u32()? },
      ServerCode::GetPersonalSimilarUsers => {
        let count = r.count(8, "similar users")?;
        Self::GetPersonalSimilarUsers {
          users: (0..count)
            .map(|_| {
              Ok(SimilarUser {
                username: r.string()?,
                rating: r.u32()?,
              })
            })
            .collect::<Result<_, DecodeError>>()?,
        }
      }
      ServerCode::GetInterestRecommendations => Self::GetInterestRecommendations {
        interest: r.string()?,
        recommendations: read_recommendations(r)?,
      },
      ServerCode::GetInterestSimilarUsers => Self::GetInterestSimilarUsers {
        interest: r.string()?,
        usernames: r.strings("similar users")?,
      },
      ServerCode::RoomTickers => {
        let room = r.string()?;
        let count = r.count(8, "room tickers")?;
        Self::RoomTickers {
          room,
          tickers: (0..count)
            .map(|_| {
              Ok(RoomTicker {
                username: r.string()?,
                message: r.string()?,
              })
            })
            .collect::<Result<_, DecodeError>>()?,
        }
      }
      ServerCode::RoomTickerAdd => Self::RoomTickerAdd {
        room: r.string()?,
        ticker: RoomTicker {
          username: r.string()?,
          message: r.string()?,
        },
      },
      ServerCode::RoomTickerRemove => Self::RoomTickerRemove {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::UserPrivileges => Self::UserPrivileges {
        username: r.string()?,
        privileged: r.bool()?,
      },
      ServerCode::NotifyPrivileges => Self::NotifyPrivileges {
        id: r.u32()?,
        username: r.string()?,
      },
      ServerCode::DistributedReset => Self::DistributedReset,
      ServerCode::PrivateRoomUsers => Self::PrivateRoomUsers {
        room: r.string()?,
        usernames: r.strings("room members")?,
      },
      ServerCode::PrivateRoomAddUser => Self::PrivateRoomAddUser {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::PrivateRoomRemoveUser => Self::PrivateRoomRemoveUser {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::PrivateRoomUnknown => Self::PrivateRoomUnknown { room: r.string()? },
      ServerCode::PrivateRoomAdded => Self::PrivateRoomAdded { room: r.string()? },
      ServerCode::PrivateRoomRemoved => Self::PrivateRoomRemoved { room: r.string()? },
      ServerCode::PrivateRoomToggle => Self::PrivateRoomToggle {
        accept_invitations: r.bool()?,
      },
      ServerCode::NewPassword => Self::NewPassword {
        password: Secret::new(r.string()?),
      },
      ServerCode::PrivateRoomAddOperator => Self::PrivateRoomAddOperator {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::PrivateRoomRemoveOperator => Self::PrivateRoomRemoveOperator {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::PrivateRoomOperatorAdded => Self::PrivateRoomOperatorAdded { room: r.string()? },
      ServerCode::PrivateRoomOperatorRemoved => {
        Self::PrivateRoomOperatorRemoved { room: r.string()? }
      }
      ServerCode::PrivateRoomOwned => Self::PrivateRoomOwned {
        room: r.string()?,
        operators: r.strings("room operators")?,
      },
      ServerCode::PublicChat => Self::PublicChat {
        room: r.string()?,
        username: r.string()?,
        message: r.string()?,
      },
      ServerCode::RelatedSearch => {
        let query = r.string()?;
        let count = r.count(8, "related terms")?;
        Self::RelatedSearch {
          query,
          terms: (0..count)
            .map(|_| {
              Ok(RelatedTerm {
                term: r.string()?,
                score: r.u32()?,
              })
            })
            .collect::<Result<_, DecodeError>>()?,
        }
      }
      ServerCode::ExcludedSearchPhrases => Self::ExcludedSearchPhrases {
        phrases: r.strings("excluded phrases")?,
      },
      ServerCode::CannotConnect => Self::CannotConnect {
        token: r.u32()?,
        username: if r.has_more() {
          Some(r.string()?)
        } else {
          None
        },
      },
      ServerCode::CannotJoinRoom => Self::CannotJoinRoom { room: r.string()? },
      // codes a client sends but the server never does
      ServerCode::SetListenPort
      | ServerCode::UnwatchUser
      | ServerCode::AcknowledgePrivateMessage
      | ServerCode::FileSearchRoom
      | ServerCode::SetOnlineStatus
      | ServerCode::SendSpeed
      | ServerCode::SharedFoldersAndFiles
      | ServerCode::UserSearch
      | ServerCode::LikedInterestAdd
      | ServerCode::LikedInterestRemove
      | ServerCode::AdminCommand
      | ServerCode::HaveNoParents
      | ServerCode::ParentsIP
      | ServerCode::AcceptChildren
      | ServerCode::WishlistSearch
      | ServerCode::SetRoomTicker
      | ServerCode::HatedInterestAdd
      | ServerCode::HatedInterestRemove
      | ServerCode::RoomSearch
      | ServerCode::SendUploadSpeed
      | ServerCode::GivePrivileges
      | ServerCode::AcknowledgeNotifyPrivileges
      | ServerCode::BranchLevel
      | ServerCode::BranchRoot
      | ServerCode::ChildDepth
      | ServerCode::PrivateRoomDropMembership
      | ServerCode::PrivateRoomDropOwnership
      | ServerCode::MessageUsers
      | ServerCode::AskPublicChat
      | ServerCode::StopPublicChat => return Err(DecodeError::UnknownCode { code: code.code() }),
    })
  }

  fn key_fields(&self) -> KeyFields<'_> {
    match self {
      Self::GetPeerAddress { username, .. }
      | Self::WatchUser { username, .. }
      | Self::GetStatus(UserStatus { username, .. })
      | Self::IgnoreUser { username }
      | Self::UnignoreUser { username }
      | Self::GetUserStats(UserStatistics { username, .. })
      | Self::QueuedDownloads { username, .. }
      | Self::GetUserInterests(UserInterests { username, .. })
      | Self::AddPrivilegedUser { username }
      | Self::UserPrivileges { username, .. } => KeyFields::username(username),
      Self::SayInChatRoom { room, username, .. }
      | Self::UserJoinedRoom {
        room,
        user: UserData { username, .. },
      }
      | Self::UserLeftRoom { room, username }
      | Self::RoomTickerAdd {
        room,
        ticker: RoomTicker { username, .. },
      }
      | Self::RoomTickerRemove { room, username }
      | Self::PrivateRoomAddUser { room, username }
      | Self::PrivateRoomRemoveUser { room, username }
      | Self::PrivateRoomAddOperator { room, username }
      | Self::PrivateRoomRemoveOperator { room, username }
      | Self::PublicChat { room, username, .. } => KeyFields::room(room).with_username(username),
      Self::JoinRoom(data) => KeyFields::room(&data.name).with_count(data.users.len()),
      Self::LeaveRoom { room }
      | Self::RoomAdded { room }
      | Self::RoomRemoved { room }
      | Self::PrivateRoomUnknown { room }
      | Self::PrivateRoomAdded { room }
      | Self::PrivateRoomRemoved { room }
      | Self::PrivateRoomOperatorAdded { room }
      | Self::PrivateRoomOperatorRemoved { room }
      | Self::CannotJoinRoom { room } => KeyFields::room(room),
      Self::RoomTickers { room, tickers } => KeyFields::room(room).with_count(tickers.len()),
      Self::PrivateRoomUsers { room, usernames }
      | Self::PrivateRoomOwned {
        room,
        operators: usernames,
      } => KeyFields::room(room).with_count(usernames.len()),
      Self::ConnectToPeer {
        username, token, ..
      }
      | Self::FileSearch {
        username, token, ..
      }
      | Self::SendConnectToken { username, token }
      | Self::PlaceInLineRequest { username, token }
      | Self::PlaceInLineResponse {
        username, token, ..
      }
      | Self::ExactFileSearch {
        username, token, ..
      }
      | Self::TunneledMessage {
        username, token, ..
      } => KeyFields::username(username).with_token(*token),
      Self::PrivateMessage { id, username, .. } | Self::NotifyPrivileges { id, username } => {
        KeyFields::username(username).with_token(*id)
      }
      Self::CannotConnect { token, username } => KeyFields {
        username: username.as_deref(),
        ..KeyFields::token(*token)
      },
      Self::RoomList(list) => KeyFields::count(list.public.len()),
      Self::GlobalUserList { users } => KeyFields::count(users.len()),
      Self::NetInfo { parents } => KeyFields::count(parents.len()),
      Self::PrivilegedUsers { usernames } => KeyFields::count(usernames.len()),
      Self::ExcludedSearchPhrases { phrases } => KeyFields::count(phrases.len()),
      _ => KeyFields::default(),
    }
  }
}
