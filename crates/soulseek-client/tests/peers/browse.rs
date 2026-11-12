//! Browsing a user, fetching a folder and a user's info: between clients on soulfind, and against
//! a scripted peer for the bounds on what a peer may send, a response that never starts or stops
//! halfway, and two browses of one user at once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::support::peer::PeerSide;
use crate::support::scripted::{BOB, Scripted};
use crate::support::soulfind::{self, User};
use crate::support::{Logs, PATIENCE, free_port, listening_options};
use soulseek_client::{BrowseOptions, BrowseProgress, ClientError, ClientOptions};
use soulseek_proto::init::PierceFirewall;
use soulseek_proto::peer::{BrowseResponse, Directory, File, FolderContentsResponse, UserInfo};
use soulseek_proto::server::{ConnectToPeerRequest, GetPeerAddressResponse};
use soulseek_proto::{
  InitMessage, LIMIT_USER_INFO_PICTURE_BYTES, Limits, PeerCode, PeerMessage, RemoteFilename,
  ServerRequest, ServerResponse,
};
use tokio_util::sync::CancellationToken;

fn mp3(name: &str) -> File {
  File {
    code: 1,
    filename: name.into(),
    size: 4_000_000,
    extension: "mp3".into(),
    attributes: Vec::new(),
  }
}

/// a listing of `files` files, large enough to arrive in several reads
fn listing(files: usize) -> BrowseResponse {
  BrowseResponse {
    directories: vec![Directory {
      name: "@@music\\album".into(),
      files: (0..files)
        .map(|n| mp3(&format!("{n:05} track.mp3")))
        .collect(),
    }],
    locked_directories: Vec::new(),
  }
}

fn info() -> UserInfo {
  UserInfo {
    description: "shares albums".into(),
    picture: Some(vec![0x89, b'P', b'N', b'G']),
    upload_slots: 3,
    queue_length: 7,
    has_free_upload_slot: false,
    upload_permitted: None,
  }
}

/// a user that shares `listing`, a folder and user info
async fn sharer(server: &Arc<soulfind::Soulfind>) -> User {
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  options.resolvers.browse_response_resolver = Some(Arc::new(|_, _| {
    Box::pin(async { Ok(soulseek_client::BrowseReply::Response(listing(3000))) })
  }));
  options.resolvers.directory_contents_resolver =
    Some(Arc::new(|_, _, _, directory: RemoteFilename| {
      Box::pin(async move {
        let below = format!("{directory}\\cd2");
        Ok(Some(vec![
          Directory {
            name: directory,
            files: vec![mp3("a.mp3")],
          },
          Directory {
            name: below.as_str().into(),
            files: vec![mp3("b.mp3")],
          },
        ]))
      })
    }));
  options.resolvers.user_info_resolver = Some(Arc::new(|_, _| Box::pin(async { Ok(info()) })));
  server.login("sharer", options).await
}

#[tokio::test]
async fn a_browse_brings_the_whole_listing_and_reports_its_progress() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let sharer = sharer(&server).await;
  let browser = server.login("browser", crate::support::options()).await;
  let seen = Arc::new(Mutex::new(Vec::new()));
  let options = BrowseOptions {
    progress_updated: Some(Arc::new({
      let seen = seen.clone();
      move |progress: &BrowseProgress| {
        seen
          .lock()
          .unwrap()
          .push((progress.username.clone(), progress.percent_complete));
      }
    })),
    ..BrowseOptions::default()
  };
  let browsed = browser
    .client
    .browse(&sharer.username, options, &CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(browsed, listing(3000));
  let seen = seen.lock().unwrap().clone();
  assert!(
    seen
      .iter()
      .all(|(username, _)| *username == sharer.username)
  );
  assert_eq!(seen.first().map(|(_, percent)| *percent), Some(0.0));
  assert_eq!(seen.last().map(|(_, percent)| *percent), Some(100.0));
}

