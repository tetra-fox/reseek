//! Messages the client sends to the server (spec 01 section 5, 01a section 2). The server's
//! replies, which often share a code but not a layout, are in [`crate::server_response`].

use std::net::Ipv4Addr;

use md5::{Digest, Md5};

use crate::codes::ServerCode;
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::Limits;
use crate::log::{self, KeyFields};
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

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum ServerRequest {
  Login {
    username: String,
    password: Secret,
    /// [`login_hash`] of the username and password
    hash: Secret,
    major_version: u32,
    minor_version: u32,
  },
  SetListenPort {
    port: u32,
    /// `(obfuscation_type, obfuscated_port)`; never sent by this client
    obfuscation: Option<(u32, u32)>,
  },
  GetPeerAddress {
    username: String,
  },
  WatchUser {
    username: String,
  },
  UnwatchUser {
    username: String,
  },
  GetStatus {
    username: String,
  },
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
    message: String,
  },
  JoinRoom {
    room: String,
    /// the type of room to create if it does not exist
    room_type: RoomType,
  },
  LeaveRoom {
    room: String,
  },
  ConnectToPeer {
    token: u32,
    username: String,
    connection_type: PeerConnectionType,
  },
  PrivateMessage {
    username: String,
    message: String,
  },
  AcknowledgePrivateMessage {
    id: u32,
  },
  /// obsolete
  FileSearchRoom {
    token: u32,
    room_id: u32,
    query: String,
  },
  FileSearch {
    token: u32,
    query: String,
  },
  SetOnlineStatus {
    status: UserPresence,
  },
  /// a write-liveness probe; the server never answers
  Ping,
  /// obsolete
  SendConnectToken {
    username: String,
    token: u32,
  },
  /// obsolete `SendDownloadSpeed`
  SendSpeed {
    username: String,
    speed: u32,
  },
  SharedFoldersAndFiles {
    directory_count: u32,
    file_count: u32,
  },
  GetUserStats {
    username: String,
  },
  /// obsolete `UploadSlotsFull`
  QueuedDownloads {
    slots_full: bool,
  },
  UserSearch {
    username: String,
    token: u32,
    query: String,
  },
  /// obsolete
  SimilarRecommendations {
    recommendation: String,
  },
  LikedInterestAdd {
    interest: String,
  },
  LikedInterestRemove {
    interest: String,
  },
  GetPersonalRecommendations,
  /// obsolete
  MyRecommendations,
  GetGlobalRecommendations,
  GetUserInterests {
    username: String,
  },
  /// obsolete
  AdminCommand {
    command: String,
    args: Vec<String>,
  },
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
  RoomList,
  /// obsolete
  ExactFileSearch {
    token: u32,
    filename: RemoteFilename,
    path: RemoteFilename,
    file_size: u64,
    checksum: u32,
    unknown: u8,
  },
  /// obsolete
  GlobalUserList,
  /// obsolete
  TunneledMessage {
    username: String,
    token: u32,
    code: u32,
    message: String,
  },
  HaveNoParents {
    have_no_parents: bool,
  },
  /// deprecated, never sent; an empty payload means no parent
  ParentsIP {
    ip: Option<Ipv4Addr>,
  },
  CheckPrivileges,
  AcceptChildren {
    accept: bool,
  },
  WishlistSearch {
    token: u32,
    query: String,
  },
  GetPersonalSimilarUsers,
  GetInterestRecommendations {
    interest: String,
  },
  GetInterestSimilarUsers {
    interest: String,
  },
  SetRoomTicker {
    room: String,
    /// empty clears the ticker
    ticker: String,
  },
  HatedInterestAdd {
    interest: String,
  },
  HatedInterestRemove {
    interest: String,
  },
  RoomSearch {
    room: String,
    token: u32,
    query: String,
  },
  SendUploadSpeed {
    /// bytes/second
    speed: u32,
  },
  /// deprecated
  UserPrivileges {
    username: String,
  },
  GivePrivileges {
    username: String,
    days: u32,
  },
  /// deprecated
  AcknowledgeNotifyPrivileges {
    id: u32,
  },
  BranchLevel {
    level: u32,
  },
  BranchRoot {
    root: String,
  },
  /// obsolete, never sent
  ChildDepth {
    depth: u32,
  },
  PrivateRoomAddUser {
    room: String,
    username: String,
  },
  PrivateRoomRemoveUser {
    room: String,
    username: String,
  },
  PrivateRoomDropMembership {
    room: String,
  },
  PrivateRoomDropOwnership {
    room: String,
  },
  /// obsolete
  PrivateRoomUnknown {
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
  /// codec only
  MessageUsers {
    usernames: Vec<String>,
    message: String,
  },
  AskPublicChat,
  StopPublicChat,
  /// obsolete
  RelatedSearch {
    query: String,
  },
  CannotConnect {
    token: u32,
    /// the peer we could not reach
    username: String,
  },
}

