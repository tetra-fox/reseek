//! Other users on soulfind: their address, watching them, their status as asked for and as
//! pushed, and their statistics as asked for and as pushed to members of a shared room.

mod support;

use soulseek_client::{ClientOptions, Event};
use soulseek_proto::server::*;
use soulseek_proto::{RoomType, ServerCode, ServerRequest, UserPresence};
use support::soulfind::{self, User};
use support::{free_port, options, room_key, user_key};

async fn peer_address(asker: &User, username: &str) -> GetPeerAddressResponse {
  asker
    .ask(
      user_key(ServerCode::GetPeerAddress, username),
      ServerRequest::GetPeerAddress(GetPeerAddressRequest {
        username: username.into(),
      }),
    )
    .await
}

#[tokio::test]
async fn the_address_of_an_online_user_and_of_an_unknown_one() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let port = free_port();
  let bob = server
    .login(
      "bob",
      ClientOptions {
        listen_port: port,
        ..options()
      },
    )
    .await;
  let alice = server.login("alice", options()).await;
  let address = peer_address(&alice, &bob.username).await;
  assert_eq!(address.username, bob.username);
  assert_eq!(Some(address.ip), bob.client.server_info().own_public_ip);
  assert_eq!(address.port, u32::from(port));
  // soulfind sends the obfuscation trailer, empty
  assert_eq!(
    address.obfuscation,
    Some(PeerAddressObfuscation {
      obfuscation_type: 0,
      obfuscated_port: 0
    })
  );
  let unknown = peer_address(&alice, &soulfind::username("nobody")).await;
  assert_eq!(unknown.ip, std::net::Ipv4Addr::UNSPECIFIED);
  assert_eq!(unknown.port, 0);
}

#[tokio::test]
async fn watching_an_online_user_and_a_missing_one() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let bob = server.login("bob", options()).await;
  let alice = server.login("alice", options()).await;
  let watched: WatchUserResponse = alice
    .ask(
      user_key(ServerCode::WatchUser, &bob.username),
      ServerRequest::WatchUser(WatchUserRequest {
        username: bob.username.clone(),
      }),
    )
    .await;
  let user = watched.user.expect("bob exists");
  assert_eq!(user.status, UserPresence::Online);
  assert!(user.country_code.is_some(), "present for an online user");

  let missing = soulfind::username("missing");
  let watched: WatchUserResponse = alice
    .ask(
      user_key(ServerCode::WatchUser, &missing),
      ServerRequest::WatchUser(WatchUserRequest {
        username: missing.clone(),
      }),
    )
    .await;
  assert_eq!(watched.username, missing);
  assert_eq!(watched.user, None);
}

#[tokio::test]
async fn status_is_raised_when_asked_for_and_when_a_watched_user_goes_away() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let bob = server.login("bob", options()).await;
  let mut alice = server.login("alice", options()).await;
  let status: UserStatus = alice
    .ask(
      user_key(ServerCode::GetStatus, &bob.username),
      ServerRequest::GetStatus(GetStatusRequest {
        username: bob.username.clone(),
      }),
    )
    .await;
  let online = UserStatus {
    username: bob.username.clone(),
    presence: UserPresence::Online,
    is_privileged: false,
  };
  assert_eq!(status, online);
  let raised = alice
    .event(|event| match event {
      Event::UserStatusChanged(status) if status.username == online.username => Some(status),
      _ => None,
    })
    .await;
  assert_eq!(raised, online);

  let _: WatchUserResponse = alice
    .ask(
      user_key(ServerCode::WatchUser, &bob.username),
      ServerRequest::WatchUser(WatchUserRequest {
        username: bob.username.clone(),
      }),
    )
    .await;
  bob
    .send(ServerRequest::SetOnlineStatus(SetOnlineStatus {
      status: UserPresence::Away,
    }))
    .await;
  let pushed = alice
    .event(|event| match event {
      Event::UserStatusChanged(status) if status.username == online.username => Some(status),
      _ => None,
    })
    .await;
  assert_eq!(pushed.presence, UserPresence::Away);
}

#[tokio::test]
async fn statistics_are_raised_when_asked_for_and_pushed_to_room_members() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("stats");
  let bob = server.login("bob", options()).await;
  let mut alice = server.login("alice", options()).await;
  let statistics: UserStatistics = alice
    .ask(
      user_key(ServerCode::GetUserStats, &bob.username),
      ServerRequest::GetUserStats(GetUserStatsRequest {
        username: bob.username.clone(),
      }),
    )
    .await;
  assert_eq!(statistics.username, bob.username);
  let raised = alice
    .event(|event| match event {
      Event::UserStatisticsChanged(raised) if raised.username == statistics.username => {
        Some(raised)
      }
      _ => None,
    })
    .await;
  assert_eq!(raised, statistics);

  for user in [&alice, &bob] {
    let _: RoomData = user
      .ask(
        room_key(ServerCode::JoinRoom, &room),
        ServerRequest::JoinRoom(JoinRoomRequest {
          room: room.clone(),
          room_type: RoomType::Public,
        }),
      )
      .await;
  }
  bob
    .send(ServerRequest::SharedFoldersAndFiles(
      SharedFoldersAndFiles {
        directory_count: 3,
        file_count: 30,
      },
    ))
    .await;
  let pushed = alice
    .event(|event| match event {
      Event::UserStatisticsChanged(pushed)
        if pushed.username == bob.username && pushed.stats.file_count == 30 =>
      {
        Some(pushed)
      }
      _ => None,
    })
    .await;
  assert_eq!(pushed.stats.directory_count, 3);
}
