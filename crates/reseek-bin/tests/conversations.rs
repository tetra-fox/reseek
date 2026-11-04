//! Private messages over soulfind between the daemon and another user's client: a message sent
//! while the daemon was away arrives at its login, is stored once however often the server
//! delivers it again, and is delivered again only until it is read; the unread counts follow a
//! read, a message sent and a purge; each delivery, connect and disconnect is one event; and a
//! message with line breaks is sent, stored and pushed on one line.

mod support;

use std::time::{Duration, Instant};

use api_client::Client;
use api_types::messaging::conversations::{
  ConversationListQuery, MessageDirection, PrivateMessage,
};
use soulseek_client::Event;
use support::soulfind::{self, User};
use support::{PATIENCE, daemon, events, logged_in, quiet, signed_in, until};

/// The messages of the conversation with `peer` once `ready` holds for them.
async fn messages_when(
  api: &Client,
  peer: &str,
  what: &str,
  ready: impl Fn(&[PrivateMessage]) -> bool,
) -> Vec<PrivateMessage> {
  let started = Instant::now();
  loop {
    let messages = match api.conversation(peer, true).await {
      Ok(conversation) => conversation.messages.unwrap_or_default(),
      Err(_) => Vec::new(),
    };
    if ready(&messages) {
      return messages;
    }
    assert!(started.elapsed() < PATIENCE, "never {what}: {messages:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

/// Logs the daemon out and in again, so the server delivers what is not acknowledged.
async fn log_in_again(api: &Client) {
  api.disconnect("logging in again").await.unwrap();
  until(api, "logged out", |s| !s.server.is_connected).await;
  api.connect().await.unwrap();
  until(api, "logged in again", logged_in).await;
}

/// `peer` says `text` to `to` and waits until the server has it.
async fn say(peer: &User, to: &str, text: &str) {
  peer.client.send_private_message(to, text).await.unwrap();
  peer.settle().await;
}

#[tokio::test]
async fn a_message_from_while_we_were_away_is_stored_once_until_it_is_read() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  let peer = server.login("peer", quiet()).await;

  api.disconnect("away").await.unwrap();
  until(&api, "logged out", |s| !s.server.is_connected).await;
  say(&peer, &name, "hello while you were away").await;
  api.connect().await.unwrap();
  until(&api, "logged in", logged_in).await;
  let first = messages_when(&api, &peer.username, "stored", |m| m.len() == 1).await;
  let away = &first[0];
  assert_eq!(away.message, "hello while you were away");
  assert_eq!(away.direction, MessageDirection::In);
  assert!(away.was_replayed, "the server kept it for us");
  assert!(!away.is_acknowledged);

  // the server delivers it again at the next login; a live message after it says it was handled
  log_in_again(&api).await;
  say(&peer, &name, "and now").await;
  let again = messages_when(&api, &peer.username, "the live one stored", |m| {
    m.len() == 2
  })
  .await;
  assert_eq!(again[0].local_id, away.local_id, "stored once");
  assert!(!again[0].is_acknowledged);

  let listed = api
    .conversations(&ConversationListQuery::default())
    .await
    .unwrap();
  assert_eq!(listed.len(), 1);
  assert_eq!(listed[0].un_acknowledged_message_count, 2);
  let summary = api.user_summary(&peer.username).await.unwrap();
  assert_eq!(summary.unread_messages, 2);
  assert_eq!(summary.figures.messages.inbound, 2);

  // read, so the server stops delivering them
  api.acknowledge_all(&peer.username).await.unwrap();
  log_in_again(&api).await;
  say(&peer, &name, "third").await;
  let read = messages_when(&api, &peer.username, "the third stored", |m| m.len() == 3).await;
  assert!(
    read[0].is_acknowledged && read[1].is_acknowledged,
    "not delivered again"
  );
  assert!(!read[2].is_acknowledged);

  // our own message counts nothing unread; the purge forgets the rest
  api.send_private(&peer.username, "hi back").await.unwrap();
  let sent = messages_when(&api, &peer.username, "ours stored", |m| m.len() == 4).await;
  assert_eq!(sent[3].direction, MessageDirection::Out);
  assert_eq!(sent[3].id, None);
  let listed = api
    .conversations(&ConversationListQuery::default())
    .await
    .unwrap();
  assert_eq!(listed[0].un_acknowledged_message_count, 1);
  api.close_conversation(&peer.username, true).await.unwrap();
  let closed = api.conversation(&peer.username, true).await.unwrap();
  assert!(!closed.is_active);
  assert_eq!(closed.messages.unwrap(), []);
  assert_eq!(closed.un_acknowledged_message_count, 0);
  assert!(
    api
      .conversations(&ConversationListQuery::default())
      .await
      .unwrap()
      .is_empty(),
    "a closed conversation is listed only when asked"
  );

  // every delivery is an event, the ones the server kept for us marked as replayed
  let delivered = events(&api, "PrivateMessageReceived", 4).await;
  let seen: Vec<(String, bool)> = delivered
    .iter()
    .map(|data| {
      let message = &data["message"];
      (
        message["message"].as_str().unwrap().to_string(),
        message["wasReplayed"].as_bool().unwrap(),
      )
    })
    .collect();
  assert_eq!(
    seen,
    [
      ("hello while you were away".to_string(), true),
      ("hello while you were away".to_string(), true),
      ("and now".to_string(), false),
      ("third".to_string(), false)
    ]
  );
  assert_eq!(events(&api, "SoulseekClientConnected", 4).await.len(), 4);
  let disconnects = events(&api, "SoulseekClientDisconnected", 3).await;
  assert_eq!(disconnects.len(), 3);
  assert_eq!(disconnects[0]["message"], "away");
  assert_eq!(disconnects[0]["exception"]["type"], "IntentionalDisconnect");
}

#[tokio::test]
async fn a_message_with_line_breaks_arrives_as_it_is_stored_on_one_line() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  let peer = server.login("peer", quiet()).await;
  let mut heard = peer.client.subscribe();

  // the server drops a message holding a line break, so this one arriving is the point
  api
    .send_private(&peer.username, "first line\r\n\r\nsecond line \n")
    .await
    .unwrap();
  let arrived = tokio::time::timeout(PATIENCE, async {
    loop {
      if let Event::PrivateMessageReceived(message) = heard.recv().await.unwrap() {
        return message.message;
      }
    }
  })
  .await
  .expect("the message never arrived");
  assert_eq!(arrived, "first line second line");
  let stored = messages_when(&api, &peer.username, "stored", |m| m.len() == 1).await;
  assert_eq!(stored[0].message, arrived);

  api
    .send_private(&peer.username, "\n\r\n")
    .await
    .expect_err("a text of line breaks alone says nothing");
}
