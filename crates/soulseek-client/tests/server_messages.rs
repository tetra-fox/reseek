//! Every message the server sends, delivered by a fake server to a logged-in client: the wait it
//! completes, the event it raises, or where it is handed on, in code order. The login reply and
//! the kick are covered by the session tests.

mod support;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use soulseek_client::{
  ClientError, ClientOptions, Event, RoomErrorKind, RoomMembers, ServerHandoff, ServerInfo, WaitKey,
};
use soulseek_proto::distributed::Embedded;
use soulseek_proto::server::*;
use soulseek_proto::{
  PeerConnectionType, Secret, ServerCode, ServerRequest, ServerResponse, UserPresence,
};
use support::{Logs, Session, USERNAME, options};
use tokio_util::sync::CancellationToken;

const PATIENCE: Duration = Duration::from_secs(10);

impl Session {
  /// registers a wait under `key`, sends `response` and returns what the wait completed with
  async fn reply<T: Send + 'static>(&mut self, key: WaitKey, response: ServerResponse) -> T {
    let wait = self
      .client
      .waiter()
      .wait::<T>(key, None, CancellationToken::new());
    self.server.send(&response).await;
    tokio::time::timeout(PATIENCE, wait)
      .await
      .expect("the wait completes")
      .unwrap()
  }

  /// sends `response` and returns the next server hand-off
  async fn handoff(&mut self, response: ServerResponse) -> ServerHandoff {
    self.server.send(&response).await;
    tokio::time::timeout(PATIENCE, self.handoffs.server.recv())
      .await
      .expect("the message is handed on")
      .unwrap()
  }

  /// sends a message every client handles by raising an event, and waits for that event, so
  /// everything sent before it has been handled
  async fn settle(&mut self) {
    let marker = ServerResponse::GlobalAdminMessage(GlobalAdminMessage {
      message: "marker".into(),
    });
    self.server.send(&marker).await;
    self
      .event(|event| {
        matches!(event, Event::GlobalMessageReceived(message) if message == "marker").then_some(())
      })
      .await;
  }
}

fn user_key(code: ServerCode, username: &str) -> WaitKey {
  WaitKey::ServerUser {
    code,
    username: username.into(),
  }
}

fn room_key(code: ServerCode, room: &str) -> WaitKey {
  WaitKey::ServerRoom {
    code,
    room: room.into(),
  }
}

fn room_user_key(code: ServerCode, room: &str, username: &str) -> WaitKey {
  WaitKey::ServerRoomUser {
    code,
    room: room.into(),
    username: username.into(),
  }
}

fn stats() -> UserStats {
  UserStats {
    average_speed: 1000,
    upload_count: 5,
    file_count: 300,
    directory_count: 20,
  }
}

fn user_data(username: &str) -> UserData {
  UserData {
    username: username.into(),
    status: UserPresence::Online,
    stats: stats(),
    slots_full: true,
    country_code: "NL".into(),
  }
}

#[tokio::test]
async fn get_peer_address_with_and_without_the_obfuscation_trailer() {
  let mut session = Session::start().await;
  let with = GetPeerAddressResponse {
    username: "bob".into(),
    ip: Ipv4Addr::new(10, 0, 0, 2),
    port: 2234,
    obfuscation: Some(PeerAddressObfuscation {
      obfuscation_type: 1,
      obfuscated_port: 2235,
    }),
  };
  let got: GetPeerAddressResponse = session
    .reply(
      user_key(ServerCode::GetPeerAddress, "bob"),
      ServerResponse::GetPeerAddress(with.clone()),
    )
    .await;
  assert_eq!(got, with);
  let without = GetPeerAddressResponse {
    obfuscation: None,
    ..with
  };
  let got: GetPeerAddressResponse = session
    .reply(
      user_key(ServerCode::GetPeerAddress, "bob"),
      ServerResponse::GetPeerAddress(without.clone()),
    )
    .await;
  assert_eq!(got, without);
}