impl ServerRequest {
  /// a `Login` with the hash computed from the credentials
  pub fn login(username: String, password: String, major_version: u32, minor_version: u32) -> Self {
    let hash = Secret::new(login_hash(&username, &password));
    Self::Login {
      username,
      password: Secret::new(password),
      hash,
      major_version,
      minor_version,
    }
  }

  pub fn code(&self) -> ServerCode {
    match self {
      Self::Login { .. } => ServerCode::Login,
      Self::SetListenPort { .. } => ServerCode::SetListenPort,
      Self::GetPeerAddress { .. } => ServerCode::GetPeerAddress,
      Self::WatchUser { .. } => ServerCode::WatchUser,
      Self::UnwatchUser { .. } => ServerCode::UnwatchUser,
      Self::GetStatus { .. } => ServerCode::GetStatus,
      Self::IgnoreUser { .. } => ServerCode::IgnoreUser,
      Self::UnignoreUser { .. } => ServerCode::UnignoreUser,
      Self::SayInChatRoom { .. } => ServerCode::SayInChatRoom,
      Self::JoinRoom { .. } => ServerCode::JoinRoom,
      Self::LeaveRoom { .. } => ServerCode::LeaveRoom,
      Self::ConnectToPeer { .. } => ServerCode::ConnectToPeer,
      Self::PrivateMessage { .. } => ServerCode::PrivateMessage,
      Self::AcknowledgePrivateMessage { .. } => ServerCode::AcknowledgePrivateMessage,
      Self::FileSearchRoom { .. } => ServerCode::FileSearchRoom,
      Self::FileSearch { .. } => ServerCode::FileSearch,
      Self::SetOnlineStatus { .. } => ServerCode::SetOnlineStatus,
      Self::Ping => ServerCode::Ping,
      Self::SendConnectToken { .. } => ServerCode::SendConnectToken,
      Self::SendSpeed { .. } => ServerCode::SendSpeed,
      Self::SharedFoldersAndFiles { .. } => ServerCode::SharedFoldersAndFiles,
      Self::GetUserStats { .. } => ServerCode::GetUserStats,
      Self::QueuedDownloads { .. } => ServerCode::QueuedDownloads,
      Self::UserSearch { .. } => ServerCode::UserSearch,
      Self::SimilarRecommendations { .. } => ServerCode::SimilarRecommendations,
      Self::LikedInterestAdd { .. } => ServerCode::LikedInterestAdd,
      Self::LikedInterestRemove { .. } => ServerCode::LikedInterestRemove,
      Self::GetPersonalRecommendations => ServerCode::GetPersonalRecommendations,
      Self::MyRecommendations => ServerCode::MyRecommendations,
      Self::GetGlobalRecommendations => ServerCode::GetGlobalRecommendations,
      Self::GetUserInterests { .. } => ServerCode::GetUserInterests,
      Self::AdminCommand { .. } => ServerCode::AdminCommand,
      Self::PlaceInLineRequest { .. } => ServerCode::PlaceInLineRequest,
      Self::PlaceInLineResponse { .. } => ServerCode::PlaceInLineResponse,
      Self::RoomList => ServerCode::RoomList,
      Self::ExactFileSearch { .. } => ServerCode::ExactFileSearch,
      Self::GlobalUserList => ServerCode::GlobalUserList,
      Self::TunneledMessage { .. } => ServerCode::TunneledMessage,
      Self::HaveNoParents { .. } => ServerCode::HaveNoParents,
      Self::ParentsIP { .. } => ServerCode::ParentsIP,
      Self::CheckPrivileges => ServerCode::CheckPrivileges,
      Self::AcceptChildren { .. } => ServerCode::AcceptChildren,
      Self::WishlistSearch { .. } => ServerCode::WishlistSearch,
      Self::GetPersonalSimilarUsers => ServerCode::GetPersonalSimilarUsers,
      Self::GetInterestRecommendations { .. } => ServerCode::GetInterestRecommendations,
      Self::GetInterestSimilarUsers { .. } => ServerCode::GetInterestSimilarUsers,
      Self::SetRoomTicker { .. } => ServerCode::SetRoomTicker,
      Self::HatedInterestAdd { .. } => ServerCode::HatedInterestAdd,
      Self::HatedInterestRemove { .. } => ServerCode::HatedInterestRemove,
      Self::RoomSearch { .. } => ServerCode::RoomSearch,
      Self::SendUploadSpeed { .. } => ServerCode::SendUploadSpeed,
      Self::UserPrivileges { .. } => ServerCode::UserPrivileges,
      Self::GivePrivileges { .. } => ServerCode::GivePrivileges,
      Self::AcknowledgeNotifyPrivileges { .. } => ServerCode::AcknowledgeNotifyPrivileges,
      Self::BranchLevel { .. } => ServerCode::BranchLevel,
      Self::BranchRoot { .. } => ServerCode::BranchRoot,
      Self::ChildDepth { .. } => ServerCode::ChildDepth,
      Self::PrivateRoomAddUser { .. } => ServerCode::PrivateRoomAddUser,
      Self::PrivateRoomRemoveUser { .. } => ServerCode::PrivateRoomRemoveUser,
      Self::PrivateRoomDropMembership { .. } => ServerCode::PrivateRoomDropMembership,
      Self::PrivateRoomDropOwnership { .. } => ServerCode::PrivateRoomDropOwnership,
      Self::PrivateRoomUnknown { .. } => ServerCode::PrivateRoomUnknown,
      Self::PrivateRoomToggle { .. } => ServerCode::PrivateRoomToggle,
      Self::NewPassword { .. } => ServerCode::NewPassword,
      Self::PrivateRoomAddOperator { .. } => ServerCode::PrivateRoomAddOperator,
      Self::PrivateRoomRemoveOperator { .. } => ServerCode::PrivateRoomRemoveOperator,
      Self::MessageUsers { .. } => ServerCode::MessageUsers,
      Self::AskPublicChat => ServerCode::AskPublicChat,
      Self::StopPublicChat => ServerCode::StopPublicChat,
      Self::RelatedSearch { .. } => ServerCode::RelatedSearch,
      Self::CannotConnect { .. } => ServerCode::CannotConnect,
    }
  }

