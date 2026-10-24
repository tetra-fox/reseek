//! Byte-exact round trips for every message in the catalogue. Each vector is built from wire
//! primitives with the `P` builder below, not with the crate's own writer, then checked both
//! ways: `encode` must produce exactly those bytes and `decode` must return the message. The
//! vectors double as the fuzz seed corpus (`write_fuzz_corpus`).

use std::io::Read;
use std::net::Ipv4Addr;
use std::path::Path;

use flate2::Compression;
use flate2::bufread::{ZlibDecoder, ZlibEncoder};
use soulseek_proto::*;

/// payload builder over wire primitives
#[derive(Clone, Default)]
struct P(Vec<u8>);

fn p() -> P {
  P::default()
}

impl P {
  fn u8(mut self, value: u8) -> Self {
    self.0.push(value);
    self
  }

  fn u16(mut self, value: u16) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  fn u32(mut self, value: u32) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  fn i32(mut self, value: i32) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  fn u64(mut self, value: u64) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  fn raw(mut self, bytes: &[u8]) -> Self {
    self.0.extend(bytes);
    self
  }

  fn bytes_string(self, bytes: &[u8]) -> Self {
    self.u32(bytes.len() as u32).raw(bytes)
  }

  fn s(self, text: &str) -> Self {
    self.bytes_string(text.as_bytes())
  }

  /// the wire holds the octets in reverse order
  fn ip(self, [a, b, c, d]: [u8; 4]) -> Self {
    self.raw(&[d, c, b, a])
  }

  fn frame(self, code: u32) -> Vec<u8> {
    let mut frame = (self.0.len() as u32 + 4).to_le_bytes().to_vec();
    frame.extend(code.to_le_bytes());
    frame.extend(self.0);
    frame
  }

  fn frame_u8(self, code: u8) -> Vec<u8> {
    let mut frame = (self.0.len() as u32 + 1).to_le_bytes().to_vec();
    frame.push(code);
    frame.extend(self.0);
    frame
  }

  /// a peer frame whose payload is this body, zlib-compressed
  fn zframe(self, code: u32) -> Vec<u8> {
    p().raw(&zlib(&self.0)).frame(code)
  }
}

fn first(mut bytes: Vec<u8>, n: usize) -> Vec<u8> {
  bytes.truncate(n);
  bytes
}

fn zlib(body: &[u8]) -> Vec<u8> {
  let mut out = Vec::new();
  ZlibEncoder::new(body, Compression::default())
    .read_to_end(&mut out)
    .unwrap();
  out
}

fn inflate(compressed: &[u8]) -> Vec<u8> {
  let mut out = Vec::new();
  ZlibDecoder::new(compressed).read_to_end(&mut out).unwrap();
  out
}

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

/// the stats tuple with its always-zero fifth field
fn stats_bytes(b: P) -> P {
  b.u32(51_200).u32(12).u32(0).u32(3_400).u32(210)
}

fn attr(attribute_type: FileAttributeType, value: u32) -> FileAttribute {
  FileAttribute {
    attribute_type,
    value,
  }
}

fn mp3(filename: &str) -> File {
  File {
    code: 1,
    filename: filename.into(),
    size: 5_242_880,
    extension: "mp3".into(),
    attributes: vec![
      attr(FileAttributeType::BitRate, 320),
      attr(FileAttributeType::Duration, 245),
      attr(FileAttributeType::VariableBitRate, 0),
    ],
  }
}

fn mp3_bytes(b: P, filename: &str) -> P {
  b.u8(1)
    .s(filename)
    .u64(5_242_880)
    .s("mp3")
    .u32(3)
    .u32(0)
    .u32(320)
    .u32(1)
    .u32(245)
    .u32(2)
    .u32(0)
}

fn flac(filename: &str) -> File {
  File {
    code: 1,
    filename: filename.into(),
    size: 31_457_280,
    extension: String::new(),
    attributes: vec![
      attr(FileAttributeType::Duration, 245),
      attr(FileAttributeType::SampleRate, 44_100),
      attr(FileAttributeType::BitDepth, 16),
      attr(FileAttributeType::Unknown(3), 9),
    ],
  }
}

fn flac_bytes(b: P, filename: &str) -> P {
  b.u8(1)
    .s(filename)
    .u64(31_457_280)
    .s("")
    .u32(4)
    .u32(1)
    .u32(245)
    .u32(4)
    .u32(44_100)
    .u32(5)
    .u32(16)
    .u32(3)
    .u32(9)
}

#[derive(Debug)]
enum Msg {
  Request(ServerRequest),
  Response(ServerResponse),
  Peer(PeerMessage),
  Init(InitMessage),
  Distributed(DistributedMessage),
}

impl Msg {
  fn encode(&self) -> Vec<u8> {
    match self {
      Msg::Request(m) => m.encode(),
      Msg::Response(m) => m.encode(),
      Msg::Peer(m) => m.encode(),
      Msg::Init(m) => m.encode(),
      Msg::Distributed(m) => m.encode(),
    }
  }

  fn assert_decodes(&self, bytes: &[u8]) {
    match self {
      Msg::Request(m) => assert_eq!(&ServerRequest::decode(bytes).unwrap(), m),
      Msg::Response(m) => assert_eq!(&ServerResponse::decode(bytes).unwrap(), m),
      Msg::Peer(m) => assert_eq!(&PeerMessage::decode(bytes).unwrap(), m),
      Msg::Init(m) => assert_eq!(&InitMessage::decode(bytes).unwrap(), m),
      Msg::Distributed(m) => assert_eq!(&DistributedMessage::decode(bytes).unwrap(), m),
    }
  }

  fn fuzz_target(&self) -> &'static str {
    match self {
      Msg::Request(_) | Msg::Response(_) => "server_message",
      Msg::Peer(_) => "peer_message",
      Msg::Init(_) => "peer_init",
      Msg::Distributed(_) => "distributed_message",
    }
  }

  fn frame_kind(&self) -> u8 {
    match self {
      Msg::Request(_) | Msg::Response(_) => 0,
      Msg::Peer(_) => 1,
      Msg::Distributed(_) => 2,
      Msg::Init(_) => 3,
    }
  }
}

fn check(msg: Msg, bytes: Vec<u8>) {
  msg.assert_decodes(&bytes);
  let encoded = msg.encode();
  if encoded.len() > 8 && matches!(&msg, Msg::Peer(m) if matches!(m.code().code(), 5 | 9 | 37)) {
    // compressed: compare the inflated bodies, then the frames
    assert_eq!(
      inflate(&encoded[8..]),
      inflate(&bytes[8..]),
      "inflated body"
    );
  }
  assert_eq!(encoded, bytes);
}

/// `name: message => bytes;` becomes a test that the bytes and the message convert both ways,
/// and an entry in `vectors()` for the fuzz corpus
macro_rules! vectors {
  ($($name:ident: $msg:expr => $bytes:expr;)*) => {
    $(
      #[test]
      fn $name() {
        check($msg, $bytes);
      }
    )*

    fn vectors() -> Vec<(&'static str, Msg, Vec<u8>)> {
      vec![$((stringify!($name), $msg, $bytes),)*]
    }
  };
}

use Msg::{Distributed as D, Init as I, Peer as Pe, Request as Rq, Response as Rs};
use ServerRequest as Q;
use ServerResponse as R;

