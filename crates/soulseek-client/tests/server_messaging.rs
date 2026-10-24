//! Private messages, admin broadcasts and the public chat feed, delivered by a fake server to a
//! logged-in client, with the acknowledgements the client sends back.

mod support;

use std::time::{Duration, UNIX_EPOCH};

use soulseek_client::{ClientOptions, Event};
use soulseek_proto::server::*;
use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};
use support::{Session, options};
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