  /// the complete frame, length prefix included
  pub fn encode(&self) -> Vec<u8> {
    let mut w = Writer::frame(self.code().code());
    match self {
      Self::Login {
        username,
        password,
        hash,
        major_version,
        minor_version,
      } => {
        w.string(username);
        w.string(password.expose());
        w.u32(*major_version);
        w.string(hash.expose());
        w.u32(*minor_version);
      }
      Self::SetListenPort { port, obfuscation } => {
        w.u32(*port);
        if let Some((obfuscation_type, obfuscated_port)) = obfuscation {
          w.u32(*obfuscation_type);
          w.u32(*obfuscated_port);
        }
      }
      Self::GetPeerAddress { username }
      | Self::WatchUser { username }
      | Self::UnwatchUser { username }
      | Self::GetStatus { username }
      | Self::IgnoreUser { username }
      | Self::UnignoreUser { username }
      | Self::GetUserStats { username }
      | Self::GetUserInterests { username }
      | Self::UserPrivileges { username } => w.string(username),
      Self::SayInChatRoom { room, message } => {
        w.string(room);
        w.string(message);
      }
      Self::JoinRoom { room, room_type } => {
        w.string(room);
        w.u32(*room_type as u32);
      }
      Self::LeaveRoom { room }
      | Self::PrivateRoomDropMembership { room }
      | Self::PrivateRoomDropOwnership { room }
      | Self::PrivateRoomUnknown { room } => w.string(room),
      Self::ConnectToPeer {
        token,
        username,
        connection_type,
      } => {
        w.u32(*token);
        w.string(username);
        w.string(connection_type.as_str());
      }
      Self::PrivateMessage { username, message } => {
        w.string(username);
        w.string(message);
      }
      Self::AcknowledgePrivateMessage { id } | Self::AcknowledgeNotifyPrivileges { id } => {
        w.u32(*id)
      }
      Self::FileSearchRoom {
        token,
        room_id,
        query,
      } => {
        w.u32(*token);
        w.u32(*room_id);
        w.string(query);
      }
      Self::FileSearch { token, query } | Self::WishlistSearch { token, query } => {
        w.u32(*token);
        w.string(query);
      }
      Self::SetOnlineStatus { status } => w.u32(*status as u32),
      Self::Ping
      | Self::GetPersonalRecommendations
      | Self::MyRecommendations
      | Self::GetGlobalRecommendations
      | Self::RoomList
      | Self::GlobalUserList
      | Self::CheckPrivileges
      | Self::GetPersonalSimilarUsers
      | Self::AskPublicChat
      | Self::StopPublicChat => {}
      Self::SendConnectToken { username, token } | Self::PlaceInLineRequest { username, token } => {
        w.string(username);
        w.u32(*token);
      }
      Self::SendSpeed { username, speed } => {
        w.string(username);
        w.u32(*speed);
      }
      Self::SharedFoldersAndFiles {
        directory_count,
        file_count,
      } => {
        w.u32(*directory_count);
        w.u32(*file_count);
      }
      Self::QueuedDownloads { slots_full } => w.u32(u32::from(*slots_full)),
      Self::UserSearch {
        username,
        token,
        query,
      } => {
        w.string(username);
        w.u32(*token);
        w.string(query);
      }
      Self::SimilarRecommendations { recommendation } => w.string(recommendation),
      Self::LikedInterestAdd { interest }
      | Self::LikedInterestRemove { interest }
      | Self::HatedInterestAdd { interest }
      | Self::HatedInterestRemove { interest }
      | Self::GetInterestRecommendations { interest }
      | Self::GetInterestSimilarUsers { interest } => w.string(interest),
      Self::AdminCommand { command, args } => {
        w.string(command);
        w.strings(args);
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
      Self::ExactFileSearch {
        token,
        filename,
        path,
        file_size,
        checksum,
        unknown,
      } => {
        w.u32(*token);
        w.filename(filename);
        w.filename(path);
        w.u64(*file_size);
        w.u32(*checksum);
        w.u8(*unknown);
      }
      Self::TunneledMessage {
        username,
        token,
        code,
        message,
      } => {
        w.string(username);
        w.u32(*token);
        w.u32(*code);
        w.string(message);
      }
      Self::HaveNoParents {
        have_no_parents: flag,
      }
      | Self::AcceptChildren { accept: flag }
      | Self::PrivateRoomToggle {
        accept_invitations: flag,
      } => w.bool(*flag),
      Self::ParentsIP { ip } => {
        if let Some(ip) = ip {
          w.ip4(*ip);
        }
      }
      Self::SetRoomTicker { room, ticker } => {
        w.string(room);
        w.string(ticker);
      }
      Self::RoomSearch { room, token, query } => {
        w.string(room);
        w.u32(*token);
        w.string(query);
      }
      Self::SendUploadSpeed { speed } => w.u32(*speed),
      Self::GivePrivileges { username, days } => {
        w.string(username);
        w.u32(*days);
      }
      Self::BranchLevel { level } => w.u32(*level),
      Self::BranchRoot { root } => w.string(root),
      Self::ChildDepth { depth } => w.u32(*depth),
      Self::PrivateRoomAddUser { room, username }
      | Self::PrivateRoomRemoveUser { room, username }
      | Self::PrivateRoomAddOperator { room, username }
      | Self::PrivateRoomRemoveOperator { room, username } => {
        w.string(room);
        w.string(username);
      }
      Self::NewPassword { password } => w.string(password.expose()),
      Self::MessageUsers { usernames, message } => {
        w.strings(usernames);
        w.string(message);
      }
      Self::RelatedSearch { query } => w.string(query),
      Self::CannotConnect { token, username } => {
        w.u32(*token);
        w.string(username);
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
        "server request",
        frame.code,
        code.map(ServerCode::name),
        error,
      )
    })?;
    log::decoded(
      "server request",
      frame.code,
      message.code().name(),
      message.key_fields(),
    );
    Ok(message)
  }