vectors! {
  // server requests
  server_request_login:
    Rq(Q::login("username".into(), "password".into(), 177, 760))
    => p().s("username").s("password").u32(177).s(MD5_USERNAME_PASSWORD).u32(760).frame(1);
  server_request_set_listen_port:
    Rq(Q::SetListenPort { port: 50_000, obfuscation: None }) => p().u32(50_000).frame(2);
  server_request_set_listen_port_obfuscated:
    Rq(Q::SetListenPort { port: 50_000, obfuscation: Some((1, 50_001)) })
    => p().u32(50_000).u32(1).u32(50_001).frame(2);
  server_request_get_peer_address: Rq(Q::GetPeerAddress { username: "bob".into() }) => p().s("bob").frame(3);
  server_request_watch_user: Rq(Q::WatchUser { username: "bob".into() }) => p().s("bob").frame(5);
  server_request_unwatch_user: Rq(Q::UnwatchUser { username: "bob".into() }) => p().s("bob").frame(6);
  server_request_get_status: Rq(Q::GetStatus { username: "bob".into() }) => p().s("bob").frame(7);
  server_request_ignore_user: Rq(Q::IgnoreUser { username: "bob".into() }) => p().s("bob").frame(11);
  server_request_unignore_user: Rq(Q::UnignoreUser { username: "bob".into() }) => p().s("bob").frame(12);
  server_request_say_in_chat_room:
    Rq(Q::SayInChatRoom { room: "indie".into(), message: "hi all".into() }) => p().s("indie").s("hi all").frame(13);
  server_request_join_room:
    Rq(Q::JoinRoom { room: "indie".into(), room_type: RoomType::Private }) => p().s("indie").u32(1).frame(14);
  server_request_leave_room: Rq(Q::LeaveRoom { room: "indie".into() }) => p().s("indie").frame(15);
  server_request_connect_to_peer:
    Rq(Q::ConnectToPeer { token: 7, username: "bob".into(), connection_type: PeerConnectionType::Peer })
    => p().u32(7).s("bob").s("P").frame(18);
  server_request_private_message:
    Rq(Q::PrivateMessage { username: "bob".into(), message: "hello\nthere".into() })
    => p().s("bob").s("hello\nthere").frame(22);
  server_request_acknowledge_private_message:
    Rq(Q::AcknowledgePrivateMessage { id: 99 }) => p().u32(99).frame(23);
  server_request_file_search_room:
    Rq(Q::FileSearchRoom { token: 8, room_id: 3, query: "a b".into() }) => p().u32(8).u32(3).s("a b").frame(25);
  server_request_file_search:
    Rq(Q::FileSearch { token: 8, query: "boards of canada".into() }) => p().u32(8).s("boards of canada").frame(26);
  server_request_set_online_status:
    Rq(Q::SetOnlineStatus { status: UserPresence::Away }) => p().u32(1).frame(28);
  server_request_ping: Rq(Q::Ping) => p().frame(32);
  server_request_send_connect_token:
    Rq(Q::SendConnectToken { username: "bob".into(), token: 5 }) => p().s("bob").u32(5).frame(33);
  server_request_send_speed:
    Rq(Q::SendSpeed { username: "bob".into(), speed: 1000 }) => p().s("bob").u32(1000).frame(34);
  server_request_shared_folders_and_files:
    Rq(Q::SharedFoldersAndFiles { directory_count: 12, file_count: 345 }) => p().u32(12).u32(345).frame(35);
  server_request_get_user_stats: Rq(Q::GetUserStats { username: "bob".into() }) => p().s("bob").frame(36);
  server_request_queued_downloads: Rq(Q::QueuedDownloads { slots_full: true }) => p().u32(1).frame(40);
  server_request_user_search:
    Rq(Q::UserSearch { username: "bob".into(), token: 9, query: "flac".into() }) => p().s("bob").u32(9).s("flac").frame(42);
  server_request_similar_recommendations:
    Rq(Q::SimilarRecommendations { recommendation: "jazz".into() }) => p().s("jazz").frame(50);
  server_request_liked_interest_add: Rq(Q::LikedInterestAdd { interest: "jazz".into() }) => p().s("jazz").frame(51);
  server_request_liked_interest_remove:
    Rq(Q::LikedInterestRemove { interest: "jazz".into() }) => p().s("jazz").frame(52);
  server_request_get_personal_recommendations: Rq(Q::GetPersonalRecommendations) => p().frame(54);
  server_request_my_recommendations: Rq(Q::MyRecommendations) => p().frame(55);
  server_request_get_global_recommendations: Rq(Q::GetGlobalRecommendations) => p().frame(56);
  server_request_get_user_interests: Rq(Q::GetUserInterests { username: "bob".into() }) => p().s("bob").frame(57);
  server_request_admin_command:
    Rq(Q::AdminCommand { command: "ban".into(), args: vec!["bob".into(), "1d".into()] })
    => p().s("ban").u32(2).s("bob").s("1d").frame(58);
  server_request_place_in_line_request:
    Rq(Q::PlaceInLineRequest { username: "bob".into(), token: 4 }) => p().s("bob").u32(4).frame(59);
  server_request_place_in_line_response:
    Rq(Q::PlaceInLineResponse { username: "bob".into(), token: 4, place: 2 }) => p().s("bob").u32(4).u32(2).frame(60);
  server_request_room_list: Rq(Q::RoomList) => p().frame(64);
  server_request_exact_file_search:
    Rq(Q::ExactFileSearch {
      token: 3, filename: "a.mp3".into(), path: "music\\a".into(), file_size: 1 << 33, checksum: 77, unknown: 1,
    })
    => p().u32(3).s("a.mp3").s("music\\a").u64(1 << 33).u32(77).u8(1).frame(65);
  server_request_global_user_list: Rq(Q::GlobalUserList) => p().frame(67);
  server_request_tunneled_message:
    Rq(Q::TunneledMessage { username: "bob".into(), token: 1, code: 2, message: "m".into() })
    => p().s("bob").u32(1).u32(2).s("m").frame(68);
  server_request_have_no_parents: Rq(Q::HaveNoParents { have_no_parents: true }) => p().u8(1).frame(71);
  server_request_parents_ip: Rq(Q::ParentsIP { ip: Some(ip()) }) => p().ip(IP).frame(73);
  server_request_parents_ip_none: Rq(Q::ParentsIP { ip: None }) => p().frame(73);
  server_request_check_privileges: Rq(Q::CheckPrivileges) => p().frame(92);
  server_request_accept_children: Rq(Q::AcceptChildren { accept: false }) => p().u8(0).frame(100);
  server_request_wishlist_search:
    Rq(Q::WishlistSearch { token: 10, query: "rare".into() }) => p().u32(10).s("rare").frame(103);
  server_request_get_personal_similar_users: Rq(Q::GetPersonalSimilarUsers) => p().frame(110);
  server_request_get_interest_recommendations:
    Rq(Q::GetInterestRecommendations { interest: "jazz".into() }) => p().s("jazz").frame(111);
  server_request_get_interest_similar_users:
    Rq(Q::GetInterestSimilarUsers { interest: "jazz".into() }) => p().s("jazz").frame(112);
  server_request_set_room_ticker:
    Rq(Q::SetRoomTicker { room: "indie".into(), ticker: "".into() }) => p().s("indie").s("").frame(116);
  server_request_hated_interest_add: Rq(Q::HatedInterestAdd { interest: "noise".into() }) => p().s("noise").frame(117);
  server_request_hated_interest_remove:
    Rq(Q::HatedInterestRemove { interest: "noise".into() }) => p().s("noise").frame(118);
  server_request_room_search:
    Rq(Q::RoomSearch { room: "indie".into(), token: 11, query: "ep".into() }) => p().s("indie").u32(11).s("ep").frame(120);
  server_request_send_upload_speed: Rq(Q::SendUploadSpeed { speed: 123_456 }) => p().u32(123_456).frame(121);
  server_request_user_privileges: Rq(Q::UserPrivileges { username: "bob".into() }) => p().s("bob").frame(122);
  server_request_give_privileges:
    Rq(Q::GivePrivileges { username: "bob".into(), days: 30 }) => p().s("bob").u32(30).frame(123);
  server_request_acknowledge_notify_privileges:
    Rq(Q::AcknowledgeNotifyPrivileges { id: 6 }) => p().u32(6).frame(125);
  server_request_branch_level: Rq(Q::BranchLevel { level: 2 }) => p().u32(2).frame(126);
  server_request_branch_root: Rq(Q::BranchRoot { root: "root".into() }) => p().s("root").frame(127);
  server_request_child_depth: Rq(Q::ChildDepth { depth: 1 }) => p().u32(1).frame(129);
  server_request_private_room_add_user:
    Rq(Q::PrivateRoomAddUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(134);
  server_request_private_room_remove_user:
    Rq(Q::PrivateRoomRemoveUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(135);
  server_request_private_room_drop_membership:
    Rq(Q::PrivateRoomDropMembership { room: "club".into() }) => p().s("club").frame(136);
  server_request_private_room_drop_ownership:
    Rq(Q::PrivateRoomDropOwnership { room: "club".into() }) => p().s("club").frame(137);
  server_request_private_room_unknown: Rq(Q::PrivateRoomUnknown { room: "club".into() }) => p().s("club").frame(138);
  server_request_private_room_toggle:
    Rq(Q::PrivateRoomToggle { accept_invitations: true }) => p().u8(1).frame(141);
  server_request_new_password: Rq(Q::NewPassword { password: Secret::new("n3w") }) => p().s("n3w").frame(142);
  server_request_private_room_add_operator:
    Rq(Q::PrivateRoomAddOperator { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(143);
  server_request_private_room_remove_operator:
    Rq(Q::PrivateRoomRemoveOperator { room: "club".into(), username: "bob".into() })
    => p().s("club").s("bob").frame(144);
  server_request_message_users:
    Rq(Q::MessageUsers { usernames: vec!["bob".into(), "eve".into()], message: "hey".into() })
    => p().u32(2).s("bob").s("eve").s("hey").frame(149);
  server_request_ask_public_chat: Rq(Q::AskPublicChat) => p().frame(150);
  server_request_stop_public_chat: Rq(Q::StopPublicChat) => p().frame(151);
  server_request_related_search: Rq(Q::RelatedSearch { query: "jazz".into() }) => p().s("jazz").frame(153);
  server_request_cannot_connect:
    Rq(Q::CannotConnect { token: 12, username: "bob".into() }) => p().u32(12).s("bob").frame(1001);

  // server responses
  server_response_login_success:
    Rs(R::Login(LoginResponse::Success {
      greeting: "welcome\nto soulseek".into(), ip: ip(), hash: Secret::new(MD5_USERNAME_PASSWORD), is_supporter: true,
    }))
    => p().u8(1).s("welcome\nto soulseek").ip(IP).s(MD5_USERNAME_PASSWORD).u8(1).frame(1);
  server_response_login_failure:
    Rs(R::Login(LoginResponse::Failure { reason: "INVALIDPASS".into(), detail: None }))
    => p().u8(0).s("INVALIDPASS").frame(1);
  server_response_login_failure_detail:
    Rs(R::Login(LoginResponse::Failure { reason: "INVALIDUSERNAME".into(), detail: Some("Nick too long.".into()) }))
    => p().u8(0).s("INVALIDUSERNAME").s("Nick too long.").frame(1);
  server_response_get_peer_address:
    Rs(R::GetPeerAddress { username: "bob".into(), ip: ip(), port: 2234, obfuscation: Some((1, 2235)) })
    => p().s("bob").ip(IP).u32(2234).u32(1).u16(2235).frame(3);
  server_response_get_peer_address_without_trailer:
    Rs(R::GetPeerAddress { username: "bob".into(), ip: Ipv4Addr::UNSPECIFIED, port: 0, obfuscation: None })
    => p().s("bob").ip([0, 0, 0, 0]).u32(0).frame(3);
  server_response_watch_user:
    Rs(R::WatchUser {
      username: "bob".into(),
      user: Some(WatchedUser { status: UserPresence::Online, stats: stats(), country_code: Some("NL".into()) }),
    })
    => stats_bytes(p().s("bob").u8(1).u32(2)).s("NL").frame(5);
  server_response_watch_user_blank_country:
    Rs(R::WatchUser {
      username: "bob".into(),
      user: Some(WatchedUser { status: UserPresence::Away, stats: stats(), country_code: Some(String::new()) }),
    })
    => stats_bytes(p().s("bob").u8(1).u32(1)).s("").frame(5);
  server_response_watch_user_offline:
    Rs(R::WatchUser {
      username: "bob".into(),
      user: Some(WatchedUser { status: UserPresence::Offline, stats: stats(), country_code: None }),
    })
    => stats_bytes(p().s("bob").u8(1).u32(0)).frame(5);
  server_response_watch_user_missing:
    Rs(R::WatchUser { username: "nobody".into(), user: None }) => p().s("nobody").u8(0).frame(5);
  server_response_get_status:
    Rs(R::GetStatus(UserStatus { username: "bob".into(), presence: UserPresence::Away, is_privileged: true }))
    => p().s("bob").u32(1).u8(1).frame(7);
  server_response_ignore_user: Rs(R::IgnoreUser { username: "bob".into() }) => p().s("bob").frame(11);
  server_response_unignore_user: Rs(R::UnignoreUser { username: "bob".into() }) => p().s("bob").frame(12);
  server_response_say_in_chat_room:
    Rs(R::SayInChatRoom { room: "indie".into(), username: "bob".into(), message: "hi".into() })
    => p().s("indie").s("bob").s("hi").frame(13);
  server_response_join_room:
    Rs(R::JoinRoom(RoomData {
      name: "indie".into(),
      users: vec![
        UserData { username: "bob".into(), status: UserPresence::Online, stats: stats(), slots_full: false, country_code: "NL".into() },
        UserData { username: "eve".into(), status: UserPresence::Away, stats: stats(), slots_full: true, country_code: "".into() },
      ],
      private: None,
    }))
    => stats_bytes(stats_bytes(p().s("indie").u32(2).s("bob").s("eve").u32(2).u32(2).u32(1).u32(2)))
      .u32(2).u32(0).u32(1).u32(2).s("NL").s("").frame(14);
  server_response_join_room_private:
    Rs(R::JoinRoom(RoomData {
      name: "club".into(),
      users: vec![UserData {
        username: "owner".into(), status: UserPresence::Online, stats: stats(), slots_full: false, country_code: "DE".into(),
      }],
      private: Some(PrivateRoomData { owner: "owner".into(), operators: vec!["op".into()] }),
    }))
    => stats_bytes(p().s("club").u32(1).s("owner").u32(1).u32(2).u32(1))
      .u32(1).u32(0).u32(1).s("DE").s("owner").u32(1).s("op").frame(14);
  server_response_leave_room: Rs(R::LeaveRoom { room: "indie".into() }) => p().s("indie").frame(15);
  server_response_user_joined_room:
    Rs(R::UserJoinedRoom {
      room: "indie".into(),
      user: UserData { username: "bob".into(), status: UserPresence::Online, stats: stats(), slots_full: true, country_code: "FR".into() },
    })
    => stats_bytes(p().s("indie").s("bob").u32(2)).u32(1).s("FR").frame(16);
  server_response_user_left_room:
    Rs(R::UserLeftRoom { room: "indie".into(), username: "bob".into() }) => p().s("indie").s("bob").frame(17);
  server_response_connect_to_peer:
    Rs(R::ConnectToPeer {
      username: "bob".into(), connection_type: PeerConnectionType::Transfer, ip: ip(), port: 2234, token: 77,
      privileged: false, obfuscation: Some((1, 2235)),
    })
    => p().s("bob").s("F").ip(IP).u32(2234).u32(77).u8(0).u32(1).u32(2235).frame(18);
  server_response_connect_to_peer_without_trailer:
    Rs(R::ConnectToPeer {
      username: "bob".into(), connection_type: PeerConnectionType::Distributed, ip: ip(), port: 2234, token: 77,
      privileged: true, obfuscation: None,
    })
    => p().s("bob").s("D").ip(IP).u32(2234).u32(77).u8(1).frame(18);
  server_response_private_message:
    Rs(R::PrivateMessage { id: 5, timestamp: 1_700_000_000, username: "bob".into(), message: "yo".into(), is_new: true })
    => p().u32(5).u32(1_700_000_000).s("bob").s("yo").u8(1).frame(22);
  server_response_private_message_replayed:
    Rs(R::PrivateMessage { id: 5, timestamp: 1_700_000_000, username: "server".into(), message: "x".into(), is_new: false })
    => p().u32(5).u32(1_700_000_000).s("server").s("x").u8(0).frame(22);
  server_response_file_search:
    Rs(R::FileSearch { username: "bob".into(), token: 3, query: "aphex".into() }) => p().s("bob").u32(3).s("aphex").frame(26);
  server_response_ping: Rs(R::Ping) => p().frame(32);
  server_response_send_connect_token:
    Rs(R::SendConnectToken { username: "bob".into(), token: 5 }) => p().s("bob").u32(5).frame(33);
  server_response_get_user_stats:
    Rs(R::GetUserStats(UserStatistics { username: "bob".into(), stats: stats() })) => stats_bytes(p().s("bob")).frame(36);
  server_response_queued_downloads:
    Rs(R::QueuedDownloads { username: "bob".into(), slots_full: false }) => p().s("bob").u32(0).frame(40);
  server_response_kicked_from_server: Rs(R::KickedFromServer) => p().frame(41);
  server_response_similar_recommendations:
    Rs(R::SimilarRecommendations { recommendation: "jazz".into(), similar: vec![] }) => p().s("jazz").u32(0).frame(50);
  server_response_get_personal_recommendations:
    Rs(R::GetPersonalRecommendations(Recommendations {
      recommendations: vec![Recommendation { interest: "jazz".into(), count: 4 }],
      unrecommendations: vec![Recommendation { interest: "noise".into(), count: -2 }],
    }))
    => p().u32(1).s("jazz").i32(4).u32(1).s("noise").i32(-2).frame(54);
  server_response_my_recommendations:
    Rs(R::MyRecommendations { likes: vec!["jazz".into()] }) => p().u32(1).s("jazz").frame(55);
  server_response_get_global_recommendations:
    Rs(R::GetGlobalRecommendations(Recommendations {
      recommendations: vec![Recommendation { interest: "pop".into(), count: 900 }],
      unrecommendations: vec![],
    }))
    => p().u32(1).s("pop").i32(900).u32(0).frame(56);
  server_response_get_user_interests:
    Rs(R::GetUserInterests(UserInterests { username: "bob".into(), likes: vec!["jazz".into()], hates: vec!["noise".into()] }))
    => p().s("bob").u32(1).s("jazz").u32(1).s("noise").frame(57);
  server_response_place_in_line_request:
    Rs(R::PlaceInLineRequest { username: "bob".into(), token: 4 }) => p().s("bob").u32(4).frame(59);
  server_response_place_in_line_response:
    Rs(R::PlaceInLineResponse { username: "bob".into(), token: 4, place: 9 }) => p().s("bob").u32(4).u32(9).frame(60);
  server_response_room_added: Rs(R::RoomAdded { room: "new".into() }) => p().s("new").frame(62);
  server_response_room_removed: Rs(R::RoomRemoved { room: "old".into() }) => p().s("old").frame(63);
  server_response_room_list:
    Rs(R::RoomList(RoomList {
      public: vec![RoomInfo { name: "indie".into(), user_count: 40 }, RoomInfo { name: "jazz".into(), user_count: 7 }],
      owned: vec![RoomInfo { name: "mine".into(), user_count: 2 }],
      private: vec![],
      moderated_room_names: vec!["club".into()],
    }))
    => p().u32(2).s("indie").s("jazz").u32(2).u32(40).u32(7).u32(1).s("mine").u32(1).u32(2).u32(0).u32(0).u32(1).s("club").frame(64);
  server_response_exact_file_search:
    Rs(R::ExactFileSearch {
      username: "bob".into(), token: 3, filename: "a.mp3".into(), path: "music".into(), file_size: 99, checksum: 1,
    })
    => p().s("bob").u32(3).s("a.mp3").s("music").u64(99).u32(1).frame(65);
  server_response_global_admin_message:
    Rs(R::GlobalAdminMessage { message: "maintenance".into() }) => p().s("maintenance").frame(66);
  server_response_global_user_list:
    Rs(R::GlobalUserList {
      users: vec![UserData { username: "bob".into(), status: UserPresence::Online, stats: stats(), slots_full: false, country_code: "NL".into() }],
    })
    => stats_bytes(p().u32(1).s("bob").u32(1).u32(2).u32(1)).u32(1).u32(0).u32(1).s("NL").frame(67);
  server_response_tunneled_message:
    Rs(R::TunneledMessage { username: "bob".into(), code: 2, token: 1, ip: ip(), port: 2234, message: "m".into() })
    => p().s("bob").u32(2).u32(1).ip(IP).u32(2234).s("m").frame(68);
  server_response_privileged_users:
    Rs(R::PrivilegedUsers { usernames: vec!["bob".into(), "eve".into()] }) => p().u32(2).s("bob").s("eve").frame(69);
  server_response_parent_min_speed: Rs(R::ParentMinSpeed { speed: 1 }) => p().u32(1).frame(83);
  server_response_parent_speed_ratio: Rs(R::ParentSpeedRatio { ratio: 50 }) => p().u32(50).frame(84);
  server_response_parent_inactivity_timeout: Rs(R::ParentInactivityTimeout { seconds: 300 }) => p().u32(300).frame(86);
  server_response_search_inactivity_timeout: Rs(R::SearchInactivityTimeout { seconds: 0 }) => p().u32(0).frame(87);
  server_response_minimum_parents_in_cache: Rs(R::MinimumParentsInCache { count: 10 }) => p().u32(10).frame(88);
  server_response_distributed_alive_interval: Rs(R::DistributedAliveInterval { seconds: 0 }) => p().u32(0).frame(90);
  server_response_add_privileged_user: Rs(R::AddPrivilegedUser { username: "bob".into() }) => p().s("bob").frame(91);
  server_response_check_privileges: Rs(R::CheckPrivileges { seconds_remaining: 86_400 }) => p().u32(86_400).frame(92);
  server_response_embedded_message:
    Rs(R::EmbeddedMessage(Embedded {
      distributed_code: 3,
      payload: p().u32(49).s("bob").u32(5).s("query").0,
    }))
    => p().u8(3).u32(49).s("bob").u32(5).s("query").frame(93);
  server_response_net_info:
    Rs(R::NetInfo { parents: vec![ParentCandidate { username: "fast".into(), ip: ip(), port: 2234 }] })
    => p().u32(1).s("fast").ip(IP).u32(2234).frame(102);
  server_response_wishlist_interval: Rs(R::WishlistInterval { seconds: 720 }) => p().u32(720).frame(104);
  server_response_get_personal_similar_users:
    Rs(R::GetPersonalSimilarUsers { users: vec![SimilarUser { username: "bob".into(), rating: 3 }] })
    => p().u32(1).s("bob").u32(3).frame(110);
  server_response_get_interest_recommendations:
    Rs(R::GetInterestRecommendations {
      interest: "jazz".into(),
      recommendations: Recommendations {
        recommendations: vec![Recommendation { interest: "bebop".into(), count: 2 }],
        unrecommendations: vec![Recommendation { interest: "metal".into(), count: -1 }],
      },
    })
    => p().s("jazz").u32(1).s("bebop").i32(2).u32(1).s("metal").i32(-1).frame(111);
  server_response_get_interest_similar_users:
    Rs(R::GetInterestSimilarUsers { interest: "jazz".into(), usernames: vec!["bob".into()] })
    => p().s("jazz").u32(1).s("bob").frame(112);
  server_response_room_tickers:
    Rs(R::RoomTickers {
      room: "indie".into(),
      tickers: vec![RoomTicker { username: "bob".into(), message: "listening".into() }],
    })
    => p().s("indie").u32(1).s("bob").s("listening").frame(113);
  server_response_room_ticker_add:
    Rs(R::RoomTickerAdd { room: "indie".into(), ticker: RoomTicker { username: "bob".into(), message: "t".into() } })
    => p().s("indie").s("bob").s("t").frame(114);
  server_response_room_ticker_remove:
    Rs(R::RoomTickerRemove { room: "indie".into(), username: "bob".into() }) => p().s("indie").s("bob").frame(115);
  server_response_user_privileges:
    Rs(R::UserPrivileges { username: "bob".into(), privileged: true }) => p().s("bob").u8(1).frame(122);
  server_response_notify_privileges:
    Rs(R::NotifyPrivileges { id: 4, username: "bob".into() }) => p().u32(4).s("bob").frame(124);
  server_response_distributed_reset: Rs(R::DistributedReset) => p().frame(130);
  server_response_private_room_users:
    Rs(R::PrivateRoomUsers { room: "club".into(), usernames: vec!["bob".into()] }) => p().s("club").u32(1).s("bob").frame(133);
  server_response_private_room_add_user:
    Rs(R::PrivateRoomAddUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(134);
  server_response_private_room_remove_user:
    Rs(R::PrivateRoomRemoveUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(135);
  server_response_private_room_unknown: Rs(R::PrivateRoomUnknown { room: "club".into() }) => p().s("club").frame(138);
  server_response_private_room_added: Rs(R::PrivateRoomAdded { room: "club".into() }) => p().s("club").frame(139);
  server_response_private_room_removed: Rs(R::PrivateRoomRemoved { room: "club".into() }) => p().s("club").frame(140);
  server_response_private_room_toggle:
    Rs(R::PrivateRoomToggle { accept_invitations: false }) => p().u8(0).frame(141);
  server_response_new_password: Rs(R::NewPassword { password: Secret::new("n3w") }) => p().s("n3w").frame(142);
  server_response_private_room_add_operator:
    Rs(R::PrivateRoomAddOperator { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(143);
  server_response_private_room_remove_operator:
    Rs(R::PrivateRoomRemoveOperator { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(144);
  server_response_private_room_operator_added:
    Rs(R::PrivateRoomOperatorAdded { room: "club".into() }) => p().s("club").frame(145);
  server_response_private_room_operator_removed:
    Rs(R::PrivateRoomOperatorRemoved { room: "club".into() }) => p().s("club").frame(146);
  server_response_private_room_owned:
    Rs(R::PrivateRoomOwned { room: "club".into(), operators: vec!["op".into()] }) => p().s("club").u32(1).s("op").frame(148);
  server_response_public_chat:
    Rs(R::PublicChat { room: "indie".into(), username: "bob".into(), message: "hi".into() })
    => p().s("indie").s("bob").s("hi").frame(152);
  server_response_related_search:
    Rs(R::RelatedSearch { query: "jazz".into(), terms: vec![RelatedTerm { term: "bebop".into(), score: 5 }] })
    => p().s("jazz").u32(1).s("bebop").u32(5).frame(153);
  server_response_excluded_search_phrases:
    Rs(R::ExcludedSearchPhrases { phrases: vec!["bad phrase".into()] }) => p().u32(1).s("bad phrase").frame(160);
  server_response_cannot_connect: Rs(R::CannotConnect { token: 12, username: None }) => p().u32(12).frame(1001);
  server_response_cannot_connect_with_username:
    Rs(R::CannotConnect { token: 12, username: Some("bob".into()) }) => p().u32(12).s("bob").frame(1001);
  server_response_cannot_join_room: Rs(R::CannotJoinRoom { room: "club".into() }) => p().s("club").frame(1003);

  // peer messages
  peer_browse_request: Pe(PeerMessage::BrowseRequest) => p().frame(4);
  peer_browse_response:
    Pe(PeerMessage::BrowseResponse(BrowseResponse {
      directories: vec![
        Directory { name: "@@share\\artist\\album".into(), files: vec![mp3("01 intro.mp3"), flac("02 song.flac")] },
        Directory { name: "@@share\\empty".into(), files: vec![] },
      ],
      locked_directories: vec![Directory { name: "@@share\\friends".into(), files: vec![mp3("secret.mp3")] }],
    }))
    => flac_bytes(mp3_bytes(p().u32(2).s("@@share\\artist\\album").u32(2), "01 intro.mp3"), "02 song.flac")
      .s("@@share\\empty").u32(0).u32(0).u32(1).s("@@share\\friends").u32(1).raw(&mp3_bytes(p(), "secret.mp3").0)
      .zframe(5);
  peer_browse_response_empty:
    Pe(PeerMessage::BrowseResponse(BrowseResponse { directories: vec![], locked_directories: vec![] }))
    => p().u32(0).u32(0).u32(0).zframe(5);
  peer_search_request: Pe(PeerMessage::SearchRequest { token: 3, query: "a".into() }) => p().u32(3).s("a").frame(8);
  peer_search_response:
    Pe(PeerMessage::SearchResponse(SearchResponse {
      username: "bob".into(),
      token: 3,
      files: vec![mp3("@@share\\artist\\01 intro.mp3")],
      has_free_upload_slot: true,
      upload_speed: 51_200,
      queue_length: 4,
      locked_files: vec![flac("@@share\\locked\\x.flac")],
    }))
    => flac_bytes(
      mp3_bytes(p().s("bob").u32(3).u32(1), "@@share\\artist\\01 intro.mp3").u8(1).u32(51_200).u32(4).u32(0).u32(1),
      "@@share\\locked\\x.flac",
    ).zframe(9);
  peer_info_request: Pe(PeerMessage::InfoRequest) => p().frame(15);
  peer_info_response:
    Pe(PeerMessage::InfoResponse(UserInfo {
      description: "hello".into(),
      picture: Some(vec![0x89, b'P', b'N', b'G']),
      upload_slots: 2,
      queue_length: 0,
      has_free_upload_slot: true,
      upload_permitted: Some(1),
    }))
    => p().s("hello").u8(1).u32(4).raw(&[0x89, b'P', b'N', b'G']).u32(2).u32(0).u8(1).u32(1).frame(16);
  peer_info_response_without_picture:
    Pe(PeerMessage::InfoResponse(UserInfo {
      description: String::new(),
      picture: None,
      upload_slots: 0,
      queue_length: 2_147_483_647,
      has_free_upload_slot: false,
      upload_permitted: None,
    }))
    => p().s("").u8(0).u32(0).u32(2_147_483_647).u8(0).frame(16);
  peer_folder_contents_request:
    Pe(PeerMessage::FolderContentsRequest { token: 8, directory_name: "@@share\\artist".into() })
    => p().u32(8).s("@@share\\artist").frame(36);
  peer_folder_contents_response:
    Pe(PeerMessage::FolderContentsResponse(FolderContentsResponse {
      token: 8,
      directory_name: "@@share\\artist".into(),
      directories: vec![
        Directory { name: "@@share\\artist".into(), files: vec![mp3("a.mp3")] },
        Directory { name: "@@share\\artist\\cd2".into(), files: vec![] },
      ],
    }))
    => mp3_bytes(p().u32(8).s("@@share\\artist").u32(2).s("@@share\\artist").u32(1), "a.mp3")
      .s("@@share\\artist\\cd2").u32(0).zframe(37);
  peer_transfer_request_download:
    Pe(PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Download, token: 5, filename: "@@share\\a.mp3".into(), file_size: None,
    }))
    => p().u32(0).u32(5).s("@@share\\a.mp3").frame(40);
  peer_transfer_request_upload:
    Pe(PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Upload, token: 5, filename: "@@share\\a.mp3".into(), file_size: Some(5_242_880),
    }))
    => p().u32(1).u32(5).s("@@share\\a.mp3").u64(5_242_880).frame(40);
  peer_transfer_response_allowed_with_size:
    Pe(PeerMessage::TransferResponse(TransferResponse::Allowed { token: 5, file_size: Some(5_242_880) }))
    => p().u32(5).u8(1).u64(5_242_880).frame(41);
  peer_transfer_response_allowed:
    Pe(PeerMessage::TransferResponse(TransferResponse::Allowed { token: 5, file_size: None })) => p().u32(5).u8(1).frame(41);
  peer_transfer_response_denied:
    Pe(PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, reason: "Queued".into() }))
    => p().u32(5).u8(0).s("Queued").frame(41);
  peer_upload_placehold: Pe(PeerMessage::UploadPlacehold { filename: "a.mp3".into() }) => p().s("a.mp3").frame(42);
  peer_queue_download: Pe(PeerMessage::QueueDownload { filename: "@@share\\a.mp3".into() }) => p().s("@@share\\a.mp3").frame(43);
  peer_queue_download_latin1:
    Pe(PeerMessage::QueueDownload { filename: RemoteFilename::from_bytes(b"caf\xe9\\\xe0.mp3".to_vec()) })
    => p().bytes_string(b"caf\xe9\\\xe0.mp3").frame(43);
  peer_place_in_queue_response:
    Pe(PeerMessage::PlaceInQueueResponse { filename: "a.mp3".into(), place: 3 }) => p().s("a.mp3").u32(3).frame(44);
  peer_upload_failed: Pe(PeerMessage::UploadFailed { filename: "a.mp3".into() }) => p().s("a.mp3").frame(46);
  peer_upload_denied:
    Pe(PeerMessage::UploadDenied { filename: "a.mp3".into(), reason: "File not shared.".into() })
    => p().s("a.mp3").s("File not shared.").frame(50);
  peer_place_in_queue_request:
    Pe(PeerMessage::PlaceInQueueRequest { filename: "a.mp3".into() }) => p().s("a.mp3").frame(51);
  peer_upload_queue_notification: Pe(PeerMessage::UploadQueueNotification) => p().frame(52);

  // init messages
  init_pierce_firewall: I(InitMessage::PierceFirewall { token: 77 }) => p().u32(77).frame_u8(0);
  init_peer_init:
    I(InitMessage::PeerInit { username: "bob".into(), connection_type: PeerConnectionType::Peer, token: 0 })
    => p().s("bob").s("P").u32(0).frame_u8(1);

  // distributed messages
  distributed_ping: D(DistributedMessage::Ping { token: None }) => p().frame_u8(0);
  distributed_ping_with_token: D(DistributedMessage::Ping { token: Some(9) }) => p().u32(9).frame_u8(0);
  distributed_search_request:
    D(DistributedMessage::SearchRequest { username: "bob".into(), token: 5, query: "query".into() })
    => p().u32(49).s("bob").u32(5).s("query").frame_u8(3);
  distributed_branch_level: D(DistributedMessage::BranchLevel { level: 3 }) => p().u32(3).frame_u8(4);
  distributed_branch_root: D(DistributedMessage::BranchRoot { root: "root".into() }) => p().s("root").frame_u8(5);
  distributed_child_depth: D(DistributedMessage::ChildDepth { depth: 2 }) => p().u32(2).frame_u8(7);
  distributed_embedded_message:
    D(DistributedMessage::EmbeddedMessage {
      embedded: Embedded { distributed_code: 3, payload: p().u32(49).s("bob").u32(5).s("q").0 },
      padded: false,
    })
    => p().u8(3).u32(49).s("bob").u32(5).s("q").frame_u8(93);
  distributed_embedded_message_padded:
    D(DistributedMessage::EmbeddedMessage {
      embedded: Embedded { distributed_code: 3, payload: p().u32(49).s("bob").u32(5).s("q").0 },
      padded: true,
    })
    => p().raw(&[0, 0, 0]).u8(3).u32(49).s("bob").u32(5).s("q").frame_u8(93);
}

/// `name: bytes => expected;` for inputs that are accepted but are not what the encoder writes,
/// or that must be rejected
macro_rules! decodes {
  ($($name:ident: $ty:ty, $bytes:expr => $expected:expr;)*) => {
    $(
      #[test]
      fn $name() {
        assert_eq!(<$ty>::decode(&$bytes), $expected);
      }
    )*

    fn decode_vectors() -> Vec<(&'static str, &'static str, Vec<u8>)> {
      vec![$((stringify!($name), stringify!($ty), $bytes),)*]
    }
  };
}

decodes! {
  // compat shapes
  login_success_must_be_exactly_one: ServerResponse, p().u8(2).s("INVALIDPASS").frame(1)
    => Ok(R::Login(LoginResponse::Failure { reason: "INVALIDPASS".into(), detail: None }));
  private_message_is_new_must_be_exactly_one: ServerResponse,
    p().u32(1).u32(2).s("bob").s("m").u8(2).frame(22)
    => Ok(R::PrivateMessage { id: 1, timestamp: 2, username: "bob".into(), message: "m".into(), is_new: false });
  bools_accept_any_non_zero: ServerResponse, p().s("bob").u32(2).u8(7).frame(7)
    => Ok(R::GetStatus(UserStatus { username: "bob".into(), presence: UserPresence::Online, is_privileged: true }));
  get_peer_address_partial_trailer_is_ignored: ServerResponse, p().s("bob").ip(IP).u32(1).u32(1).frame(3)
    => Ok(R::GetPeerAddress { username: "bob".into(), ip: ip(), port: 1, obfuscation: None });
  recommendations_without_second_block: ServerResponse, p().u32(1).s("jazz").i32(-3).frame(54)
    => Ok(R::GetPersonalRecommendations(Recommendations {
      recommendations: vec![Recommendation { interest: "jazz".into(), count: -3 }],
      unrecommendations: vec![],
    }));
  transfer_request_download_ignores_stray_size: PeerMessage, p().u32(0).u32(5).s("a.mp3").u64(0).frame(40)
    => Ok(PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Download, token: 5, filename: "a.mp3".into(), file_size: None,
    }));
  transfer_request_upload_without_size: PeerMessage, p().u32(1).u32(5).s("a.mp3").frame(40)
    => Ok(PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Upload, token: 5, filename: "a.mp3".into(), file_size: None,
    }));
  transfer_response_allowed_must_be_exactly_one: PeerMessage, p().u32(5).u8(2).s("Cancelled").frame(41)
    => Ok(PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, reason: "Cancelled".into() }));
  transfer_response_denied_needs_a_reason: PeerMessage, p().u32(5).u8(0).frame(41)
    => Err(DecodeError::Truncated { needed: 4, remaining: 0 });
  info_response_museek_stray_bytes: PeerMessage, p().s("d").u8(0).u32(1).u32(0).u8(1).raw(&[0, 0, 0]).frame(16)
    => Ok(PeerMessage::InfoResponse(UserInfo {
      description: "d".into(), picture: None, upload_slots: 1, queue_length: 0, has_free_upload_slot: true, upload_permitted: None,
    }));
  search_response_legacy_without_trailer: PeerMessage, p().s("bob").u32(3).u32(0).u8(0).u32(10).u32(2).zframe(9)
    => Ok(PeerMessage::SearchResponse(SearchResponse {
      username: "bob".into(), token: 3, files: vec![], has_free_upload_slot: false, upload_speed: 10, queue_length: 2, locked_files: vec![],
    }));
  search_response_eight_byte_queue_length: PeerMessage, p().s("bob").u32(3).u32(0).u8(1).u32(10).u64(2).zframe(9)
    => Ok(PeerMessage::SearchResponse(SearchResponse {
      username: "bob".into(), token: 3, files: vec![], has_free_upload_slot: true, upload_speed: 10, queue_length: 2, locked_files: vec![],
    }));
  search_response_stray_trailing_byte: PeerMessage, p().s("bob").u32(3).u32(0).u8(1).u32(10).u32(2).u8(0).zframe(9)
    => Ok(PeerMessage::SearchResponse(SearchResponse {
      username: "bob".into(), token: 3, files: vec![], has_free_upload_slot: true, upload_speed: 10, queue_length: 2, locked_files: vec![],
    }));
  browse_response_without_trailer: PeerMessage, p().u32(0).zframe(5)
    => Ok(PeerMessage::BrowseResponse(BrowseResponse { directories: vec![], locked_directories: vec![] }));
  browse_response_ns_sign_extended_size: PeerMessage,
    p().u32(1).s("d").u32(1).u8(1).s("big.iso").u64(0xffff_ffff_ffff_ffff).s("iso").u32(0).u32(0).u32(0).zframe(5)
    => Ok(PeerMessage::BrowseResponse(BrowseResponse {
      directories: vec![Directory {
        name: "d".into(),
        files: vec![File { code: 1, filename: "big.iso".into(), size: 4_294_967_295, extension: "iso".into(), attributes: vec![] }],
      }],
      locked_directories: vec![],
    }));
  embedded_message_with_short_zero_prefix_is_not_padded: DistributedMessage, p().raw(&[0, 0, 0]).frame_u8(93)
    => Ok(DistributedMessage::EmbeddedMessage {
      embedded: Embedded { distributed_code: 0, payload: vec![0, 0] }, padded: false,
    });
  peer_init_token_is_ignored_but_kept: InitMessage, p().s("bob").s("F").u32(1234).frame_u8(1)
    => Ok(InitMessage::PeerInit { username: "bob".into(), connection_type: PeerConnectionType::Transfer, token: 1234 });
  trailing_fields_are_ignored: ServerResponse, p().s("room").raw(&[1, 2, 3]).frame(15)
    => Ok(R::LeaveRoom { room: "room".into() });

  // rejections
  unknown_server_code: ServerResponse, p().u32(1).frame(9999) => Err(DecodeError::UnknownCode { code: 9999 });
  request_only_code_is_not_a_response: ServerResponse, p().u32(1).frame(2) => Err(DecodeError::UnknownCode { code: 2 });
  peer_code_without_layout: PeerMessage, p().u32(1).frame(1) => Err(DecodeError::UnknownCode { code: 1 });
  distributed_placeholder_code: DistributedMessage, p().frame_u8(6) => Err(DecodeError::UnknownCode { code: 6 });
  join_room_with_mismatched_lists: ServerResponse,
    p().s("r").u32(2).s("a").s("b").u32(1).u32(2).frame(14)
    => Err(DecodeError::ListLengthMismatch { what: "user statuses" });
  room_list_with_mismatched_lists: ServerResponse, p().u32(1).s("a").u32(2).u32(1).u32(1).frame(64)
    => Err(DecodeError::ListLengthMismatch { what: "public rooms" });
  room_count_beyond_the_payload: ServerResponse, p().u32(u32::MAX).frame(64)
    => Err(DecodeError::CountTooLarge { what: "public rooms", count: u32::MAX, remaining: 0 });
  user_presence_out_of_range: ServerResponse, p().s("bob").u32(3).u8(0).frame(7)
    => Err(DecodeError::InvalidValue { what: "user presence", value: 3 });
  connect_to_peer_unknown_type: ServerResponse, p().s("bob").s("X").ip(IP).u32(1).u32(1).u8(0).frame(18)
    => Err(DecodeError::UnknownConnectionType);
  peer_init_unknown_type: InitMessage, p().s("bob").s("X").u32(0).frame_u8(1) => Err(DecodeError::UnknownConnectionType);
  peer_init_reserved_username: InitMessage, p().s("server").s("P").u32(0).frame_u8(1)
    => Err(DecodeError::InvalidUsername { what: "peer init" });
  peer_init_empty_username: InitMessage, p().s("").s("P").u32(0).frame_u8(1)
    => Err(DecodeError::InvalidUsername { what: "peer init" });
  init_frame_over_limit: InitMessage, p().raw(&[0; 4096]).frame_u8(1)
    => Err(DecodeError::FrameTooLarge { len: 4097, limit: LIMIT_INIT_MESSAGE_BYTES });
  branch_level_negative: DistributedMessage, p().i32(-1).frame_u8(4)
    => Err(DecodeError::NegativeBranchLevel { level: -1 });
  branch_root_non_printable: DistributedMessage, p().s("a\u{7}b").frame_u8(5)
    => Err(DecodeError::InvalidUsername { what: "branch root" });
  distributed_search_identifier_not_49: DistributedMessage, p().u32(0).s("bob").u32(5).s("q").frame_u8(3)
    => Err(DecodeError::BadSearchIdentifier { identifier: 0 });
  info_response_picture_over_limit: PeerMessage, p().s("").u8(1).u32(16 << 20 | 1).frame(16)
    => Err(DecodeError::LimitExceeded { what: "user info picture bytes", value: 16 << 20 | 1, limit: 16 << 20 });
  file_with_too_many_attributes: PeerMessage,
    p().s("bob").u32(3).u32(1).u8(1).s("f").u64(1).s("").u32(65).raw(&[0; 65 * 8]).zframe(9)
    => Err(DecodeError::LimitExceeded { what: "attributes of one file", value: 65, limit: 64 });
  search_response_file_count_beyond_the_body: PeerMessage, p().s("bob").u32(3).u32(1_000_000).zframe(9)
    => Err(DecodeError::CountTooLarge { what: "files", count: 1_000_000, remaining: 0 });
  compressed_payload_not_zlib: PeerMessage, p().raw(b"plain").frame(5) => Err(DecodeError::InvalidZlib);
  string_longer_than_the_payload: ServerResponse, p().u32(10).raw(b"abc").frame(15)
    => Err(DecodeError::Truncated { needed: 10, remaining: 3 });
  frame_with_bytes_after_it: ServerResponse, [p().frame(32), vec![0]].concat() => Err(DecodeError::TrailingBytes { extra: 1 });
  frame_cut_short: ServerResponse, first(p().s("room").frame(15), 10)
    => Err(DecodeError::IncompleteFrame { needed: 16, available: 10 });
}

