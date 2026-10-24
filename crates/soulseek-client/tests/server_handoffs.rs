//! Server messages about peer connections, searches and the distributed tree, delivered by a fake
//! server to a logged-in client: the ones handed on to the components that own them, and the
//! tuning values merged into the server info.

mod support;

use std::net::Ipv4Addr;

use soulseek_client::{Event, ServerHandoff, ServerInfo};
use soulseek_proto::distributed::Embedded;
use soulseek_proto::server::*;
use soulseek_proto::{PeerConnectionType, ServerResponse};
use support::{Logs, Session, USERNAME};
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
async fn distributed_reset_is_raised_and_handed_on() {
  let mut session = Session::start().await;
  let handoff = session.handoff(ServerResponse::DistributedReset).await;
  assert!(matches!(handoff, ServerHandoff::DistributedReset));
  session
    .event(|event| matches!(event, Event::DistributedNetworkReset).then_some(()))
    .await;
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
