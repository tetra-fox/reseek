//! Private rooms against a scripted server: giving up a room we own, which the server does not
//! answer, leaves the room and forgets it at once.

mod support;

use soulseek_proto::server::{
  JoinRoomRequest, LeaveRoom, PrivateRoomData, PrivateRoomDropOwnership, RoomData, UserData,
  UserStats,
};
use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};
use support::client::USERNAME;
use support::{PATIENCE, scripted};

/// The data of the private room `name` we own, with us in it.
fn owned(name: String) -> RoomData {
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
      owner: USERNAME.into(),
      operators: Vec::new(),
    }),
  }
}

#[tokio::test]
async fn giving_up_a_room_sends_it_unanswered_then_leaves_it() {
  let dir = tempfile::tempdir().unwrap();
  let (_running, api, mut server) = scripted(dir.path()).await;

  let joining = tokio::spawn({
    let api = api.clone();
    async move { api.join_room("club", true).await }
  });
  let data = server
    .request_matching(|request| match request {
      ServerRequest::JoinRoom(JoinRoomRequest { room, .. }) if room == "club" => Some(owned(room)),
      _ => None,
    })
    .await;
  server.send(&ServerResponse::JoinRoom(data)).await;
  let room = joining.await.unwrap().unwrap().expect("joined anew");
  assert_eq!(room.owner.as_deref(), Some(USERNAME));

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
}