#[test]
fn every_catalogue_code_with_a_layout_has_a_vector() {
  let mut missing = Vec::new();
  let vectors = vectors();
  let has = |family: fn(&Msg) -> Option<u32>, code: u32| {
    vectors.iter().any(|(_, m, _)| family(m) == Some(code))
  };
  let request = |m: &Msg| {
    if let Msg::Request(m) = m {
      Some(m.code().code())
    } else {
      None
    }
  };
  let response = |m: &Msg| {
    if let Msg::Response(m) = m {
      Some(m.code().code())
    } else {
      None
    }
  };
  let peer = |m: &Msg| {
    if let Msg::Peer(m) = m {
      Some(m.code().code())
    } else {
      None
    }
  };
  let distributed = |m: &Msg| {
    if let Msg::Distributed(m) = m {
      Some(m.code().code())
    } else {
      None
    }
  };
  let init = |m: &Msg| {
    if let Msg::Init(m) = m {
      Some(m.code().code())
    } else {
      None
    }
  };
  for code in 0..2000 {
    let Some(name) = ServerCode::from_code(code).map(ServerCode::name) else {
      continue;
    };
    if !has(request, code) && !has(response, code) {
      missing.push(format!("server {code} {name}"));
    }
  }
  // peer codes named by the reference without a layout
  let no_layout = [1, 10, 14, 33, 34, 47, 48, 49];
  for code in 0..100 {
    if let Some(c) = PeerCode::from_code(code)
      && !no_layout.contains(&code)
      && !has(peer, code)
    {
      missing.push(format!("peer {code} {}", c.name()));
    }
    // 6 is a placeholder of the original library, not a wire code
    if let Some(c) = DistributedCode::from_code(code)
      && code != 6
      && !has(distributed, code)
    {
      missing.push(format!("distributed {code} {}", c.name()));
    }
    if let Some(c) = InitCode::from_code(code)
      && !has(init, code)
    {
      missing.push(format!("init {code} {}", c.name()));
    }
  }
  assert!(
    missing.is_empty(),
    "codes without a round-trip vector: {missing:#?}"
  );
}

