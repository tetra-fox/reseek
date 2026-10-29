//! Asking about other users over soulfind: browsing one with the progress pushed to the event
//! stream and the last of it at 100 %, their presence, info and statistics, watching them, and
//! the same 404 for a user who is offline and one the configuration blocks.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use api_types::live::LiveFrame;
use api_types::users::UserPresence;
use futures_util::StreamExt;
use soulseek_client::{BrowseReply, ClientOptions};
use soulseek_proto::peer::{BrowseResponse, Directory, File, UserInfo};
use support::soulfind;
use support::{PATIENCE, daemon, free_port, logged_in, signed_in, until};

/// A share listing of `folders` folders of 100 files each.
fn listing(folders: usize) -> BrowseResponse {
  BrowseResponse {
    directories: (0..folders)
      .map(|folder| Directory {
        name: format!("@@music\\Artist {folder}\\Album {folder}").into(),
        files: (0..100)
          .map(|track| File {
            code: 1,
            filename: format!("{track:02} Track number {track} of album {folder}.flac").into(),
            size: 30_000_000 + track as u64,
            extension: "flac".into(),
            attributes: Vec::new(),
          })
          .collect(),
      })
      .collect(),
    locked_directories: Vec::new(),
  }
}

#[tokio::test]
async fn a_user_is_browsed_with_progress_asked_about_and_watched() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(
    dir.path().join("config.toml"),
    "[transfers.groups.blacklisted]\nmembers = [\"mallory\"]\n",
  )
  .unwrap();
  let name = soulfind::username("daemon");
  let running = daemon(dir.path(), &server, &name, &[]);
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;

  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let shared = listing(200);
  options.resolvers.browse_response_resolver = Some(Arc::new(move |_, _| {
    let shared = shared.clone();
    Box::pin(async move { Ok(BrowseReply::Response(shared)) })
  }));
  options.resolvers.user_info_resolver = Some(Arc::new(|_, _| {
    Box::pin(async {
      Ok(UserInfo {
        description: "hello from the peer".into(),
        picture: None,
        upload_slots: 2,
        queue_length: 0,
        has_free_upload_slot: true,
        upload_permitted: Some(1),
      })
    })
  }));
  let peer = server.login("peer", options).await;

  let mut stream = api.live(&["users"]).open().await.unwrap();
  let browsed = api.browse(&peer.username).await.unwrap();
  assert_eq!(browsed.directory_count, 200);
  assert_eq!(browsed.directories[0].file_count, 100);
  let mut progress = Vec::new();
  let started = Instant::now();
  while progress
    .last()
    .is_none_or(|p: &api_types::users::BrowseProgress| p.percent_complete < 100.0)
  {
    assert!(
      started.elapsed() < PATIENCE,
      "the browse never reached 100 %"
    );
    let event = tokio::time::timeout(PATIENCE, stream.next())
      .await
      .expect("a frame arrives")
      .unwrap()
      .unwrap();
    if let LiveFrame::UsersBrowseProgress(shown) = event.frame {
      assert_eq!(shown.username, peer.username);
      progress.push(shown);
    }
  }
  assert_eq!(progress[0].bytes_transferred, 0, "the start is pushed");
  let status = api.browse_status(&peer.username).await.unwrap();
  assert_eq!(status.percent_complete, 100.0, "kept a while after the end");
  assert_eq!(status.bytes_remaining, 0);

  let presence = api.user_status(&peer.username).await.unwrap();
  assert_eq!(presence.presence, UserPresence::Online);
  let info = api.user_info(&peer.username).await.unwrap();
  assert_eq!(info.description, "hello from the peer");
  assert_eq!(info.upload_permitted, Some(1));
  let statistics = api.user_statistics(&peer.username).await.unwrap();
  assert_eq!(statistics.statistics.file_count, 0);

  api.watch(&peer.username).await.unwrap();
  assert_eq!(
    api.watched().await.unwrap(),
    std::slice::from_ref(&peer.username)
  );
  let cached = api.user_cache().await.unwrap();
  let peer_cached = cached.iter().find(|u| u.username == peer.username).unwrap();
  assert_eq!(peer_cached.status.unwrap().presence, UserPresence::Online);
  api.unwatch(&peer.username).await.unwrap();
  assert!(api.watched().await.unwrap().is_empty());

  // the profile says what was learned along the way, once the touches are written
  let started = Instant::now();
  loop {
    let profile = api.user_profile(&peer.username, false).await.unwrap();
    if profile.user.known && profile.info.is_some() && profile.user.statistics.is_some() {
      assert_eq!(profile.info.unwrap().description, "hello from the peer");
      break;
    }
    assert!(started.elapsed() < PATIENCE, "never stored: {profile:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }

  let offline = soulfind::username("ghost");
  for nobody in [offline.as_str(), "mallory"] {
    let refused = api.endpoint(nobody).await.unwrap_err();
    assert_eq!(refused.status().map(|s| s.as_u16()), Some(404), "{nobody}");
    let refused = api.browse(nobody).await.unwrap_err();
    assert!(
      refused
        .to_string()
        .contains(&format!("User {nobody} appears to be offline")),
      "{refused}"
    );
  }
}
