//! Server messages soulfind never sends, delivered by a scripted server: privilege notifications,
//! the distributed tree's tuning values, the shapes older or newer servers send, codes nothing
//! handles, malformed frames and unread bytes.

mod support;

use soulseek_client::{ClientOptions, Event, ServerInfo, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{ServerCode, ServerRequest, ServerResponse};
use support::{Logs, Session, options};

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
async fn add_privileged_user_is_a_notification_without_acknowledgment() {
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
        requires_acknowledgment,
      } => Some((username, id, requires_acknowledgment)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("bob".into(), None, false));
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
        requires_acknowledgment,
      } => Some((username, id, requires_acknowledgment)),
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
async fn cannot_connect_naming_the_user_is_raised() {
  let mut session = Session::start().await;
  let named = CannotConnectResponse {
    token: 13,
    username: Some("bob".into()),
  };
  session
    .server
    .send(&ServerResponse::CannotConnect(named))
    .await;
  let raised = session
    .event(|event| match event {
      Event::UserCannotConnect { token, username } => Some((token, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, (13, "bob".into()));
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

/// soulfind drops a private message holding a line break without telling either side, so the
/// line breaks are carried here, both ways, by a server that does not
#[tokio::test]
async fn a_private_message_keeps_its_line_breaks() {
  let mut session = Session::with_options(ClientOptions {
    auto_acknowledge_private_messages: false,
    ..options()
  })
  .await;
  session
    .client
    .send_private_message("bob", "one\ntwo\r\nthree")
    .await
    .unwrap();
  assert_eq!(
    session.server.request().await,
    ServerRequest::PrivateMessage(PrivateMessageRequest {
      username: "bob".into(),
      message: "one\ntwo\r\nthree".into(),
    })
  );
  session
    .server
    .send(&ServerResponse::PrivateMessage(PrivateMessageResponse {
      id: 4,
      timestamp: 1_700_000_000,
      username: "bob".into(),
      message: "four\nfive".into(),
      is_new: true,
    }))
    .await;
  let received = session
    .event(|event| match event {
      Event::PrivateMessageReceived(message) => Some(message),
      _ => None,
    })
    .await;
  assert_eq!(received.message, "four\nfive");
}
