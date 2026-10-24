//! Children below us in the distributed tree, against a scripted server and scripted children:
//! a search straight from the server makes us a branch root that takes children; each child
//! hears our branch, the searches we pass down and an answer to its ping; the child limit,
//! lowered by our upload speed, is never overshot even by children arriving together; a child
//! replaced under the same name, or one that leaves, is accounted for exactly once.

mod support;

use std::time::Duration;

use soulseek_client::{ClientOptions, Event};
use soulseek_proto::distributed::Ping;
use soulseek_proto::frame::FrameKind;
use soulseek_proto::server::{
  ConnectToPeerResponse, ParentMinSpeed, ParentSpeedRatio, UserStatistics, UserStats,
};
use soulseek_proto::{
  DistributedMessage, InitMessage, PeerConnectionType, ServerRequest, ServerResponse,
};
use support::peer::{FakePeer, PeerSide};
use support::tree::{branch_information, embedded_search, join_as_child, search, status};
use support::{Session, USERNAME, listening_options};

/// a client the server made a branch root, so it takes up to `limit` children
async fn branch_root(limit: u32) -> Session {
  let mut session = Session::with_options(ClientOptions {
    distributed_child_limit: limit,
    ..listening_options()
  })
  .await;
  session
    .server
    .send(&embedded_search("someone", 1, "first"))
    .await;
  session
    .event(|event| matches!(event, Event::PromotedToDistributedBranchRoot).then_some(()))
    .await;
  session
}

async fn child(session: &Session, username: &str) -> PeerSide {
  let mut child = join_as_child(session.listen_address(), username).await;
  assert_eq!(branch_information(&mut child).await, (0, USERNAME.into()));
  child
}

async fn added(session: &mut Session) -> String {
  session
    .event(|event| match event {
      Event::DistributedChildAdded { username, .. } => Some(username),
      _ => None,
    })
    .await
}

#[tokio::test]
async fn a_branch_root_takes_children_and_tells_the_server() {
  let mut session = branch_root(25).await;
  let info = session.client.distributed_network();
  assert!(info.is_branch_root);
  assert!(info.can_accept_children);
  let mut first = child(&session, "first").await;
  assert_eq!(added(&mut session).await, "first");
  // the server hears about the new branch and child once the changes settle
  tokio::time::pause();
  tokio::time::advance(Duration::from_secs(6)).await;
  tokio::time::resume();
  let reported = status(&mut session.server).await;
  assert!(reported.accepts_children);
  assert_eq!((reported.level, reported.root.as_str()), (0, USERNAME));
  // searches from the server go down unwrapped
  session
    .server
    .send(&embedded_search("bob", 2, "jazz"))
    .await;
  assert_eq!(
    first.frame(FrameKind::Distributed).await,
    search("bob", 2, "jazz")
  );
  // a ping is answered with an empty one
  first
    .send(&DistributedMessage::Ping(Ping { token: None }).encode())
    .await;
  assert_eq!(
    first.read_distributed().await,
    DistributedMessage::Ping(Ping { token: None })
  );
  assert_eq!(session.client.distributed_network().children[0].0, "first");
}

#[tokio::test]
async fn children_past_the_limit_are_refused() {
  let mut session = branch_root(2).await;
  let _first = child(&session, "first").await;
  let _second = child(&session, "second").await;
  added(&mut session).await;
  added(&mut session).await;
  let mut third = join_as_child(session.listen_address(), "third").await;
  third.closed().await;
  let info = session.client.distributed_network();
  assert_eq!(info.children.len(), 2);
  assert!(!info.can_accept_children);
}