#[test]
fn peer_codes_without_layout_decode_as_unknown() {
  for code in [1, 10, 14, 33, 34, 47, 48, 49] {
    assert_eq!(
      PeerMessage::decode(&p().u32(1).frame(code)),
      Err(DecodeError::UnknownCode { code })
    );
  }
}

#[test]
fn file_transfer_init_round_trip() {
  let raw = 77u32.to_le_bytes();
  assert_eq!(FileTransferInit { token: 77 }.encode(), raw);
  assert_eq!(
    FileTransferInit::decode(&raw),
    Ok(FileTransferInit { token: 77 })
  );
  assert_eq!(
    FileTransferInit::decode(&raw[..3]),
    Err(DecodeError::Truncated {
      needed: 4,
      remaining: 3
    })
  );
}

#[test]
fn file_offset_round_trip() {
  let raw = 1_048_576u64.to_le_bytes();
  assert_eq!(FileOffset { offset: 1_048_576 }.encode(), raw);
  assert_eq!(
    FileOffset::decode(&raw),
    Ok(FileOffset { offset: 1_048_576 })
  );
  // the Soulseek NS resume bug arrives as u64::MAX; decoding keeps it for the uploader to refuse
  assert_eq!(
    FileOffset::decode(&[0xff; 8]),
    Ok(FileOffset { offset: u64::MAX })
  );
  assert_eq!(
    FileOffset::decode(&[0; 9]),
    Err(DecodeError::TrailingBytes { extra: 1 })
  );
}