#[tokio::test]
async fn watch_user_online_offline_and_missing() {
  let mut session = Session::start().await;
  for user in [
    Some(WatchedUser {
      status: UserPresence::Online,
      stats: stats(),
      country_code: Some("NL".into()),
    }),
    Some(WatchedUser {
      status: UserPresence::Offline,
      stats: stats(),
      country_code: None,
    }),
    None,
  ] {
    let response = WatchUserResponse {
      username: "bob".into(),
      user,
    };
    let got: WatchUserResponse = session
      .reply(
        user_key(ServerCode::WatchUser, "bob"),
        ServerResponse::WatchUser(response.clone()),
      )
      .await;
    assert_eq!(got, response);
  }
}

#[tokio::test]
async fn get_status_completes_and_always_raises_the_status() {
  let mut session = Session::start().await;
  let status = UserStatus {
    username: "bob".into(),
    presence: UserPresence::Away,
    is_privileged: true,
  };
  let got: UserStatus = session
    .reply(
      user_key(ServerCode::GetStatus, "bob"),
      ServerResponse::GetStatus(status.clone()),
    )
    .await;
  assert_eq!(got, status);
  let raised = session
    .event(|event| match event {
      Event::UserStatusChanged(status) => Some(status),
      _ => None,
    })
    .await;
  assert_eq!(raised, status);
  // pushed for a watched user, with nobody waiting
  let pushed = UserStatus {
    username: "carol".into(),
    presence: UserPresence::Online,
    is_privileged: false,
  };
  session
    .server
    .send(&ServerResponse::GetStatus(pushed.clone()))
    .await;
  let raised = session
    .event(|event| match event {
      Event::UserStatusChanged(status) => Some(status),
      _ => None,
    })
    .await;
  assert_eq!(raised, pushed);
}

#[tokio::test]
async fn say_in_chat_room_raises_the_message() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::SayInChatRoom(SayInChatRoomResponse {
      room: "indie".into(),
      username: "bob".into(),
      message: "hi".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomMessageReceived {
        room_name,
        username,
        message,
      } => Some((room_name, username, message)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into(), "hi".into()));
}

#[tokio::test]
async fn join_room_completes_with_the_room_and_its_private_block() {
  let mut session = Session::start().await;
  let room = RoomData {
    name: "secret".into(),
    users: vec![user_data("bob"), user_data(USERNAME)],
    private: Some(PrivateRoomData {
      owner: "bob".into(),
      operators: vec!["carol".into()],
    }),
  };
  let got: RoomData = session
    .reply(
      room_key(ServerCode::JoinRoom, "secret"),
      ServerResponse::JoinRoom(room.clone()),
    )
    .await;
  assert_eq!(got, room);
  assert!(got.is_private());
}