#[tokio::test]
async fn a_folder_comes_with_the_folders_below_it_and_info_says_who_may_upload() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let sharer = sharer(&server).await;
  let asker = server.login("asker", crate::support::options()).await;
  let folder = asker
    .client
    .get_directory_contents(
      &sharer.username,
      "@@music\\album",
      None,
      None,
      &CancellationToken::new(),
    )
    .await
    .unwrap();
  let names: Vec<_> = folder.iter().map(|dir| dir.name.as_str()).collect();
  assert_eq!(names, ["@@music\\album", "@@music\\album\\cd2"]);
  let info = asker
    .client
    .get_user_info(&sharer.username, &CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(
    info,
    UserInfo {
      upload_permitted: Some(1),
      ..self::info()
    }
  );
  let offline = asker
    .client
    .get_user_info(&soulfind::username("nobody"), &CancellationToken::new())
    .await;
  assert!(
    matches!(offline, Err(ClientError::UserOffline(_))),
    "{offline:?}"
  );
}

#[tokio::test]
async fn a_browse_response_larger_than_the_bound_is_refused_and_never_inflated() {
  let logs = Logs::capture();
  let mut options = listening_options();
  options.limits.max_message_size = 1024;
  let mut scripted = Scripted::with_options(options).await;
  let browse = tokio::spawn({
    let client = scripted.client().clone();
    async move {
      client
        .browse(BOB, BrowseOptions::default(), &CancellationToken::new())
        .await
    }
  });
  let mut bob = scripted.connected().await;
  assert_eq!(bob.read_peer().await, PeerMessage::BrowseRequest);
  // the header of a response just over the bound
  let len: u32 = 1025;
  let mut header = len.to_le_bytes().to_vec();
  header.extend(5u32.to_le_bytes());
  bob.send(&header).await;
  let error = tokio::time::timeout(PATIENCE, browse)
    .await
    .unwrap()
    .unwrap()
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::Message { message, .. } if message == "browse response too large"),
    "{error:?}"
  );
  // the rest of it, which is no zlib stream, then a request on the same connection
  let mut rest = vec![0; len as usize - 4];
  rest.extend(PeerMessage::InfoRequest.encode());
  bob.send(&rest).await;
  assert!(matches!(
    bob.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  assert!(
    logs.lines_with(&["malformed message rejected"]).is_empty(),
    "{}",
    logs.text()
  );
}

#[tokio::test]
async fn a_browse_fails_when_its_response_never_starts_or_stops_halfway() {
  let mut scripted = Scripted::start().await;
  let options = BrowseOptions {
    response_timeout: Duration::from_millis(200),
    ..BrowseOptions::default()
  };
  let browse = tokio::spawn({
    let client = scripted.client().clone();
    async move { client.browse(BOB, options, &CancellationToken::new()).await }
  });
  let mut bob = scripted.connected().await;
  assert_eq!(bob.read_peer().await, PeerMessage::BrowseRequest);
  let error = browse.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Timeout(_)), "{error:?}");

  let browse = tokio::spawn({
    let client = scripted.client().clone();
    async move {
      client
        .browse(BOB, BrowseOptions::default(), &CancellationToken::new())
        .await
    }
  });
  assert_eq!(bob.read_peer().await, PeerMessage::BrowseRequest);
  let frame = PeerMessage::BrowseResponse(listing(100)).encode();
  bob.send(&frame[..frame.len() / 2]).await;
  drop(bob);
  let error = browse.await.unwrap().unwrap_err();
  assert!(
    matches!(&error, ClientError::Connection { message, .. } if message.starts_with("Peer connection disconnected unexpectedly")),
    "{error:?}"
  );
}

#[tokio::test]
async fn two_browses_of_one_user_both_take_the_response() {
  let mut scripted = Scripted::start().await;
  let client = scripted.client().clone();
  let browse = || {
    let client = client.clone();
    tokio::spawn(async move {
      client
        .browse(BOB, BrowseOptions::default(), &CancellationToken::new())
        .await
    })
  };
  let first = browse();
  let mut bob = scripted.connected().await;
  let second = browse();
  assert_eq!(bob.read_peer().await, PeerMessage::BrowseRequest);
  assert_eq!(bob.read_peer().await, PeerMessage::BrowseRequest);
  bob
    .send(&PeerMessage::BrowseResponse(listing(10)).encode())
    .await;
  assert_eq!(first.await.unwrap().unwrap(), listing(10));
  assert_eq!(second.await.unwrap().unwrap(), listing(10));
}