#[test]
fn embedded_inner_frame_decodes_as_distributed() {
  let embedded = Embedded {
    distributed_code: 3,
    payload: p().u32(49).s("bob").u32(5).s("q").0,
  };
  assert_eq!(
    embedded.inner_frame(),
    p().u32(49).s("bob").u32(5).s("q").frame_u8(3)
  );
  assert_eq!(
    DistributedMessage::decode(&embedded.inner_frame()),
    Ok(DistributedMessage::SearchRequest {
      username: "bob".into(),
      token: 5,
      query: "q".into()
    })
  );
}

#[test]
fn latin1_filename_is_echoed_with_its_original_bytes() {
  let bytes = p().bytes_string(b"caf\xe9.mp3").frame(43);
  let PeerMessage::QueueDownload { filename } = PeerMessage::decode(&bytes).unwrap() else {
    panic!()
  };
  assert_eq!(filename.as_str(), "caf\u{e9}.mp3");
  assert!(filename.is_lossy());
  let echo = PeerMessage::PlaceInQueueRequest { filename }.encode();
  assert_eq!(echo, p().bytes_string(b"caf\xe9.mp3").frame(51));
}

#[test]
fn search_response_peek_stops_before_the_files() {
  // the file list is garbage, but the prefix is enough to drop an unknown token
  let frame = p()
    .s("bob")
    .u32(42)
    .u32(u32::MAX)
    .raw(&[0xff; 16])
    .zframe(9);
  assert_eq!(
    peek_search_response(&frame, &Limits::DEFAULT),
    Ok(("bob".into(), 42))
  );
  assert!(PeerMessage::decode(&frame).is_err());
  let short = p().s("bob").zframe(9);
  assert_eq!(
    peek_search_response(&short, &Limits::DEFAULT),
    Err(DecodeError::Truncated {
      needed: 4,
      remaining: 0
    })
  );
  let wrong_code = p().s("bob").u32(42).zframe(5);
  assert!(peek_search_response(&wrong_code, &Limits::DEFAULT).is_err());
}

