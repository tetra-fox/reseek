//! Server messages about other users, delivered by a fake server to a logged-in client: their
//! address, watch replies, status and statistics.

mod support;

use std::net::Ipv4Addr;

use soulseek_client::Event;
use soulseek_proto::server::*;
use soulseek_proto::{ServerCode, ServerResponse, UserPresence};
use support::{Session, stats, user_key};
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
