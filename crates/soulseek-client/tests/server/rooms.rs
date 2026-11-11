//! Chat rooms on soulfind: joining and leaving as every member sees it, chat lines, tickers, the
//! room list, a join refused because a private room holds the name, and the checks made before
//! anything is sent.

use crate::support::options;
use crate::support::soulfind::{self, User};
use soulseek_client::{ClientError, ClientState, Event, RoomErrorKind};
use soulseek_proto::RoomType;
use soulseek_proto::server::{RoomInfo, UserData};

/// waits for the event telling `user` that `username` joined `room`
async fn joined(user: &mut User, room: &str, username: &str) -> UserData {
  user
    .event(|event| match event {
      Event::RoomJoined {
        room_name,
        username: joined,
        user_data,
      } if room_name == room && joined == username => Some(user_data),
      _ => None,
    })
    .await
}

async fn left(user: &mut User, room: &str, username: &str) {
  user
    .event(|event| {
      matches!(event, Event::RoomLeft { room_name, username: gone } if room_name == room && gone == username)
        .then_some(())
    })
    .await;
}

#[tokio::test]
async fn joining_and_leaving_as_every_member_sees_it() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("room");
  let mut alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", options()).await;
  let (alice_name, bob_name) = (alice.username.clone(), bob.username.clone());

  let data = alice
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();
  assert_eq!(data.name, room);
  assert_eq!(data.users.len(), 1);
  assert_eq!(data.users[0].username, alice_name);
  assert!(!data.is_private());
  // the server announces our own arrival too
  joined(&mut alice, &room, &alice_name).await;

  let data = bob.client.join_room(&room, RoomType::Public).await.unwrap();
  // the server lists members in no fixed order
  let mut members: Vec<&str> = data
    .users
    .iter()
    .map(|user| user.username.as_str())
    .collect();
  members.sort_unstable();
  let mut expected = [alice_name.as_str(), bob_name.as_str()];
  expected.sort_unstable();
  assert_eq!(members, expected);
  let user_data = joined(&mut alice, &room, &bob_name).await;
  assert_eq!(user_data.username, bob_name);

  bob.client.leave_room(&room).await.unwrap();
  // the server does not announce our own departure; the client raises it
  left(&mut bob, &room, &bob_name).await;
  left(&mut alice, &room, &bob_name).await;
}

#[tokio::test]
async fn chat_lines_reach_every_member_including_the_speaker() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("chat");
  let mut alice = server.login("alice", options()).await;
  let mut bob = server.login("bob", options()).await;
  alice
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();
  bob.client.join_room(&room, RoomType::Public).await.unwrap();
  let speaker = bob.username.clone();
  bob.client.send_room_message(&room, "hi all").await.unwrap();
  for user in [&mut alice, &mut bob] {
    let raised = user
      .event(|event| match event {
        Event::RoomMessageReceived {
          room_name,
          username,
          message,
        } => Some((room_name, username, message)),
        _ => None,
      })
      .await;
    assert_eq!(raised, (room.clone(), speaker.clone(), "hi all".into()));
  }
}

#[tokio::test]
async fn tickers_are_listed_on_join_and_pushed_when_set_and_cleared() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("ticker");
  let mut alice = server.login("alice", options()).await;
  let bob = server.login("bob", options()).await;
  alice
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();
  let initial = alice
    .event(|event| match event {
      Event::RoomTickerListReceived { room_name, tickers } if room_name == room => Some(tickers),
      _ => None,
    })
    .await;
  assert!(initial.is_empty());
  bob.client.join_room(&room, RoomType::Public).await.unwrap();

  bob
    .client
    .set_room_ticker(&room, "listening")
    .await
    .unwrap();
  let added = alice
    .event(|event| match event {
      Event::RoomTickerAdded { room_name, ticker } => Some((room_name, ticker)),
      _ => None,
    })
    .await;
  assert_eq!(added.0, room);
  assert_eq!(
    (added.1.username.as_str(), added.1.message.as_str()),
    (bob.username.as_str(), "listening")
  );

  let mut carol = server.login("carol", options()).await;
  carol
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();
  let listed = carol
    .event(|event| match event {
      Event::RoomTickerListReceived { room_name, tickers } if room_name == room => Some(tickers),
      _ => None,
    })
    .await;
  assert_eq!(listed, [added.1]);

  // an empty ticker clears ours
  bob.client.set_room_ticker(&room, "").await.unwrap();
  let removed = carol
    .event(|event| match event {
      Event::RoomTickerRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(removed, (room, bob.username.clone()));
}

#[tokio::test]
async fn the_room_list_counts_the_members() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("listed");
  let alice = server.login("alice", options()).await;
  alice
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap();
  let mut carol = server.login("carol", options()).await;
  let list = carol.client.get_room_list().await.unwrap();
  assert!(
    list.public.contains(&RoomInfo {
      name: room.clone(),
      user_count: 1
    }),
    "{list:?}"
  );
  // the answer is raised as well, as the very list the operation returned
  carol
    .event(|event| {
      matches!(event, Event::RoomListReceived(raised) if std::sync::Arc::ptr_eq(&raised, &list))
        .then_some(())
    })
    .await;
}

#[tokio::test]
async fn joining_a_name_a_private_room_holds_is_refused() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let room = soulfind::username("held");
  let alice = server.login("alice", options()).await;
  alice
    .client
    .join_room(&room, RoomType::Private)
    .await
    .unwrap();
  let carol = server.login("carol", options()).await;
  let error = carol
    .client
    .join_room(&room, RoomType::Public)
    .await
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::Room { kind: RoomErrorKind::JoinForbidden, message } if message == "A private room with this name already exists"),
    "{error:?}"
  );
}

#[tokio::test]
async fn bad_room_names_and_empty_messages_are_refused_before_anything_is_sent() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("alice", options()).await;
  // blank, then one the server's naming rules refuse
  for name in [" ", "two  spaces"] {
    let error = alice
      .client
      .join_room(name, RoomType::Public)
      .await
      .unwrap_err();
    assert!(
      matches!(error, ClientError::InvalidArgument(_)),
      "{name:?}: {error:?}"
    );
  }
  assert!(matches!(
    alice.client.send_room_message("room", "").await,
    Err(ClientError::InvalidArgument(_))
  ));
  assert_eq!(alice.client.state(), ClientState::LoggedIn);
}

#[tokio::test]
async fn room_operations_need_a_logged_in_session() {
  let client = soulseek_client::Client::new(options()).unwrap();
  let error = client
    .join_room("room", RoomType::Public)
    .await
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::InvalidOperation(message) if message == "The server connection must be connected and logged in to join chat room room (currently: Disconnected)"),
    "{error:?}"
  );
}
