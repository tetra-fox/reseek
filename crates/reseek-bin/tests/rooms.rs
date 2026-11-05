//! Rooms over soulfind: a configured room is joined at login, what is said there is kept with
//! the mentions of us marked and counted unread, the history and the pinned chats are kept
//! through a restart that joins the room again with the gap the restart left in it; the rooms we
//! were in are joined again after a restart until we leave them, and one taken by a private room
//! is forgotten, while a room joined only because the configuration listed it is not; a room
//! joined twice is joined once, left and joined again, and refused while logged out; the
//! server's room list is asked for once a minute at most; and each message said is one event.

mod support;

use std::time::{Duration, Instant};

use api_client::Client;
use api_types::messaging::rooms::{RoomHistory, RoomMessagesQuery};
use api_types::messaging::{ChatKind, ChatPin};
use soulseek_proto::RoomType;
use support::soulfind::{self, User};
use support::{PATIENCE, daemon, events, logged_in, quiet, signed_in, until};

fn newest() -> RoomMessagesQuery {
  RoomMessagesQuery {
    before: None,
    after: None,
    limit: 100,
    include_hidden: false,
  }
}

/// The history of `room` once `ready` holds for it.
async fn history_when(
  api: &Client,
  room: &str,
  what: &str,
  ready: impl Fn(&RoomHistory) -> bool,
) -> RoomHistory {
  let started = Instant::now();
  loop {
    if let Ok(history) = api.room_messages(room, &newest()).await
      && ready(&history)
    {
      return history;
    }
    assert!(started.elapsed() < PATIENCE, "never {what}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

/// `peer` says `text` in `room` and waits until the server has it.
async fn say(peer: &User, room: &str, text: &str) {
  peer.client.send_room_message(room, text).await.unwrap();
  peer.settle().await;
}

#[tokio::test]
async fn a_configured_room_keeps_its_history_and_its_gaps_across_a_restart() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let room = soulfind::username("room");
  std::fs::write(
    dir.path().join("config.toml"),
    format!("rooms = [\"{room}\"]\n"),
  )
  .unwrap();
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "in the room", |s| {
    logged_in(s) && s.rooms.contains(&room)
  })
  .await;
  let peer = server.login("peer", quiet()).await;
  peer
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();

  say(&peer, &room, &format!("hey {name}, are you there?")).await;
  let first = history_when(&api, &room, "stored", |h| h.messages.len() == 1).await;
  let mention = &first.messages[0];
  assert_eq!(mention.username, peer.username);
  assert!(mention.is_mention && !mention.is_self);
  let said = events(&api, "RoomMessageReceived", 1).await;
  assert_eq!(said.len(), 1);
  assert_eq!(said[0]["message"]["roomName"], room.as_str());
  assert_eq!(said[0]["message"]["username"], peer.username.as_str());
  assert_eq!(said[0]["message"]["message"], mention.message.as_str());
  let joined = api.joined_rooms().await.unwrap();
  assert_eq!((joined[0].unread, joined[0].mentions), (1, 1));
  api.mark_room_read(&room, mention.local_id).await.unwrap();
  assert_eq!(api.joined_rooms().await.unwrap()[0].unread, 0);
  let pins = [
    ChatPin {
      kind: ChatKind::User,
      name: peer.username.clone(),
    },
    ChatPin {
      kind: ChatKind::Room,
      name: room.clone(),
    },
  ];
  let set = api.set_chat_pins(&pins).await.unwrap();
  assert_eq!(set.pins, pins);

  api.shut_down().await.unwrap();
  let mut stopped = running;
  stopped.process.child.wait().unwrap();
  // a gap of under a second is not one
  tokio::time::sleep(Duration::from_millis(1500)).await;
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "in the room again", |s| {
    logged_in(s) && s.rooms.contains(&room)
  })
  .await;
  say(&peer, &room, "welcome back").await;
  let after = history_when(&api, &room, "the second stored", |h| h.messages.len() == 2).await;
  assert_eq!(
    after.messages[0].local_id, mention.local_id,
    "kept across the restart"
  );
  assert_eq!(after.gaps.len(), 1, "{:?}", after.gaps);
  let gap = after.gaps[0];
  assert!(gap.from >= mention.timestamp && gap.to.unwrap() <= after.messages[1].timestamp);
  assert_eq!(
    api.joined_rooms().await.unwrap()[0].unread,
    1,
    "the read marker stayed"
  );
  assert_eq!(
    api.chat_pins().await.unwrap(),
    set,
    "the pins stayed, in their order, with their revision"
  );
}

