//! The rooms joined again after a login, against a scripted server: a room the server has us
//! in already, which it says by not answering the join, is left and joined again once; and the
//! configured rooms are joined after a login the daemon's events missed in a burst.

mod support;

use soulseek_proto::server::{
  JoinRoomRequest, LeaveRoom, PrivateMessageResponse, RoomData, UserData, UserStats,
};
use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};
use support::client::{USERNAME, login_success};
use support::{PATIENCE, scripted, scripted_login, until};

#[tokio::test]
async fn an_unanswered_join_is_left_and_joined_again_once() {
  let dir = tempfile::tempdir().unwrap();
  let (_running, api, mut server) = scripted(dir.path()).await;

  let joining = tokio::spawn({
    let api = api.clone();
    async move { api.join_room("indie", false).await }
  });
  let (mut joins, mut leaves) = (0, 0);
  let serving = async {
    loop {
      match server.request().await {
        ServerRequest::JoinRoom(JoinRoomRequest { room, .. }) if room == "indie" => {
          joins += 1;
          // the first join goes unanswered, as for a room the server has us in already
          if joins > 1 {
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
            let data = RoomData {
              name: room,
              users: vec![us],
              private: None,
            };
            server.send(&ServerResponse::JoinRoom(data)).await;
          }
        }
        ServerRequest::LeaveRoom(LeaveRoom { room }) if room == "indie" => {
          leaves += 1;
          server
            .send(&ServerResponse::LeaveRoom(LeaveRoom { room }))
            .await;
        }
        _ => {}
      }
    }
  };
  let joined = tokio::select! {
    () = serving => unreachable!("the server serves until the join ends"),
    joined = joining => joined.unwrap().unwrap(),
  };
  let room = joined.expect("joined anew, not taken as joined already");
  assert_eq!(room.name, "indie");
  assert_eq!((joins, leaves), (2, 1));
  until(&api, "in the room", |state| {
    state.rooms.iter().any(|room| room == "indie")
  })
  .await;
}

/// A private message from someone, as the server sends one.
fn message(id: u32) -> Vec<u8> {
  ServerResponse::PrivateMessage(PrivateMessageResponse {
    id,
    timestamp: 1_700_000_000,
    username: "chatty".into(),
    message: format!("hello {id}"),
    is_new: true,
  })
  .encode()
}

#[tokio::test]
async fn a_login_missed_in_a_burst_of_events_still_joins_the_rooms() {
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(dir.path().join("config.toml"), "rooms = [\"indie\"]\n").unwrap();
  let (_running, mut server) = scripted_login(dir.path()).await;
  server.read_login().await;
  // the daemon stores each message before it takes the next, so its events fall far behind
  // and the login among them is gone before it is read
  let mut burst = Vec::new();
  for id in 0..2_000 {
    burst.extend(message(id));
  }
  burst.extend(login_success().encode());
  for id in 2_000..12_000 {
    burst.extend(message(id));
  }
  server.send_bytes(&burst).await;
  let joined = tokio::time::timeout(PATIENCE, async {
    loop {
      if let ServerRequest::JoinRoom(JoinRoomRequest { room, .. }) = server.request().await {
        return room;
      }
    }
  })
  .await
  .expect("the configured room is joined after the login");
  assert_eq!(joined, "indie");
}
