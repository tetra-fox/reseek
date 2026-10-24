//! Server messages about chat rooms, delivered by a fake server to a logged-in client: chat lines,
//! joining and leaving, members, the room list, tickers and refused joins.

mod support;

use std::sync::Arc;

use soulseek_client::{ClientError, Event, RoomErrorKind, WaitKey};
use soulseek_proto::server::*;
use soulseek_proto::{ServerCode, ServerResponse};
use support::{PATIENCE, Session, USERNAME, room_key, user_data};
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn say_in_chat_room_raises_the_message() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::SayInChatRoom(SayInChatRoomResponse {
      room: "indie".into(),
      username: "bob".into(),
      message: "hi".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomMessageReceived {
        room_name,
        username,
        message,
      } => Some((room_name, username, message)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into(), "hi".into()));
}

#[tokio::test]
async fn join_room_completes_with_the_room_and_its_private_block() {
  let mut session = Session::start().await;
  let room = RoomData {
    name: "secret".into(),
    users: vec![user_data("bob"), user_data(USERNAME)],
    private: Some(PrivateRoomData {
      owner: "bob".into(),
      operators: vec!["carol".into()],
    }),
  };
  let got: RoomData = session
    .reply(
      room_key(ServerCode::JoinRoom, "secret"),
      ServerResponse::JoinRoom(room.clone()),
    )
    .await;
  assert_eq!(got, room);
  assert!(got.is_private());
}

#[tokio::test]
async fn leave_room_completes_and_raises_our_own_departure() {
  let mut session = Session::start().await;
  let () = session
    .reply(
      room_key(ServerCode::LeaveRoom, "indie"),
      ServerResponse::LeaveRoom(LeaveRoom {
        room: "indie".into(),
      }),
    )
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomLeft {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), USERNAME.into()));
}

#[tokio::test]
async fn user_joined_room_raises_the_member_with_their_data() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::UserJoinedRoom(UserJoinedRoom {
      room: "indie".into(),
      user: user_data("bob"),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomJoined {
        room_name,
        username,
        user_data,
      } => Some((room_name, username, user_data)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into(), user_data("bob")));
  assert!(raised.2.slots_full);
}

#[tokio::test]
async fn user_left_room_raises_the_departure() {
  let mut session = Session::start().await;
  session
    .server
    .send(&ServerResponse::UserLeftRoom(UserLeftRoom {
      room: "indie".into(),
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomLeft {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into()));
}

#[tokio::test]
async fn room_list_completes_and_is_raised() {
  let mut session = Session::start().await;
  let list = RoomList {
    public: vec![RoomInfo {
      name: "indie".into(),
      user_count: 12,
    }],
    owned: Vec::new(),
    private: vec![RoomInfo {
      name: "secret".into(),
      user_count: 3,
    }],
    moderated_room_names: vec!["secret".into()],
  };
  let got: Arc<RoomList> = session
    .reply(
      WaitKey::Server(ServerCode::RoomList),
      ServerResponse::RoomList(list.clone()),
    )
    .await;
  assert_eq!(*got, list);
  let raised = session
    .event(|event| match event {
      Event::RoomListReceived(list) => Some(list),
      _ => None,
    })
    .await;
  assert_eq!(*raised, list);
}

#[tokio::test]
async fn room_tickers_are_raised() {
  let mut session = Session::start().await;
  let ticker = RoomTicker {
    username: "bob".into(),
    message: "listening".into(),
  };
  session
    .server
    .send(&ServerResponse::RoomTickers(RoomTickers {
      room: "indie".into(),
      tickers: vec![ticker.clone()],
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomTickerListReceived { room_name, tickers } => Some((room_name, tickers)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), vec![ticker.clone()]));

  session
    .server
    .send(&ServerResponse::RoomTickerAdd(RoomTickerAdd {
      room: "indie".into(),
      ticker: ticker.clone(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomTickerAdded { room_name, ticker } => Some((room_name, ticker)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), ticker));

  session
    .server
    .send(&ServerResponse::RoomTickerRemove(RoomTickerRemove {
      room: "indie".into(),
      username: "bob".into(),
    }))
    .await;
  let raised = session
    .event(|event| match event {
      Event::RoomTickerRemoved {
        room_name,
        username,
      } => Some((room_name, username)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("indie".into(), "bob".into()));
}

#[tokio::test]
async fn cannot_join_room_fails_the_join() {
  let mut session = Session::start().await;
  let wait = session.client.waiter().wait::<RoomData>(
    room_key(ServerCode::JoinRoom, "secret"),
    None,
    CancellationToken::new(),
  );
  session
    .server
    .send(&ServerResponse::CannotJoinRoom(CannotJoinRoom {
      room: "secret".into(),
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