#[tokio::test]
async fn leave_room_completes_and_raises_our_own_departure() {
  let mut session = Session::start().await;
  let () = session
    .reply(
      room_key(ServerCode::LeaveRoom, "indie"),
      ServerResponse::LeaveRoom(LeaveRoom {
        room: "indie".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomLeft {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), USERNAME.into()));
}

#[tokio::test]
async fn user_joined_room_raises_the_member_with_their_data() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::UserJoinedRoom(UserJoinedRoom {
      room: "indie".into(),
      user: user_data("bob"),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomJoined {
        room_name,
        username,
        user_data,
      } => Some((room_name, username, user_data)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into(), user_data("bob")));
  assert!(raised.2.slots_full);
}

#[tokio::test]
async fn user_left_room_raises_the_departure() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::UserLeftRoom(UserLeftRoom {
      room: "indie".into(),
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomLeft {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into()));
}

#[tokio::test]
async fn connect_to_peer_is_handed_on_with_its_trailer() {
  let mut session = Session::start().await;
  for obfuscation in [
    Some(Obfuscation {
      obfuscation_type: 1,
      obfuscated_port: 2235,
    }),
    None,
  ] {
    let request = ConnectToPeerResponse {
      username: "bob".into(),
      connection_type: PeerConnectionType::Peer,
      ip: Ipv4Addr::new(10, 0, 0, 2),
      port: 2234,
      token: 77,
      privileged: false,
      obfuscation,
    };
    let handoff = session
      .handoff(ServerResponse::ConnectToPeer(request.clone()))
      .await;
    assert!(matches!(handoff, ServerHandoff::ConnectToPeer(got) if got == request));
  }
}

#[tokio::test]
async fn private_message_is_raised_and_acknowledged() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateMessage(PrivateMessageResponse {
      id: 41,
      timestamp: 1_700_000_000,
      username: "bob".into(),
      message: "hello".into(),
      is_new: false,
    }))
    .await;
  let message = session
    .event(|event| match event {
      Event::PrivateMessageReceived(message) => Some(message),
      _ => None,
    })
    .await;
  assert_eq!(message.id, 41);
  assert_eq!(message.username, "bob");
  assert_eq!(message.message, "hello");
  assert!(message.replayed);
  assert_eq!(
    message.timestamp,
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
  );
  assert_eq!(
    session.server.request().await,
    ServerRequest::AcknowledgePrivateMessage(AcknowledgePrivateMessage { id: 41 })
  );
}

#[tokio::test]
async fn private_message_is_not_acknowledged_when_turned_off() {
  let mut session = Session::with_options(ClientOptions {
    auto_acknowledge_private_messages: false,
    ..options()
  })
  .await;
  session
    .server
    .send(&ServerResponse::PrivateMessage(PrivateMessageResponse {
      id: 41,
      timestamp: 0,
      username: "bob".into(),
      message: "hello".into(),
      is_new: true,
    }))
    .await;
  let message = session
    .event(|event| match event {
      Event::PrivateMessageReceived(message) => Some(message),
      _ => None,
    })
    .await;
  assert!(!message.replayed);
  let marker = ServerRequest::SetOnlineStatus(SetOnlineStatus {
    status: UserPresence::Online,
  });
  session.client.write_server_message(&marker).await.unwrap();
  assert_eq!(session.server.request().await, marker);
}

#[tokio::test]
async fn file_search_from_another_user_is_handed_on_and_our_own_is_dropped() {
  let mut session = Session::start().await;
  let own = FileSearchResponse {
    username: USERNAME.into(),
    token: 1,
    query: "ours".into(),
  };
  session.server.send(&ServerResponse::FileSearch(own)).await;
  let theirs = FileSearchResponse {
    username: "bob".into(),
    token: 2,
    query: "theirs".into(),
  };
  let handoff = session
    .handoff(ServerResponse::FileSearch(theirs.clone()))
    .await;
  assert!(matches!(handoff, ServerHandoff::FileSearch(got) if got == theirs));
}

#[tokio::test]
async fn ping_completes_its_wait_and_an_unsolicited_one_is_dropped() {
  let mut session = Session::start().await;
  session.server.send(&ServerResponse::Ping).await;
  session.settle().await;
  let () = session
    .reply(WaitKey::Server(ServerCode::Ping), ServerResponse::Ping)
    .await;
}

#[tokio::test]
async fn get_user_stats_completes_and_always_raises_the_statistics() {
  let mut session = Session::start().await;
  let statistics = UserStatistics {
    username: "bob".into(),
    stats: stats(),
  };
  let got: UserStatistics = session
    .reply(
      user_key(ServerCode::GetUserStats, "bob"),
      ServerResponse::GetUserStats(statistics.clone()),
    )
    .await;
  assert_eq!(got, statistics);
  let raised = session
    .event(|event| match event {
      Event::UserStatisticsChanged(statistics) => Some(statistics),
      _ => None,
    })
    .await;
  assert_eq!(raised, statistics);
}