#[test]
fn configured_limits_apply() {
  let response = PeerMessage::SearchResponse(SearchResponse {
    username: "bob".into(),
    token: 1,
    files: vec![mp3("a.mp3"), mp3("b.mp3")],
    has_free_upload_slot: true,
    upload_speed: 0,
    queue_length: 0,
    locked_files: vec![mp3("c.mp3")],
  });
  let frame = response.encode();
  let files = Limits {
    files_per_response: 2,
    ..Limits::DEFAULT
  };
  assert_eq!(
    PeerMessage::decode_with(&frame, &files),
    Err(DecodeError::LimitExceeded {
      what: "files in one response",
      value: 3,
      limit: 2
    })
  );
  let inflated = Limits {
    inflated_bytes: 16,
    ..Limits::DEFAULT
  };
  assert_eq!(
    PeerMessage::decode_with(&frame, &inflated),
    Err(DecodeError::InflatedTooLarge { limit: 16 })
  );
  let frame_len = Limits {
    peer_message_bytes: 16,
    ..Limits::DEFAULT
  };
  assert!(matches!(
    PeerMessage::decode_with(&frame, &frame_len),
    Err(DecodeError::FrameTooLarge { limit: 16, .. })
  ));
  assert_eq!(
    PeerMessage::decode_with(&frame, &Limits::DEFAULT),
    Ok(response)
  );
}