#[tokio::test]
async fn user_info_too_large_to_read_fails_the_request() {
  let mut options = listening_options();
  options.message_timeout = Duration::from_secs(30);
  let mut scripted = Scripted::with_options(options).await;
  let client = scripted.client().clone();
  let ask = move || {
    let client = client.clone();
    tokio::spawn(async move { client.get_user_info(BOB, &CancellationToken::new()).await })
  };
  let asked = ask();
  let mut bob = scripted.connected().await;
  assert_eq!(bob.read_peer().await, PeerMessage::InfoRequest);
  // a short description leaves the frame within bounds, so the decoder finds the picture
  let large = UserInfo {
    picture: Some(vec![0; LIMIT_USER_INFO_PICTURE_BYTES + 1]),
    ..info()
  };
  bob.send(&PeerMessage::InfoResponse(large).encode()).await;
  let error = tokio::time::timeout(PATIENCE, asked)
    .await
    .unwrap()
    .unwrap()
    .unwrap_err();
  assert!(error.to_string().contains("invalid user info"), "{error:?}");

  // one longer than any user info may be fails the request from its header
  let asked = ask();
  assert_eq!(bob.read_peer().await, PeerMessage::InfoRequest);
  let len = Limits::DEFAULT.peer_message_bytes(PeerCode::InfoResponse) + 1;
  let mut header = len.to_le_bytes().to_vec();
  header.extend(PeerCode::InfoResponse.code().to_le_bytes());
  bob.send(&header).await;
  let error = tokio::time::timeout(PATIENCE, asked)
    .await
    .unwrap()
    .unwrap()
    .unwrap_err();
  assert!(
    error.to_string().contains("user info too large"),
    "{error:?}"
  );
}

#[tokio::test]
async fn user_info_is_received_from_a_peer_slower_to_connect_than_the_message_timeout() {
  let mut options = listening_options();
  options.message_timeout = Duration::from_millis(300);
  let mut scripted = Scripted::with_options(options).await;
  let client = scripted.client().clone();
  let asked =
    tokio::spawn(async move { client.get_user_info(BOB, &CancellationToken::new()).await });
  // nothing listens where the server says bob is, so only bob dialing us can connect
  let server = &mut scripted.session.server;
  server
    .request_matching(|request| matches!(request, ServerRequest::GetPeerAddress(_)).then_some(()))
    .await;
  server
    .send(&ServerResponse::GetPeerAddress(GetPeerAddressResponse {
      username: BOB.into(),
      ip: std::net::Ipv4Addr::LOCALHOST,
      port: u32::from(free_port()),
      obfuscation: None,
    }))
    .await;
  let token = server
    .request_matching(|request| match request {
      ServerRequest::ConnectToPeer(ConnectToPeerRequest { token, .. }) => Some(token),
      _ => None,
    })
    .await;
  tokio::time::sleep(Duration::from_millis(600)).await;
  let mut bob = PeerSide::connect(scripted.session.listen_address()).await;
  bob
    .send(&InitMessage::PierceFirewall(PierceFirewall { token }).encode())
    .await;
  assert_eq!(bob.read_peer().await, PeerMessage::InfoRequest);
  bob.send(&PeerMessage::InfoResponse(info()).encode()).await;
  let answer = tokio::time::timeout(PATIENCE, asked)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(answer.unwrap(), info());
}

