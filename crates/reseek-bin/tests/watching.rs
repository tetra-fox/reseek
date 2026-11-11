//! Watching a user takes one round trip however many ask at once: against a scripted server
//! that counts the watch requests and answers them slowly, two requests to watch the same user
//! both succeed on one watch. What we like and hate goes to the server at every login, not only
//! when it changes.

mod support;

use std::time::Duration;

use api_types::profile::Interests;
use soulseek_proto::server::{
  HatedInterestAdd, LikedInterestAdd, UserStats, UserStatus, WatchUserResponse, WatchedUser,
};
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

#[tokio::test]
async fn our_interests_reach_the_server_when_changed_and_at_every_login() {
  let dir = tempfile::tempdir().unwrap();
  let config = dir.path().join("config.toml");
  std::fs::write(&config, "remote_configuration = true\n").unwrap();
  let (running, api, mut server) = scripted(dir.path()).await;
  let lists = Interests {
    liked: vec!["jazz".into()],
    hated: vec!["polka".into()],
  };
  let read = api.profile().await.unwrap().options_revision;
  api.set_interests(&lists, Some(&read)).await.unwrap();
  let told = server
    .request_matching(|request| match request {
      ServerRequest::LikedInterestAdd(LikedInterestAdd { interest }) => Some(interest),
      _ => None,
    })
    .await;
  assert_eq!(told, "jazz");
  let written = std::fs::read_to_string(&config).unwrap();
  assert!(written.contains("[soulseek.interests]"), "{written}");
  drop(running);

  // the server keeps nothing between sessions, so a new one is told both lists
  let (_running, api, mut server) = scripted(dir.path()).await;
  assert_eq!(api.profile().await.unwrap().interests, lists);
  let liked = server
    .request_matching(|request| match request {
      ServerRequest::LikedInterestAdd(LikedInterestAdd { interest }) => Some(interest),
      _ => None,
    })
    .await;
  let hated = server
    .request_matching(|request| match request {
      ServerRequest::HatedInterestAdd(HatedInterestAdd { interest }) => Some(interest),
      _ => None,
    })
    .await;
  assert_eq!((liked.as_str(), hated.as_str()), ("jazz", "polka"));
}