#[test]
fn directory_limit_counts_every_list() {
  let mut body = p().u32(LIMIT_DIRECTORIES_PER_RESPONSE as u32);
  for _ in 0..LIMIT_DIRECTORIES_PER_RESPONSE {
    body = body.s("").u32(0);
  }
  let frame = body.u32(0).u32(1).s("").u32(0).zframe(5);
  assert_eq!(
    PeerMessage::decode(&frame),
    Err(DecodeError::LimitExceeded {
      what: "directories in one response",
      value: LIMIT_DIRECTORIES_PER_RESPONSE as u64 + 1,
      limit: LIMIT_DIRECTORIES_PER_RESPONSE as u64,
    })
  );
}

/// Writes every vector into `fuzz/corpus/<target>/` as seeds. Run through `just fuzz-seed`.
#[test]
#[ignore = "writes the fuzz seed corpus; run by `just fuzz-seed`"]
fn write_fuzz_corpus() {
  let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus");
  let write = |target: &str, name: &str, bytes: &[u8]| {
    let dir = corpus.join(target);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
  };
  let mut frames: Vec<(String, u8, Vec<u8>)> = Vec::new();
  for (name, msg, bytes) in vectors() {
    write(msg.fuzz_target(), name, &bytes);
    frames.push((name.to_owned(), msg.frame_kind(), bytes.clone()));
    if let Msg::Peer(peer) = &msg {
      // the compressed targets take a mode byte: 0 = body to compress, 1 = raw zlib stream
      let target = match peer.code() {
        PeerCode::SearchResponse => "compressed_search_response",
        PeerCode::BrowseResponse | PeerCode::FolderContentsResponse => "compressed_browse_response",
        _ => continue,
      };
      let folder = u8::from(peer.code() == PeerCode::FolderContentsResponse) << 1;
      write(
        target,
        &format!("{name}_body"),
        &[&[folder][..], &inflate(&bytes[8..])].concat(),
      );
      write(
        target,
        &format!("{name}_zlib"),
        &[&[folder | 1][..], &bytes[8..]].concat(),
      );
    }
  }
  for (name, family, bytes) in decode_vectors() {
    let (target, kind) = match family {
      "ServerResponse" | "ServerRequest" => ("server_message", 0),
      "PeerMessage" => ("peer_message", 1),
      "DistributedMessage" => ("distributed_message", 2),
      "InitMessage" => ("peer_init", 3),
      other => panic!("no fuzz target for {other}"),
    };
    write(target, name, &bytes);
    frames.push((name.to_owned(), kind, bytes));
  }
  // the frame target takes a frame kind byte, then a stream of frames
  for (name, kind, bytes) in frames {
    write("frame", &name, &[&[kind][..], &bytes].concat());
  }
}