fn recommendations(unrecommended: bool) -> Recommendations {
  Recommendations {
    recommendations: vec![Recommendation {
      interest: "jazz".into(),
      count: 4,
    }],
    unrecommendations: if unrecommended {
      vec![Recommendation {
        interest: "noise".into(),
        count: -2,
      }]
    } else {
      Vec::new()
    },
  }
}

/// a recommendations frame without the optional second block
fn recommendations_without_second_block(code: u32) -> Vec<u8> {
  let mut payload = code.to_le_bytes().to_vec();
  payload.extend(1u32.to_le_bytes());
  payload.extend(4u32.to_le_bytes());
  payload.extend(b"jazz");
  payload.extend(4i32.to_le_bytes());
  let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
  frame.extend(payload);
  frame
}

#[tokio::test]
async fn personal_and_global_recommendations_with_and_without_the_second_block() {
  let mut session = Session::start().await;
  let got: Recommendations = session
    .reply(
      WaitKey::Server(ServerCode::GetPersonalRecommendations),
      ServerResponse::GetPersonalRecommendations(recommendations(true)),
    )
    .await;
  assert_eq!(got, recommendations(true));
  let got: Recommendations = session
    .reply(
      WaitKey::Server(ServerCode::GetGlobalRecommendations),
      ServerResponse::GetGlobalRecommendations(recommendations(true)),
    )
    .await;
  assert_eq!(got, recommendations(true));

  let wait = session.client.waiter().wait::<Recommendations>(
    WaitKey::Server(ServerCode::GetGlobalRecommendations),
    None,
    CancellationToken::new(),
  );
  session
    .server
    .send_bytes(&recommendations_without_second_block(56))
    .await;
  assert_eq!(wait.await.unwrap(), recommendations(false));
}

#[tokio::test]
async fn get_user_interests_completes_for_the_user() {
  let mut session = Session::start().await;
  let interests = UserInterests {
    username: "bob".into(),
    likes: vec!["jazz".into()],
    hates: vec!["noise".into()],
  };
  let got: UserInterests = session
    .reply(
      user_key(ServerCode::GetUserInterests, "bob"),
      ServerResponse::GetUserInterests(interests.clone()),
    )
    .await;
  assert_eq!(got, interests);
}

#[tokio::test]
async fn room_list_completes_and_is_raised() {
  let mut session = Session::start().await;
  let list = RoomList {
    public: vec![RoomInfo {
      name: "indie".into(),
      user_count: 12,
    }],
    owned: Vec::new(),
    private: vec![RoomInfo {
      name: "secret".into(),
      user_count: 3,
    }],
    moderated_room_names: vec!["secret".into()],
  };
  let got: Arc<RoomList> = session
    .reply(
      WaitKey::Server(ServerCode::RoomList),
      ServerResponse::RoomList(list.clone()),
    )
    .await;
  assert_eq!(*got, list);
  let raised = session
    .event(|event| match event {
      Event::RoomListReceived(list) => Some(list),
      _ => None,
    })
    .await;
  assert_eq!(*raised, list);
}

#[tokio::test]
async fn global_admin_message_is_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::GlobalAdminMessage(GlobalAdminMessage {
      message: "maintenance".into(),
    }))
    .await;
  session
    .event(|event| {
      matches!(event, Event::GlobalMessageReceived(message) if message == "maintenance")
        .then_some(())
    })
    .await;
}

#[tokio::test]
async fn privileged_users_are_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivilegedUsers(PrivilegedUsers {
      usernames: vec!["bob".into(), "carol".into()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivilegedUserListReceived(users) => Some(users),
      _ => None,
    })
    .await;
  assert_eq!(raised, ["bob", "carol"]);
}