#[tokio::test]
async fn children_arriving_together_never_overshoot_the_limit() {
  let mut session = branch_root(2).await;
  let peers = [
    FakePeer::start().await,
    FakePeer::start().await,
    FakePeer::start().await,
  ];
  // three ask through the server at once and one connects directly
  for (n, peer) in peers.iter().enumerate() {
    session
      .server
      .send(&ServerResponse::ConnectToPeer(ConnectToPeerResponse {
        username: format!("relayed{n}"),
        connection_type: PeerConnectionType::Distributed,
        ip: std::net::Ipv4Addr::LOCALHOST,
        port: peer.port(),
        token: 100 + n as u32,
        privileged: false,
        obfuscation: None,
      }))
      .await;
  }
  let mut direct = join_as_child(session.listen_address(), "direct").await;
  // two slots, so of the four exactly two are taken and the others refused
  let refused = session
    .server
    .request_matching(|request| match request {
      ServerRequest::CannotConnect(report) => Some(report.token),
      _ => None,
    })
    .await;
  assert!((100..103).contains(&refused));
  let mut joined = Vec::new();
  for peer in &peers {
    if let Ok(mut connection) =
      tokio::time::timeout(Duration::from_millis(500), peer.accept()).await
    {
      assert!(matches!(
        connection.read_init().await,
        InitMessage::PierceFirewall(_)
      ));
      joined.push(connection);
    }
  }
  // a child that joined hears our branch; a refused one is closed
  let direct_joined = direct
    .frame_or_close(FrameKind::Distributed)
    .await
    .is_some();
  assert_eq!(joined.len() + usize::from(direct_joined), 2);
  assert_eq!(session.client.distributed_network().children.len(), 2);
}

#[tokio::test]
async fn our_upload_speed_lowers_the_limit_and_too_little_of_it_takes_no_children() {
  let mut session = branch_root(25).await;
  session
    .server
    .send(&ServerResponse::ParentSpeedRatio(ParentSpeedRatio {
      ratio: 100,
    }))
    .await;
  let own = |average_speed| {
    ServerResponse::GetUserStats(UserStatistics {
      username: USERNAME.into(),
      stats: UserStats {
        average_speed,
        upload_count: 0,
        file_count: 0,
        directory_count: 0,
      },
    })
  };
  session.server.send(&own(250)).await;
  session
    .event(|event| {
      matches!(event, Event::DistributedNetworkStateChanged(info) if info.child_limit == 2)
        .then_some(())
    })
    .await;
  session
    .server
    .send(&ServerResponse::ParentMinSpeed(ParentMinSpeed {
      speed: 1000,
    }))
    .await;
  session
    .event(|event| {
      matches!(event, Event::DistributedNetworkStateChanged(info) if !info.can_accept_children)
        .then_some(())
    })
    .await;
  let mut refused = join_as_child(session.listen_address(), "slow").await;
  refused.closed().await;
  tokio::time::pause();
  tokio::time::advance(Duration::from_secs(6)).await;
  tokio::time::resume();
  assert!(!status(&mut session.server).await.accepts_children);
}

#[tokio::test]
async fn a_replaced_child_closing_never_takes_its_replacement_with_it() {
  let mut session = branch_root(25).await;
  let mut first = child(&session, "twice").await;
  added(&mut session).await;
  let mut second = child(&session, "twice").await;
  first.closed().await;
  // the replacement still gets the searches passed down
  session.server.send(&embedded_search("bob", 3, "x")).await;
  assert_eq!(
    second.frame(FrameKind::Distributed).await,
    search("bob", 3, "x")
  );
  let children = session.client.distributed_network().children;
  assert_eq!(children.len(), 1);
  assert_eq!(children[0].1, second.stream.local_addr().unwrap());
}

#[tokio::test]
async fn a_child_that_leaves_is_accounted_for() {
  let mut session = branch_root(25).await;
  let leaving = child(&session, "leaving").await;
  added(&mut session).await;
  drop(leaving);
  let gone = session
    .event(|event| match event {
      Event::DistributedChildDisconnected { username, .. } => Some(username),
      _ => None,
    })
    .await;
  assert_eq!(gone, "leaving");
  assert!(session.client.distributed_network().children.is_empty());
}
