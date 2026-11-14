//! Server messages soulfind never sends, delivered by a scripted server: privilege notifications,
//! the distributed tree's tuning values, the shapes older or newer servers send, codes nothing
//! handles, malformed frames and unread bytes, and statuses timed against our requests for them.

use crate::support::{Logs, Session, options};
use soulseek_client::{ClientOptions, Event, ServerInfo};
use soulseek_proto::server::*;
use soulseek_proto::{ServerRequest, ServerResponse};

#[tokio::test]
async fn privilege_notifications_are_raised_and_only_notify_privileges_is_acknowledged() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::AddPrivilegedUser(AddPrivilegedUser {
      username: "bob".into(),
    }))
    .await;
  session
    .server
    .send(&ServerResponse::NotifyPrivileges(NotifyPrivileges {
      id: 9,
      username: "carol".into(),
    }))
    .await;
  for expected in [("bob".into(), None, false), ("carol".into(), Some(9), true)] {
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
    assert_eq!(raised, expected);
  }
  // the first request after both is the acknowledgment, so the added user got none
  assert_eq!(
    session.server.request().await,
    ServerRequest::AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges { id: 9 })
  );
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
    own_public_ip: Some(crate::support::PUBLIC_IP),
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
  assert_eq!(
    session.client.state(),
    soulseek_client::ClientState::LoggedIn
  );
}

#[tokio::test]
async fn a_malformed_message_is_dropped_with_one_warning_a_minute_and_the_connection_stays_up() {
  let logs = Logs::capture();
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
  session.server.send_bytes(&frame).await;
  session.settle().await;
  assert_eq!(
    session.client.state(),
    soulseek_client::ClientState::LoggedIn
  );
  let warned = logs.lines_with(&["WARN", "server message not read", "GetStatus"]);
  assert_eq!(warned.len(), 1, "{}", logs.text());
  let rejected = logs.lines_with(&["DEBUG", "message rejected", "code=7 "]);
  assert_eq!(rejected.len(), 2, "the decoder tells each: {}", logs.text());
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

/// bob's status as the server sends it, asked for or not
fn bob_is(presence: soulseek_proto::UserPresence) -> ServerResponse {
  ServerResponse::GetStatus(UserStatus {
    username: "bob".into(),
    presence,
    is_privileged: false,
  })
}

/// The presence of the next status of bob `session` raises, and whether it was asked for.
async fn bob_raised(session: &mut Session) -> (soulseek_proto::UserPresence, bool) {
  session
    .event(|event| match event {
      Event::UserStatusChanged { status, asked } if status.username == "bob" => {
        Some((status.presence, asked))
      }
      _ => None,
    })
    .await
}

fn asks_for_bob(request: ServerRequest) -> Option<()> {
  matches!(request, ServerRequest::GetStatus(asked) if asked.username == "bob").then_some(())
}

#[tokio::test]
async fn a_status_is_raised_as_asked_only_while_a_request_for_it_waits() {
  use soulseek_proto::UserPresence;

  let mut session = Session::start().await;
  let client = session.client.clone();
  let mut asking = Box::pin(client.get_user_status("bob"));
  tokio::select! {
    biased;
    _ = &mut asking => panic!("answered before the server was asked"),
    () = session.server.request_matching(asks_for_bob) => {}
  }
  session.server.send(&bob_is(UserPresence::Online)).await;
  assert_eq!(bob_raised(&mut session).await, (UserPresence::Online, true));
  assert_eq!(asking.await.unwrap().presence, UserPresence::Online);

  // a request given up stops waiting as it is dropped, so what comes next is pushed
  let mut given_up = Box::pin(client.get_user_status("bob"));
  tokio::select! {
    biased;
    _ = &mut given_up => panic!("answered before the server was asked"),
    () = session.server.request_matching(asks_for_bob) => {}
  }
  drop(given_up);
  session.server.send(&bob_is(UserPresence::Away)).await;
  assert_eq!(bob_raised(&mut session).await, (UserPresence::Away, false));
}

#[tokio::test]
async fn of_two_status_requests_one_dropped_after_its_reply_leaves_the_others_reply_asked() {
  use soulseek_proto::UserPresence;

  let mut session = Session::start().await;
  let client = session.client.clone();
  let mut first = Box::pin(client.get_user_status("bob"));
  let mut second = Box::pin(client.get_user_status("bob"));
  for asking in [&mut first, &mut second] {
    tokio::select! {
      biased;
      _ = asking => panic!("answered before the server was asked"),
      () = session.server.request_matching(asks_for_bob) => {}
    }
  }
  // the first request's reply comes and is raised before the request is polled again
  session.server.send(&bob_is(UserPresence::Online)).await;
  assert_eq!(bob_raised(&mut session).await, (UserPresence::Online, true));
  drop(first);
  session.server.send(&bob_is(UserPresence::Online)).await;
  assert_eq!(
    bob_raised(&mut session).await,
    (UserPresence::Online, true),
    "the second request's reply is raised as pushed"
  );
  assert_eq!(second.await.unwrap().presence, UserPresence::Online);
}