#[tokio::test]
async fn tuning_values_merge_into_the_server_info() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::ParentMinSpeed(ParentMinSpeed { speed: 1 }))
    .await;
  session
    .server
    .send(&ServerResponse::ParentSpeedRatio(ParentSpeedRatio {
      ratio: 50,
    }))
    .await;
  session
    .server
    .send(&ServerResponse::WishlistInterval(WishlistInterval {
      seconds: 720,
    }))
    .await;
  let merged = session
    .event(|event| match event {
      Event::ServerInfoReceived(info) if info.wishlist_interval.is_some() => Some(info),
      _ => None,
    })
    .await;
  let expected = ServerInfo {
    parent_min_speed: Some(1),
    parent_speed_ratio: Some(50),
    wishlist_interval: Some(720),
    is_supporter: Some(true),
    own_public_ip: Some(support::PUBLIC_IP),
    ..ServerInfo::default()
  };
  assert_eq!(merged, expected);
  assert_eq!(session.client.server_info(), expected);
}

#[tokio::test]
async fn obsolete_tuning_values_are_stored_and_not_used() {
  let logs = Logs::capture();
  let mut session = Session::start().await;
  for response in [
    ServerResponse::ParentInactivityTimeout(ParentInactivityTimeout { seconds: 300 }),
    ServerResponse::SearchInactivityTimeout(SearchInactivityTimeout { seconds: 30 }),
    ServerResponse::MinimumParentsInCache(MinimumParentsInCache { count: 10 }),
    ServerResponse::DistributedAliveInterval(DistributedAliveInterval { seconds: 60 }),
  ] {
    session.server.send(&response).await;
  }
  session.settle().await;
  let info = session.client.server_info();
  assert_eq!(info.parent_inactivity_timeout, Some(300));
  assert_eq!(info.search_inactivity_timeout, Some(30));
  assert_eq!(info.minimum_parents_in_cache, Some(10));
  assert_eq!(info.distributed_alive_interval, Some(60));
  assert_eq!(
    logs.lines_with(&["obsolete tuning value", "code=86"]).len(),
    1
  );
}