#[tokio::test]
async fn a_folder_request_waits_only_as_long_as_it_is_told_to() {
  let logs = Logs::capture();
  let mut scripted = Scripted::start().await;
  let client = scripted.client().clone();
  let started = tokio::time::Instant::now();
  let folder = tokio::spawn(async move {
    client
      .get_directory_contents(
        BOB,
        "@@music",
        None,
        Some(Duration::from_millis(200)),
        &CancellationToken::new(),
      )
      .await
  });
  let mut bob = scripted.connected().await;
  let PeerMessage::FolderContentsRequest(request) = bob.read_peer().await else {
    panic!("no folder request");
  };
  let folder = tokio::time::timeout(PATIENCE, folder)
    .await
    .unwrap()
    .unwrap();
  assert!(matches!(folder, Err(ClientError::Timeout(_))), "{folder:?}");
  assert!(started.elapsed() < Duration::from_secs(2));
  // nothing waits on bob any more, so the late answer is passed over from its header
  let late = PeerMessage::FolderContentsResponse(FolderContentsResponse {
    token: request.token,
    directory_name: request.directory_name,
    directories: Vec::new(),
  });
  bob.send(&late.encode()).await;
  let code = format!("code={}", PeerCode::FolderContentsResponse.code());
  logs.wait_for(&["frame skipped", &code]).await;
}

#[tokio::test]
async fn a_folder_under_a_token_nobody_asked_for_is_dropped_before_inflating() {
  let logs = Logs::capture();
  let mut scripted = Scripted::start().await;
  let client = scripted.client().clone();
  let folder = tokio::spawn(async move {
    client
      .get_directory_contents(BOB, "@@music", None, None, &CancellationToken::new())
      .await
  });
  let mut bob = scripted.connected().await;
  let PeerMessage::FolderContentsRequest(request) = bob.read_peer().await else {
    panic!("no folder request");
  };
  let answer = |token| {
    PeerMessage::FolderContentsResponse(FolderContentsResponse {
      token,
      directory_name: request.directory_name.clone(),
      directories: vec![Directory {
        name: "@@music".into(),
        files: vec![mp3("@@music\\one.mp3")],
      }],
    })
    .encode()
  };
  // bob was asked, so the frame is read, but its token is not the one asked under
  bob.send(&answer(request.token.wrapping_add(1))).await;
  logs
    .wait_for(&[
      "folder contents response nobody waits for",
      "dropped before inflating",
    ])
    .await;
  bob.send(&answer(request.token)).await;
  let folder = tokio::time::timeout(PATIENCE, folder)
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert_eq!(folder.len(), 1);
}

#[tokio::test]
async fn a_response_read_whole_still_arrives_after_its_connection_closed_waiting_for_a_handler() {
  // user info answered only once the test lets it, so every handler stays busy
  let release = Arc::new(tokio::sync::Semaphore::new(0));
  let mut options = listening_options();
  options.peer_connection_options.inactivity_timeout = Some(Duration::from_millis(300));
  options.resolvers.user_info_resolver = Some(Arc::new({
    let release = release.clone();
    move |_, _| {
      let release = release.clone();
      Box::pin(async move {
        release.acquire().await.unwrap().forget();
        Ok(info())
      })
    }
  }));
  let mut scripted = Scripted::with_options(options).await;
  let browse = tokio::spawn({
    let client = scripted.client().clone();
    async move {
      client
        .browse(BOB, BrowseOptions::default(), &CancellationToken::new())
        .await
    }
  });
  let mut bob = scripted.connected().await;
  assert_eq!(bob.read_peer().await, PeerMessage::BrowseRequest);
  // 16 requests take every handler one connection may hold; the response behind them waits for
  // one, the connection goes quiet and closes
  let mut frames: Vec<u8> = (0..16)
    .flat_map(|_| PeerMessage::InfoRequest.encode())
    .collect();
  frames.extend(PeerMessage::BrowseResponse(listing(10)).encode());
  bob.send(&frames).await;
  bob.closed().await;
  release.add_permits(256);
  let browsed = tokio::time::timeout(PATIENCE, browse)
    .await
    .expect("the browse ends")
    .unwrap()
    .unwrap();
  assert_eq!(browsed, listing(10));
}
