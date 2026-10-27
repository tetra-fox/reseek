//! Watching a user takes one round trip however many ask at once: against a scripted server
//! that counts the watch requests and answers them slowly, two requests to watch the same user
//! both succeed on one watch.

mod support;

use std::time::Duration;

use soulseek_proto::server::{UserStats, UserStatus, WatchUserResponse, WatchedUser};
use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};
use support::scripted;

#[tokio::test]
async fn two_watches_of_one_user_at_once_ask_the_server_once() {
  let dir = tempfile::tempdir().unwrap();
  let (_running, api, mut server) = scripted(dir.path()).await;

  let first = tokio::spawn({
    let api = api.clone();
    async move { api.watch("bob").await }
  });
  let second = tokio::spawn({
    let api = api.clone();
    async move { api.watch("bob").await }
  });
  let mut watches = 0;
  let serving = async {
    loop {
      match server.request().await {
        ServerRequest::WatchUser(request) if request.username == "bob" => {
          watches += 1;
          // slow enough for the second request to arrive while the first is out
          tokio::time::sleep(Duration::from_millis(300)).await;
          let stats = UserStats {
            average_speed: 1000,
            upload_count: 1,
            file_count: 10,
            directory_count: 2,
          };
          let watched = WatchedUser {
            status: UserPresence::Online,
            stats,
            country_code: None,
          };
          let reply = WatchUserResponse {
            username: "bob".into(),
            user: Some(watched),
          };
          server.send(&ServerResponse::WatchUser(reply)).await;
        }
        ServerRequest::GetStatus(request) if request.username == "bob" => {
          let status = UserStatus {
            username: "bob".into(),
            presence: UserPresence::Online,
            is_privileged: false,
          };
          server.send(&ServerResponse::GetStatus(status)).await;
        }
        _ => {}
      }
    }
  };
  tokio::select! {
    () = serving => unreachable!("the server serves until the watches end"),
    (first, second) = async { tokio::join!(first, second) } => {
      first.unwrap().unwrap();
      second.unwrap().unwrap();
    }
  }
  assert_eq!(watches, 1);
  assert_eq!(api.watched().await.unwrap(), ["bob"]);
}
