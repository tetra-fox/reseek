//! A room the server has us in already, which it says by not answering the join, is left and
//! joined again once: against a scripted server that lets the first join go unanswered.

mod support;

use soulseek_proto::server::{JoinRoomRequest, LeaveRoom, RoomData, UserData, UserStats};
use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};
use support::client::USERNAME;
use support::{scripted, until};

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