#[tokio::test]
async fn add_privileged_user_is_a_notification_without_acknowledgement() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::AddPrivilegedUser(AddPrivilegedUser {
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivilegeNotificationReceived {
        username,
        id,
        requires_acknowledgement,
      } => Some((username, id, requires_acknowledgement)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("bob".into(), None, false));
}

#[tokio::test]
async fn check_privileges_completes_with_seconds() {
  let mut session = Session::start().await;
  let got: u32 = session
    .reply(
      WaitKey::Server(ServerCode::CheckPrivileges),
      ServerResponse::CheckPrivileges(CheckPrivilegesResponse {
        seconds_remaining: 86_400,
      }),
    )
    .await;
  assert_eq!(got, 86_400);
}

#[tokio::test]
async fn embedded_message_is_handed_on() {
  let mut session = Session::start().await;
  let embedded = Embedded {
    distributed_code: 3,
    payload: vec![49, 0, 0, 0, 1, 2, 3],
  };
  let handoff = session
    .handoff(ServerResponse::EmbeddedMessage(embedded.clone()))
    .await;
  assert!(matches!(handoff, ServerHandoff::EmbeddedMessage(got) if got == embedded));
}

#[tokio::test]
async fn net_info_is_handed_on() {
  let mut session = Session::start().await;
  let candidates = NetInfo {
    parents: vec![ParentCandidate {
      username: "bob".into(),
      ip: Ipv4Addr::new(10, 0, 0, 2),
      port: 2234,
    }],
  };
  let handoff = session
    .handoff(ServerResponse::NetInfo(candidates.clone()))
    .await;
  assert!(matches!(handoff, ServerHandoff::NetInfo(got) if got == candidates));
}

#[tokio::test]
async fn similar_users_complete() {
  let mut session = Session::start().await;
  let users = GetPersonalSimilarUsersResponse {
    users: vec![SimilarUser {
      username: "bob".into(),
      rating: 3,
    }],
  };
  let got: GetPersonalSimilarUsersResponse = session
    .reply(
      WaitKey::Server(ServerCode::GetPersonalSimilarUsers),
      ServerResponse::GetPersonalSimilarUsers(users.clone()),
    )
    .await;
  assert_eq!(got, users);
}

#[tokio::test]
async fn interest_recommendations_and_similar_users_complete_for_the_interest() {
  let mut session = Session::start().await;
  let interest_key = |code| WaitKey::ServerInterest {
    code,
    interest: "jazz".into(),
  };
  let recommended = GetInterestRecommendationsResponse {
    interest: "jazz".into(),
    recommendations: recommendations(true),
  };
  let got: GetInterestRecommendationsResponse = session
    .reply(
      interest_key(ServerCode::GetInterestRecommendations),
      ServerResponse::GetInterestRecommendations(recommended.clone()),
    )
    .await;
  assert_eq!(got, recommended);
  let similar = GetInterestSimilarUsersResponse {
    interest: "jazz".into(),
    usernames: vec!["bob".into()],
  };
  let got: GetInterestSimilarUsersResponse = session
    .reply(
      interest_key(ServerCode::GetInterestSimilarUsers),
      ServerResponse::GetInterestSimilarUsers(similar.clone()),
    )
    .await;
  assert_eq!(got, similar);
}

#[tokio::test]
async fn room_tickers_are_raised() {
  let mut session = Session::start().await;
  let ticker = RoomTicker {
    username: "bob".into(),
    message: "listening".into(),
  };
  session
    .server
    .send(&ServerResponse::RoomTickers(RoomTickers {
      room: "indie".into(),
      tickers: vec![ticker.clone()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomTickerListReceived { room_name, tickers } => Some((room_name, tickers)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), vec![ticker.clone()]));

  session
    .server
    .send(&ServerResponse::RoomTickerAdd(RoomTickerAdd {
      room: "indie".into(),
      ticker: ticker.clone(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomTickerAdded { room_name, ticker } => Some((room_name, ticker)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), ticker));

  session
    .server
    .send(&ServerResponse::RoomTickerRemove(RoomTickerRemove {
      room: "indie".into(),
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomTickerRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into()));
}

#[tokio::test]
async fn user_privileges_completes_with_the_flag() {
  let mut session = Session::start().await;
  let got: bool = session
    .reply(
      user_key(ServerCode::UserPrivileges, "bob"),
      ServerResponse::UserPrivileges(UserPrivilegesResponse {
        username: "bob".into(),
        privileged: true,
      }),
    )
    .await;
  assert!(got);
}

#[tokio::test]
async fn notify_privileges_is_raised_and_acknowledged() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::NotifyPrivileges(NotifyPrivileges {
      id: 9,
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivilegeNotificationReceived {
        username,
        id,
        requires_acknowledgement,
      } => Some((username, id, requires_acknowledgement)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("bob".into(), Some(9), true));
  assert_eq!(
    session.server.request().await,
    ServerRequest::AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges { id: 9 })
  );
}

#[tokio::test]
async fn distributed_reset_is_raised_and_handed_on() {
  let mut session = Session::start().await;
  let handoff = session.handoff(ServerResponse::DistributedReset).await;
  assert!(matches!(handoff, ServerHandoff::DistributedReset));
  session
    .event(|event| matches!(event, Event::DistributedNetworkReset).then_some(()))
    .await;
}

#[tokio::test]
async fn private_room_member_and_operator_lists_are_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateRoomUsers(PrivateRoomUsers {
      room: "secret".into(),
      usernames: vec!["carol".into()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomUserListReceived(members) => Some(members),
      _ => None,
    })
    .await;
  assert_eq!(
    raised,
    RoomMembers {
      room_name: "secret".into(),
      users: vec!["carol".into()]
    }
  );
  session
    .server
    .send(&ServerResponse::PrivateRoomOwned(PrivateRoomOwned {
      room: "secret".into(),
      operators: vec!["dave".into()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomModeratedUserListReceived(members) => Some(members),
      _ => None,
    })
    .await;
  assert_eq!(
    raised,
    RoomMembers {
      room_name: "secret".into(),
      users: vec!["dave".into()]
    }
  );
}

#[tokio::test]
async fn private_room_member_changes_complete_and_are_raised() {
  let mut session = Session::start().await;
  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomAddUser, "secret", "carol"),
      ServerResponse::PrivateRoomAddUser(PrivateRoomAddUser {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomUserAdded {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));

  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomRemoveUser, "secret", "carol"),
      ServerResponse::PrivateRoomRemoveUser(PrivateRoomRemoveUser {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomUserRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));
}

#[tokio::test]
async fn private_room_operator_changes_complete_and_are_raised() {
  let mut session = Session::start().await;
  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomAddOperator, "secret", "carol"),
      ServerResponse::PrivateRoomAddOperator(PrivateRoomAddOperator {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomOperatorAdded {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));

  let () = session
    .reply(
      room_user_key(ServerCode::PrivateRoomRemoveOperator, "secret", "carol"),
      ServerResponse::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator {
        room: "secret".into(),
        username: "carol".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::PrivateRoomOperatorRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("secret".into(), "carol".into()));
}

#[tokio::test]
async fn our_private_room_membership_is_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateRoomAdded(PrivateRoomAdded {
      room: "secret".into(),
    }))
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomMembershipAdded(room) if room == "secret").then_some(())
    })
    .await;
  let () = session
    .reply(
      room_key(ServerCode::PrivateRoomRemoved, "secret"),
      ServerResponse::PrivateRoomRemoved(PrivateRoomRemoved {
        room: "secret".into(),
      }),
    )
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomMembershipRemoved(room) if room == "secret").then_some(())
    })
    .await;
}

