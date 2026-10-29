//! Private messages and the public chat feed on soulfind: delivery, the acknowledgment that
//! stops the server replaying a message at every login (sent at once, or by the application
//! later), and messages kept for an offline user.

use crate::support::options;
use crate::support::soulfind::{self, PASSWORD, User};
use soulseek_client::{ClientError, ClientOptions, Event, PrivateMessage};
use soulseek_proto::RoomType;

fn without_acknowledgment() -> ClientOptions {
  ClientOptions {
    auto_acknowledge_private_messages: false,
    ..options()
  }
}

async fn message(to: &User, from: &User, text: &str) {
  from
    .client
    .send_private_message(&to.username, text)
    .await
    .unwrap();
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
  let mut bob = server.login("bob", without_acknowledgment()).await;
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
  // the acknowledgment is queued before this request, so it has reached the server once this
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
  alice
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();
  carol.client.start_public_chat().await.unwrap();
  carol.settle().await;
  alice
    .client
    .send_room_message(&room, "hello world")
    .await
    .unwrap();
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
  carol.client.stop_public_chat().await.unwrap();
}

#[tokio::test]
async fn a_message_the_application_acknowledges_later_is_not_replayed() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", without_acknowledgment()).await;
  message(&bob, &alice, "first").await;
  let first = received(&mut bob).await;
  bob
    .client
    .acknowledge_private_message(first.id)
    .await
    .unwrap();
  bob.settle().await;
  relogin(&server, &bob).await;
  message(&bob, &alice, "second").await;
  assert_eq!(received(&mut bob).await.message, "second");
}

#[tokio::test]
async fn a_message_needs_a_recipient_and_a_text() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  for (username, text) in [(" ", "hi"), ("bob", "")] {
    assert!(matches!(
      alice.client.send_private_message(username, text).await,
      Err(ClientError::InvalidArgument(_))
    ));
  }
  // a text of only whitespace is still a message
  alice.client.send_private_message("bob", " ").await.unwrap();
}
