//! Private rooms against a scripted server: giving up a room we own, which the server does not
//! answer, leaves the room and forgets it at once, and the server's room list is asked for anew
//! after we create a room, give one up or are added to one; a room not joined as its owner is
//! not ours to give up.

mod support;

use api_client::Client;
use api_types::messaging::rooms::RoomInfo;
use soulseek_proto::server::{
  JoinRoomRequest, LeaveRoom, PrivateMessageResponse, PrivateRoomAdded, PrivateRoomData,
  PrivateRoomDropOwnership, RoomData, RoomList, UserData, UserStats,
};
use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};
use support::client::{ServerSide, USERNAME};
use support::{PATIENCE, scripted};

/// The data of the private room `name` that `owner` owns, with us in it.
fn private_room(name: String, owner: &str) -> RoomData {
  let us = UserData {
    username: USERNAME.into(),
    status: UserPresence::Online,
    stats: UserStats {
      average_speed: 0,
      upload_count: 0,
      file_count: 0,
      directory_count: 0,
    },
    slots_full: false,
    country_code: String::new(),
  };
  RoomData {
    name,
    users: vec![us],
    private: Some(PrivateRoomData {
      owner: owner.into(),
      operators: Vec::new(),
    }),
  }
}

/// Joins the private room `club`, which the server says `owner` owns.
async fn join_club(api: &Client, server: &mut ServerSide, owner: &str) {
  let joining = tokio::spawn({
    let api = api.clone();
    async move { api.join_room("club", true).await }
  });
  let data = server
    .request_matching(|request| match request {
      ServerRequest::JoinRoom(JoinRoomRequest { room, .. }) if room == "club" => {
        Some(private_room(room, owner))
      }
      _ => None,
    })
    .await;
  server.send(&ServerResponse::JoinRoom(data)).await;
  let room = joining.await.unwrap().unwrap().expect("joined anew");
  assert_eq!(room.owner.as_deref(), Some(owner));
}

/// What `GET /rooms/available` answers, the server answering with `list` when asked.
async fn available(api: &Client, server: &mut ServerSide, list: RoomList) -> Vec<RoomInfo> {
  let asking = tokio::spawn({
    let api = api.clone();
    async move { api.available_rooms().await }
  });
  let serving = async {
    loop {
      if matches!(server.request().await, ServerRequest::RoomList) {
        server.send(&ServerResponse::RoomList(list.clone())).await;
      }
    }
  };
  tokio::select! {
    () = serving => unreachable!("the server serves until the list is read"),
    rooms = asking => rooms.unwrap().unwrap(),
  }
}

/// Where the server's list has `club`.
enum Listed {
  Owned,
  Member,
  Public,
}

/// The server's list with `club` where `listed` says.
fn club_listed(listed: Listed) -> RoomList {
  let club = vec![soulseek_proto::server::RoomInfo {
    name: "club".into(),
    user_count: 1,
  }];
  match listed {
    Listed::Owned => RoomList {
      owned: club,
      ..RoomList::default()
    },
    Listed::Member => RoomList {
      private: club,
      ..RoomList::default()
    },
    Listed::Public => RoomList {
      public: club,
      ..RoomList::default()
    },
  }
}

fn club(rooms: &[RoomInfo]) -> &RoomInfo {
  rooms
    .iter()
    .find(|room| room.name == "club")
    .expect("club listed")
}

#[tokio::test]
async fn giving_up_a_room_sends_it_unanswered_then_leaves_it() {
  let dir = tempfile::tempdir().unwrap();
  let (_running, api, mut server) = scripted(dir.path()).await;

  let before = available(&api, &mut server, RoomList::default()).await;
  assert!(before.is_empty());
  join_club(&api, &mut server, USERNAME).await;
  let created = available(&api, &mut server, club_listed(Listed::Owned)).await;
  assert!(
    club(&created).is_owned,
    "the list is asked for after the room is created"
  );

  let giving_up = tokio::spawn({
    let api = api.clone();
    async move { api.drop_room_ownership("club").await }
  });
  // the server answers nothing to the drop and keeps us in the room, so the leave is answered
  let mut seen = Vec::new();
  let serving = async {
    loop {
      match server.request().await {
        ServerRequest::PrivateRoomDropOwnership(PrivateRoomDropOwnership { room }) => {
          seen.push(format!("drop {room}"));
        }
        ServerRequest::LeaveRoom(LeaveRoom { room }) => {
          seen.push(format!("leave {room}"));
          server
            .send(&ServerResponse::LeaveRoom(LeaveRoom { room }))
            .await;
        }
        _ => {}
      }
    }
  };
  let given_up = tokio::time::timeout(PATIENCE, async {
    tokio::select! {
      () = serving => unreachable!("the server serves until the drop ends"),
      given_up = giving_up => given_up.unwrap(),
    }
  })
  .await
  .expect("the drop does not wait for an answer");
  given_up.expect("ownership given up");
  assert_eq!(seen, ["drop club", "leave club"]);

  let state = api.application().await.unwrap();
  assert!(!state.rooms.iter().any(|room| room == "club"));
  assert!(api.joined_rooms().await.unwrap().is_empty());
  let after = available(&api, &mut server, club_listed(Listed::Public)).await;
  let listed = club(&after);
  assert!(
    !listed.is_owned && !listed.is_private,
    "the list is asked for after the room is given up"
  );
}

#[tokio::test]
async fn a_room_not_joined_as_its_owner_is_not_given_up() {
  let dir = tempfile::tempdir().unwrap();
  let (_running, api, mut server) = scripted(dir.path()).await;
  join_club(&api, &mut server, "bob").await;
  let refused = api.drop_room_ownership("club").await.unwrap_err();
  assert_eq!(refused.status(), Some(reqwest::StatusCode::FORBIDDEN));
  let state = api.application().await.unwrap();
  assert!(state.rooms.iter().any(|room| room == "club"));

  // a room we were only added to names no owner
  server
    .send(&ServerResponse::PrivateRoomAdded(PrivateRoomAdded {
      room: "den".into(),
    }))
    .await;
  let started = std::time::Instant::now();
  let refused = loop {
    match api.drop_room_ownership("den").await {
      Err(e) if e.status() == Some(reqwest::StatusCode::NOT_FOUND) => {
        assert!(started.elapsed() < PATIENCE, "the addition never came in");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
      }
      answer => break answer.unwrap_err(),
    }
  };
  assert_eq!(refused.status(), Some(reqwest::StatusCode::FORBIDDEN));
}

#[tokio::test]
async fn a_room_we_are_added_to_is_listed_at_the_next_look() {
  let dir = tempfile::tempdir().unwrap();
  let (_running, api, mut server) = scripted(dir.path()).await;
  assert!(
    available(&api, &mut server, RoomList::default())
      .await
      .is_empty()
  );

  // the daemon takes the server's events in order, so once the message after the addition is
  // in, the addition is too
  server
    .send(&ServerResponse::PrivateRoomAdded(PrivateRoomAdded {
      room: "club".into(),
    }))
    .await;
  server
    .send(&ServerResponse::PrivateMessage(PrivateMessageResponse {
      id: 1,
      timestamp: 1_700_000_000,
      username: "chatty".into(),
      message: "hello".into(),
      is_new: true,
    }))
    .await;
  let started = std::time::Instant::now();
  while api.conversation("chatty", false).await.is_err() {
    assert!(started.elapsed() < PATIENCE, "the message never came in");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
  }

  let added = available(&api, &mut server, club_listed(Listed::Member)).await;
  let listed = club(&added);
  assert!(
    listed.is_private && !listed.is_owned,
    "the list is asked for after we are added"
  );
}
