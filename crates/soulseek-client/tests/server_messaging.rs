//! Private messages and the public chat feed on soulfind: delivery, the acknowledgement that
//! stops the server replaying a message at every login, and messages kept for an offline user.

mod support;

use soulseek_client::{ClientOptions, Event, PrivateMessage};
use soulseek_proto::server::*;
use soulseek_proto::{RoomType, ServerCode, ServerRequest};
use support::soulfind::{self, PASSWORD, User};
use support::{options, room_key};

fn without_acknowledgement() -> ClientOptions {
  ClientOptions {
    auto_acknowledge_private_messages: false,
    ..options()
  }
}

async fn message(to: &User, from: &User, text: &str) {
  from
    .send(ServerRequest::PrivateMessage(PrivateMessageRequest {
      username: to.username.clone(),
      message: text.into(),
    }))
    .await;
}

async fn received(user: &mut User) -> PrivateMessage {
  user
    .event(|event| match event {
      Event::PrivateMessageReceived(message) => Some(message),
      _ => None,
    })
    .await
}

/// logs `user` out and in again on the same client
async fn relogin(server: &soulfind::Soulfind, user: &User) {
  user.client.disconnect(None, None).await;
  server
    .connect(&user.client, &user.username, PASSWORD)
    .await
    .unwrap();
}

#[tokio::test]
async fn an_unacknowledged_message_is_replayed_at_the_next_login() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", without_acknowledgement()).await;
  message(&bob, &alice, "hello").await;
  let live = received(&mut bob).await;
  assert_eq!(live.username, alice.username);
  assert_eq!(live.message, "hello");
  assert!(!live.replayed);

  relogin(&server, &bob).await;
  let replayed = received(&mut bob).await;
  assert_eq!(replayed.id, live.id);
  assert_eq!(replayed.message, "hello");
  assert!(replayed.replayed);
}

#[tokio::test]
async fn an_acknowledged_message_is_not_replayed() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", options()).await;
  message(&bob, &alice, "first").await;
  assert_eq!(received(&mut bob).await.message, "first");
  // the acknowledgement is queued before this request, so it has reached the server once this
  // is answered
  bob.settle().await;

  relogin(&server, &bob).await;
  // a replay of the first would be sent at login, before this one
  message(&bob, &alice, "second").await;
  let next = received(&mut bob).await;
  assert_eq!(next.message, "second");
  assert!(!next.replayed);
}

#[tokio::test]
async fn a_message_to_an_offline_user_is_delivered_at_their_login() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", options()).await;
  bob.client.disconnect(None, None).await;
  message(&bob, &alice, "while you were away").await;
  // the server handles alice's messages in order, so it has stored the message once it answers
  // the next one
  alice.settle().await;
  server
    .connect(&bob.client, &bob.username, PASSWORD)
    .await
    .unwrap();
  let stored = received(&mut bob).await;
  assert_eq!(stored.message, "while you were away");
  assert!(stored.replayed);
}

#[tokio::test]
async fn the_public_chat_feed_carries_lines_from_rooms_we_are_not_in() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("public");
  let alice = server.login("alice", options()).await;
  let mut carol = server.login("carol", options()).await;
  let _: RoomData = alice
    .ask(
      room_key(ServerCode::JoinRoom, &room),
      ServerRequest::JoinRoom(JoinRoomRequest {
        room: room.clone(),
        room_type: RoomType::Public,
      }),
    )
    .await;
  carol.send(ServerRequest::AskPublicChat).await;
  carol.settle().await;
  alice
    .send(ServerRequest::SayInChatRoom(SayInChatRoomRequest {
      room: room.clone(),
      message: "hello world".into(),
    }))
    .await;
  let raised = carol
    .event(|event| match event {
      Event::PublicChatMessageReceived {
        room_name,
        username,
        message,
      } => Some((room_name, username, message)),
      _ => None,
    })
    .await;
  assert_eq!(raised, (room, alice.username.clone(), "hello world".into()));
  carol.send(ServerRequest::StopPublicChat).await;
}