  fn read(code: ServerCode, payload: &[u8]) -> Result<Self, DecodeError> {
    let r = &mut Reader::new(payload);
    Ok(match code {
      ServerCode::Login => Self::Login {
        username: r.string()?,
        password: Secret::new(r.string()?),
        major_version: r.u32()?,
        hash: Secret::new(r.string()?),
        minor_version: r.u32()?,
      },
      ServerCode::SetListenPort => Self::SetListenPort {
        port: r.u32()?,
        obfuscation: if r.remaining() >= 8 {
          Some((r.u32()?, r.u32()?))
        } else {
          None
        },
      },
      ServerCode::GetPeerAddress => Self::GetPeerAddress {
        username: r.string()?,
      },
      ServerCode::WatchUser => Self::WatchUser {
        username: r.string()?,
      },
      ServerCode::UnwatchUser => Self::UnwatchUser {
        username: r.string()?,
      },
      ServerCode::GetStatus => Self::GetStatus {
        username: r.string()?,
      },
      ServerCode::IgnoreUser => Self::IgnoreUser {
        username: r.string()?,
      },
      ServerCode::UnignoreUser => Self::UnignoreUser {
        username: r.string()?,
      },
      ServerCode::SayInChatRoom => Self::SayInChatRoom {
        room: r.string()?,
        message: r.string()?,
      },
      ServerCode::JoinRoom => Self::JoinRoom {
        room: r.string()?,
        room_type: RoomType::from_u32(r.u32()?)?,
      },
      ServerCode::LeaveRoom => Self::LeaveRoom { room: r.string()? },
      ServerCode::ConnectToPeer => Self::ConnectToPeer {
        token: r.u32()?,
        username: r.string()?,
        connection_type: PeerConnectionType::parse(&r.string()?)?,
      },
      ServerCode::PrivateMessage => Self::PrivateMessage {
        username: r.string()?,
        message: r.string()?,
      },
      ServerCode::AcknowledgePrivateMessage => Self::AcknowledgePrivateMessage { id: r.u32()? },
      ServerCode::FileSearchRoom => Self::FileSearchRoom {
        token: r.u32()?,
        room_id: r.u32()?,
        query: r.string()?,
      },
      ServerCode::FileSearch => Self::FileSearch {
        token: r.u32()?,
        query: r.string()?,
      },
      ServerCode::SetOnlineStatus => Self::SetOnlineStatus {
        status: UserPresence::from_u32(r.u32()?)?,
      },
      ServerCode::Ping => Self::Ping,
      ServerCode::SendConnectToken => Self::SendConnectToken {
        username: r.string()?,
        token: r.u32()?,
      },
      ServerCode::SendSpeed => Self::SendSpeed {
        username: r.string()?,
        speed: r.u32()?,
      },
      ServerCode::SharedFoldersAndFiles => Self::SharedFoldersAndFiles {
        directory_count: r.u32()?,
        file_count: r.u32()?,
      },
      ServerCode::GetUserStats => Self::GetUserStats {
        username: r.string()?,
      },
      ServerCode::QueuedDownloads => Self::QueuedDownloads {
        slots_full: r.u32()? != 0,
      },
      ServerCode::UserSearch => Self::UserSearch {
        username: r.string()?,
        token: r.u32()?,
        query: r.string()?,
      },
      ServerCode::SimilarRecommendations => Self::SimilarRecommendations {
        recommendation: r.string()?,
      },
      ServerCode::LikedInterestAdd => Self::LikedInterestAdd {
        interest: r.string()?,
      },
      ServerCode::LikedInterestRemove => Self::LikedInterestRemove {
        interest: r.string()?,
      },
      ServerCode::GetPersonalRecommendations => Self::GetPersonalRecommendations,
      ServerCode::MyRecommendations => Self::MyRecommendations,
      ServerCode::GetGlobalRecommendations => Self::GetGlobalRecommendations,
      ServerCode::GetUserInterests => Self::GetUserInterests {
        username: r.string()?,
      },
      ServerCode::AdminCommand => Self::AdminCommand {
        command: r.string()?,
        args: r.strings("admin command arguments")?,
      },
      ServerCode::PlaceInLineRequest => Self::PlaceInLineRequest {
        username: r.string()?,
        token: r.u32()?,
      },
      ServerCode::PlaceInLineResponse => Self::PlaceInLineResponse {
        username: r.string()?,
        token: r.u32()?,
        place: r.u32()?,
      },
      ServerCode::RoomList => Self::RoomList,
      ServerCode::ExactFileSearch => Self::ExactFileSearch {
        token: r.u32()?,
        filename: r.filename()?,
        path: r.filename()?,
        file_size: r.u64()?,
        checksum: r.u32()?,
        unknown: r.u8()?,
      },
      ServerCode::GlobalUserList => Self::GlobalUserList,
      ServerCode::TunneledMessage => Self::TunneledMessage {
        username: r.string()?,
        token: r.u32()?,
        code: r.u32()?,
        message: r.string()?,
      },
      ServerCode::HaveNoParents => Self::HaveNoParents {
        have_no_parents: r.bool()?,
      },
      ServerCode::ParentsIP => Self::ParentsIP {
        ip: if r.has_more() { Some(r.ip4()?) } else { None },
      },
      ServerCode::CheckPrivileges => Self::CheckPrivileges,
      ServerCode::AcceptChildren => Self::AcceptChildren { accept: r.bool()? },
      ServerCode::WishlistSearch => Self::WishlistSearch {
        token: r.u32()?,
        query: r.string()?,
      },
      ServerCode::GetPersonalSimilarUsers => Self::GetPersonalSimilarUsers,
      ServerCode::GetInterestRecommendations => Self::GetInterestRecommendations {
        interest: r.string()?,
      },
      ServerCode::GetInterestSimilarUsers => Self::GetInterestSimilarUsers {
        interest: r.string()?,
      },
      ServerCode::SetRoomTicker => Self::SetRoomTicker {
        room: r.string()?,
        ticker: r.string()?,
      },
      ServerCode::HatedInterestAdd => Self::HatedInterestAdd {
        interest: r.string()?,
      },
      ServerCode::HatedInterestRemove => Self::HatedInterestRemove {
        interest: r.string()?,
      },
      ServerCode::RoomSearch => Self::RoomSearch {
        room: r.string()?,
        token: r.u32()?,
        query: r.string()?,
      },
      ServerCode::SendUploadSpeed => Self::SendUploadSpeed { speed: r.u32()? },
      ServerCode::UserPrivileges => Self::UserPrivileges {
        username: r.string()?,
      },
      ServerCode::GivePrivileges => Self::GivePrivileges {
        username: r.string()?,
        days: r.u32()?,
      },
      ServerCode::AcknowledgeNotifyPrivileges => Self::AcknowledgeNotifyPrivileges { id: r.u32()? },
      ServerCode::BranchLevel => Self::BranchLevel { level: r.u32()? },
      ServerCode::BranchRoot => Self::BranchRoot { root: r.string()? },
      ServerCode::ChildDepth => Self::ChildDepth { depth: r.u32()? },
      ServerCode::PrivateRoomAddUser => Self::PrivateRoomAddUser {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::PrivateRoomRemoveUser => Self::PrivateRoomRemoveUser {
        room: r.string()?,
        username: r.string()?,
      },
      ServerCode::PrivateRoomDropMembership => {
        Self::PrivateRoomDropMembership { room: r.string()? }
      }
      ServerCode::PrivateRoomDropOwnership => Self::PrivateRoomDropOwnership { room: r.string()? },
      ServerCode::PrivateRoomUnknown => Self::PrivateRoomUnknown { room: r.string()? },
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
      ServerCode::MessageUsers => Self::MessageUsers {
        usernames: r.strings("message recipients")?,
        message: r.string()?,
      },
      ServerCode::AskPublicChat => Self::AskPublicChat,
      ServerCode::StopPublicChat => Self::StopPublicChat,
      ServerCode::RelatedSearch => Self::RelatedSearch { query: r.string()? },
      ServerCode::CannotConnect => Self::CannotConnect {
        token: r.u32()?,
        username: r.string()?,
      },
      // codes the server sends but a client never does
      ServerCode::UserJoinedRoom
      | ServerCode::UserLeftRoom
      | ServerCode::KickedFromServer
      | ServerCode::RoomAdded
      | ServerCode::RoomRemoved
      | ServerCode::GlobalAdminMessage
      | ServerCode::PrivilegedUsers
      | ServerCode::ParentMinSpeed
      | ServerCode::ParentSpeedRatio
      | ServerCode::ParentInactivityTimeout
      | ServerCode::SearchInactivityTimeout
      | ServerCode::MinimumParentsInCache
      | ServerCode::DistributedAliveInterval
      | ServerCode::AddPrivilegedUser
      | ServerCode::EmbeddedMessage
      | ServerCode::NetInfo
      | ServerCode::WishlistInterval
      | ServerCode::RoomTickers
      | ServerCode::RoomTickerAdd
      | ServerCode::RoomTickerRemove
      | ServerCode::NotifyPrivileges
      | ServerCode::DistributedReset
      | ServerCode::PrivateRoomUsers
      | ServerCode::PrivateRoomAdded
      | ServerCode::PrivateRoomRemoved
      | ServerCode::PrivateRoomOperatorAdded
      | ServerCode::PrivateRoomOperatorRemoved
      | ServerCode::PrivateRoomOwned
      | ServerCode::PublicChat
      | ServerCode::ExcludedSearchPhrases
      | ServerCode::CannotJoinRoom => return Err(DecodeError::UnknownCode { code: code.code() }),
    })
  }

  fn key_fields(&self) -> KeyFields<'_> {
    match self {
      Self::Login { username, .. }
      | Self::GetPeerAddress { username }
      | Self::WatchUser { username }
      | Self::UnwatchUser { username }
      | Self::GetStatus { username }
      | Self::IgnoreUser { username }
      | Self::UnignoreUser { username }
      | Self::PrivateMessage { username, .. }
      | Self::SendSpeed { username, .. }
      | Self::GetUserStats { username }
      | Self::GetUserInterests { username }
      | Self::UserPrivileges { username }
      | Self::GivePrivileges { username, .. } => KeyFields::username(username),
      Self::SayInChatRoom { room, .. }
      | Self::JoinRoom { room, .. }
      | Self::LeaveRoom { room }
      | Self::SetRoomTicker { room, .. }
      | Self::PrivateRoomDropMembership { room }
      | Self::PrivateRoomDropOwnership { room }
      | Self::PrivateRoomUnknown { room } => KeyFields::room(room),
      Self::PrivateRoomAddUser { room, username }
      | Self::PrivateRoomRemoveUser { room, username }
      | Self::PrivateRoomAddOperator { room, username }
      | Self::PrivateRoomRemoveOperator { room, username } => {
        KeyFields::room(room).with_username(username)
      }
      Self::RoomSearch { room, token, .. } => KeyFields::room(room).with_token(*token),
      Self::ConnectToPeer {
        token, username, ..
      }
      | Self::SendConnectToken { username, token }
      | Self::UserSearch {
        username, token, ..
      }
      | Self::PlaceInLineRequest { username, token }
      | Self::PlaceInLineResponse {
        username, token, ..
      }
      | Self::TunneledMessage {
        username, token, ..
      }
      | Self::CannotConnect { token, username } => KeyFields::username(username).with_token(*token),
      Self::FileSearchRoom { token, .. }
      | Self::FileSearch { token, .. }
      | Self::WishlistSearch { token, .. }
      | Self::ExactFileSearch { token, .. } => KeyFields::token(*token),
      Self::AcknowledgePrivateMessage { id } | Self::AcknowledgeNotifyPrivileges { id } => {
        KeyFields::token(*id)
      }
      Self::BranchRoot { root } => KeyFields::username(root),
      Self::MessageUsers { usernames, .. } => KeyFields::count(usernames.len()),
      _ => KeyFields::default(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn login_hash_is_md5_of_username_and_password() {
    // md5("usernamepassword")
    assert_eq!(
      login_hash("username", "password"),
      "d51c9a7e9353746a6020f9602d452929"
    );
  }

  #[test]
  fn login_debug_never_shows_credentials() {
    let login = ServerRequest::login("alice".into(), "hunter2".into(), 1, 2);
    let debug = format!("{login:?}");
    assert!(!debug.contains("hunter2"));
    assert!(!debug.contains(&login_hash("alice", "hunter2")));
    assert!(debug.contains("alice"));
  }
}