#[tokio::test]
async fn a_room_is_joined_once_left_and_joined_again_but_not_while_logged_out() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  let room = soulfind::username("lobby");

  let joined = api.join_room(&room, false).await.unwrap().expect("joined");
  assert_eq!(joined.name, room);
  assert!(joined.users.iter().any(|u| u.username == name && u.is_self));
  assert!(
    api.join_room(&room, false).await.unwrap().is_none(),
    "in it already"
  );
  until(&api, "listed", |s| s.rooms.contains(&room)).await;
  api.leave_room(&room).await.unwrap();
  until(&api, "left", |s| !s.rooms.contains(&room)).await;
  assert!(api.join_room(&room, false).await.unwrap().is_some());

  api.disconnect("away").await.unwrap();
  until(&api, "logged out", |s| !s.server.is_connected).await;
  let refused = api.join_room("elsewhere", false).await.unwrap_err();
  assert_eq!(refused.status().map(|s| s.as_u16()), Some(409));
}

#[tokio::test]
async fn the_rooms_we_were_in_are_joined_again_after_a_restart_until_left() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let [kept, left, taken, last] = ["kept", "left", "taken", "last"].map(soulfind::username);
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  for room in [&kept, &left, &taken] {
    api.join_room(room, false).await.unwrap().expect("joined");
  }
  api.leave_room(&left).await.unwrap();
  api.shut_down().await.unwrap();
  let mut stopped = running;
  stopped.process.child.wait().unwrap();
  // with us gone the room is empty and its name free, and a private room takes it
  let owner = server.login("owner", quiet()).await;
  owner
    .client
    .join_room(&taken, RoomType::Private)
    .await
    .unwrap();

  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "in the room again", |s| {
    logged_in(s) && s.rooms.contains(&kept)
  })
  .await;
  running
    .process
    .printed_line(&["can no longer be joined; forgotten", &taken])
    .await;
  api.join_room(&last, false).await.unwrap().expect("joined");
  api.shut_down().await.unwrap();
  let mut stopped = running;
  stopped.process.child.wait().unwrap();

  // the rooms are joined again in the order they were joined, so the last one comes after
  // any attempt at the forgotten one
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  let state = until(&api, "in both rooms again", |s| {
    logged_in(s) && s.rooms.contains(&last)
  })
  .await;
  assert_eq!(state.rooms, [kept, last], "the left room stays left");
  assert!(
    !running.process.printed().contains(&taken),
    "the forgotten room is not tried again"
  );
}

#[tokio::test]
async fn a_room_taken_out_of_the_configuration_is_not_joined_after_a_restart() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let [configured, both, joined] = ["configured", "both", "joined"].map(soulfind::username);
  let config = dir.path().join("config.toml");
  std::fs::write(&config, format!("rooms = [\"{configured}\"]\n")).unwrap();
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "in the configured room", |s| {
    logged_in(s) && s.rooms.contains(&configured)
  })
  .await;
  for room in [&both, &joined] {
    api.join_room(room, false).await.unwrap().expect("joined");
  }
  api.shut_down().await.unwrap();
  let mut stopped = running;
  stopped.process.child.wait().unwrap();

  // the room joined by hand is listed too, and stays ours once the list drops it
  std::fs::write(&config, format!("rooms = [\"{both}\"]\n")).unwrap();
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "in the rooms again", |s| {
    logged_in(s) && s.rooms.contains(&joined)
  })
  .await;
  api.shut_down().await.unwrap();
  let mut stopped = running;
  stopped.process.child.wait().unwrap();

  std::fs::write(&config, "").unwrap();
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  // the remembered rooms are joined in the order they were joined, so a configured room that
  // was wrongly remembered would come before the last one
  let state = until(&api, "in the rooms joined by hand", |s| {
    logged_in(s) && s.rooms.contains(&joined)
  })
  .await;
  assert_eq!(state.rooms, [both, joined]);
}

#[tokio::test]
async fn the_room_list_is_asked_for_once_a_minute_at_most() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let name = soulfind::username("daemon");
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  let peer = server.login("peer", quiet()).await;
  let early = soulfind::username("early");
  peer
    .client
    .join_room(&early, RoomType::Public)
    .await
    .unwrap();

  let listed = api.available_rooms().await.unwrap();
  assert!(
    listed
      .iter()
      .any(|r| r.name == early && r.user_count == 1 && !r.joined)
  );
  let late = soulfind::username("late");
  peer
    .client
    .join_room(&late, RoomType::Public)
    .await
    .unwrap();
  let again = api.available_rooms().await.unwrap();
  assert!(
    !again.iter().any(|r| r.name == late),
    "the list asked for a moment ago is served"
  );
}
