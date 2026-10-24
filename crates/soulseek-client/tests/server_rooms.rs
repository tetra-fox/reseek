//! Chat rooms on soulfind: joining and leaving as every member sees it, chat lines, tickers, the
//! room list, and a join refused because a private room holds the name.

mod support;

use std::sync::Arc;

use soulseek_client::{ClientError, Event, RoomErrorKind, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{RoomType, ServerCode, ServerRequest};
use support::soulfind::{self, User};
use support::{PATIENCE, options, room_key};
use tokio_util::sync::CancellationToken;

async fn join(user: &User, room: &str, room_type: RoomType) -> RoomData {
  user
    .ask(
      room_key(ServerCode::JoinRoom, room),
      ServerRequest::JoinRoom(JoinRoomRequest {
        room: room.into(),
        room_type,
      }),
    )
    .await
}

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

  let data = join(&alice, &room, RoomType::Public).await;
  assert_eq!(data.name, room);
  assert_eq!(data.users.len(), 1);
  assert_eq!(data.users[0].username, alice.username);
  assert!(!data.is_private());
  // the server announces our own arrival too
  joined(&mut alice, &room, &alice_name).await;

  let data = join(&bob, &room, RoomType::Public).await;
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

  let () = bob
    .ask(
      room_key(ServerCode::LeaveRoom, &room),
      ServerRequest::LeaveRoom(LeaveRoom { room: room.clone() }),
    )
    .await;
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
  join(&alice, &room, RoomType::Public).await;
  join(&bob, &room, RoomType::Public).await;
  let speaker = bob.username.clone();
  bob
    .send(ServerRequest::SayInChatRoom(SayInChatRoomRequest {
      room: room.clone(),
      message: "hi all".into(),
    }))
    .await;
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
  join(&alice, &room, RoomType::Public).await;
  let initial = alice
    .event(|event| match event {
      Event::RoomTickerListReceived { room_name, tickers } if room_name == room => Some(tickers),
      _ => None,
    })
    .await;
  assert!(initial.is_empty());
  join(&bob, &room, RoomType::Public).await;

  bob
    .send(ServerRequest::SetRoomTicker(SetRoomTicker {
      room: room.clone(),
      ticker: "listening".into(),
    }))
    .await;
  let ticker = RoomTicker {
    username: bob.username.clone(),
    message: "listening".into(),
  };
  let added = alice
    .event(|event| match event {
      Event::RoomTickerAdded { room_name, ticker } => Some((room_name, ticker)),
      _ => None,
    })
    .await;
  assert_eq!(added, (room.clone(), ticker.clone()));

  let mut carol = server.login("carol", options()).await;
  join(&carol, &room, RoomType::Public).await;
  let listed = carol
    .event(|event| match event {
      Event::RoomTickerListReceived { room_name, tickers } if room_name == room => Some(tickers),
      _ => None,
    })
    .await;
  assert_eq!(listed, [ticker]);

  bob
    .send(ServerRequest::SetRoomTicker(SetRoomTicker {
      room: room.clone(),
      ticker: String::new(),
    }))
    .await;
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
  join(&alice, &room, RoomType::Public).await;
  let mut carol = server.login("carol", options()).await;
  let list: Arc<RoomList> = carol
    .ask(
      WaitKey::Server(ServerCode::RoomList),
      ServerRequest::RoomList,
    )
    .await;
  assert!(
    list.public.contains(&RoomInfo {
      name: room.clone(),
      user_count: 1
    }),
    "{list:?}"
  );
  // the answer is raised as well, as the very list the wait completed with
  carol
    .event(|event| {
      matches!(event, Event::RoomListReceived(raised) if Arc::ptr_eq(&raised, &list)).then_some(())
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
  join(&alice, &room, RoomType::Private).await;
  let carol = server.login("carol", options()).await;
  let wait = carol.client.waiter().wait::<RoomData>(
    room_key(ServerCode::JoinRoom, &room),
    None,
    CancellationToken::new(),
  );
  carol
    .send(ServerRequest::JoinRoom(JoinRoomRequest {
      room: room.clone(),
      room_type: RoomType::Public,
    }))
    .await;
  let error = tokio::time::timeout(PATIENCE, wait)
    .await
    .unwrap()
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::Room { kind: RoomErrorKind::JoinForbidden, message } if message == "A private room with this name already exists"),
    "{error:?}"
  );
}