#[tokio::test]
async fn our_private_room_moderation_is_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PrivateRoomOperatorAdded(
      PrivateRoomOperatorAdded {
        room: "secret".into(),
      },
    ))
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomModerationAdded(room) if room == "secret").then_some(())
    })
    .await;
  let () = session
    .reply(
      room_key(ServerCode::PrivateRoomOperatorRemoved, "secret"),
      ServerResponse::PrivateRoomOperatorRemoved(PrivateRoomOperatorRemoved {
        room: "secret".into(),
      }),
    )
    .await;
  session
    .event(|event| {
      matches!(event, Event::PrivateRoomModerationRemoved(room) if room == "secret").then_some(())
    })
    .await;
}

#[tokio::test]
async fn private_room_toggle_completes_with_the_setting() {
  let mut session = Session::start().await;
  let got: bool = session
    .reply(
      WaitKey::Server(ServerCode::PrivateRoomToggle),
      ServerResponse::PrivateRoomToggle(PrivateRoomToggle {
        accept_invitations: true,
      }),
    )
    .await;
  assert!(got);
}

#[tokio::test]
async fn new_password_completes_with_the_echo() {
  let mut session = Session::start().await;
  let got: Secret = session
    .reply(
      WaitKey::Server(ServerCode::NewPassword),
      ServerResponse::NewPassword(NewPassword {
        new_password: Secret::new("n3w"),
      }),
    )
    .await;
  assert_eq!(got.expose(), "n3w");
}

#[tokio::test]
async fn public_chat_is_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::PublicChat(PublicChat {
      room: "indie".into(),
      username: "bob".into(),
      message: "hi all".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::PublicChatMessageReceived {
        room_name,
        username,
        message,
      } => Some((room_name, username, message)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into(), "hi all".into()));
}

#[tokio::test]
async fn excluded_search_phrases_are_raised() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::ExcludedSearchPhrases(
      ExcludedSearchPhrases {
        phrases: vec!["bad phrase".into()],
      },
    ))
    .await;
  let raised = session
    .event(|event| match event {
      Event::ExcludedSearchPhrasesReceived(phrases) => Some(phrases),
      _ => None,
    })
    .await;
  assert_eq!(raised, ["bad phrase"]);
}

#[tokio::test]
async fn cannot_connect_is_handed_on_and_raised_only_with_a_username() {
  let mut session = Session::start().await;
  let token_only = CannotConnectResponse {
    token: 12,
    username: None,
  };
  let handoff = session
    .handoff(ServerResponse::CannotConnect(token_only.clone()))
    .await;
  assert!(matches!(handoff, ServerHandoff::CannotConnect(got) if got == token_only));

  let named = CannotConnectResponse {
    token: 13,
    username: Some("bob".into()),
  };
  let handoff = session
    .handoff(ServerResponse::CannotConnect(named.clone()))
    .await;
  assert!(matches!(handoff, ServerHandoff::CannotConnect(got) if got == named));
  let raised = session
    .event(|event| match event {
      Event::UserCannotConnect { token, username } => Some((token, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, (13, "bob".into()), "only the named one is raised");
}

#[tokio::test]
async fn cannot_join_room_fails_the_join() {
  let mut session = Session::start().await;
  let wait = session.client.waiter().wait::<RoomData>(
    room_key(ServerCode::JoinRoom, "secret"),
    None,
    CancellationToken::new(),
  );
  session
    .server
    .send(&ServerResponse::CannotJoinRoom(CannotJoinRoom {
      room: "secret".into(),
    }))
    .await;
  let error = tokio::time::timeout(PATIENCE, wait)
    .await
    .unwrap()
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::Room { kind: RoomErrorKind::JoinForbidden, message } if message == "A private room with this name already exists"),
    "{error:?}"
  );
}

#[tokio::test]
async fn unhandled_and_obsolete_codes_are_logged_and_the_connection_stays_up() {
  let logs = Logs::capture();
  let mut session = Session::start().await;
  // a code the server echoes that nothing handles, a code nobody knows, an obsolete code with a
  // decoder and one without
  session
    .server
    .send_bytes(&[8, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0])
    .await;
  session
    .server
    .send_bytes(&[4, 0, 0, 0, 0x0f, 0x27, 0, 0])
    .await;
  session
    .server
    .send(&ServerResponse::IgnoreUser(IgnoreUser {
      username: "bob".into(),
    }))
    .await;
  session.server.send_bytes(&[4, 0, 0, 0, 58, 0, 0, 0]).await;
  session.settle().await;
  assert_eq!(
    logs
      .lines_with(&["unhandled server message", "code=2 "])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&["unhandled server message", "code=9999"])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&["obsolete server message", "code=11", "IgnoreUser"])
      .len(),
    1
  );
  assert_eq!(
    logs
      .lines_with(&["obsolete server message", "code=58", "AdminCommand"])
      .len(),
    1
  );
  assert_eq!(session.client.state(), soulseek_client::ClientState::READY);
}

#[tokio::test]
async fn a_malformed_message_is_dropped_and_the_connection_stays_up() {
  let mut session = Session::start().await;
  // GetStatus with a presence of 9
  let mut payload = 7u32.to_le_bytes().to_vec();
  payload.extend(3u32.to_le_bytes());
  payload.extend(b"bob");
  payload.extend(9u32.to_le_bytes());
  payload.push(0);
  let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
  frame.extend(payload);
  session.server.send_bytes(&frame).await;
  session.settle().await;
  assert_eq!(session.client.state(), soulseek_client::ClientState::READY);
}

#[tokio::test]
async fn unread_bytes_are_reported_when_asked_for() {
  let logs = Logs::capture();
  let mut session = Session::with_options(ClientOptions {
    report_unread_message_data: true,
    ..options()
  })
  .await;
  let mut frame = ServerResponse::GlobalAdminMessage(GlobalAdminMessage {
    message: "with a tail".into(),
  })
  .encode();
  frame.extend([1, 2, 3]);
  let len = (frame.len() - 4) as u32;
  frame[..4].copy_from_slice(&len.to_le_bytes());
  session.server.send_bytes(&frame).await;
  session.settle().await;
  assert_eq!(
    logs
      .lines_with(&["bytes past its fields", "code=66", "unread=3"])
      .len(),
    1
  );
}
