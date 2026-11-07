//! What the client does with each peer message, sent by a scripted peer on a message connection:
//! answers from the application's resolvers, refusals, and what a hostile or vanishing peer
//! causes.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::support::peer::{FakePeer, PeerSide};
use crate::support::{Logs, PATIENCE, Session, listening_options};
use soulseek_client::{
  ClientError, ClientOptions, Event, IncomingBrowse, MessageErrorKind, Resolvers, SearchOptions,
  SearchScope, SearchState, WaitKey,
};
use soulseek_proto::peer::*;
use soulseek_proto::{
  InitMessage, Limits, PeerCode, PeerMessage, RemoteFilename, TransferDirection,
};
use tokio_util::sync::CancellationToken;

fn resolvers() -> Resolvers {
  Resolvers {
    enqueue_download: Some(Arc::new(|_, _, filename: RemoteFilename| {
      Box::pin(async move {
        match filename.as_str() {
          name if name.contains("banned") => Err(ClientError::DownloadEnqueue("Banned".into())),
          name if name.contains("broken") => Err(ClientError::client("disk on fire", None)),
          _ => Ok(()),
        }
      })
    })),
    place_in_queue_resolver: Some(Arc::new(|_, _, _| Box::pin(async { Ok(Some(3)) }))),
    ..Resolvers::default()
  }
}

async fn session() -> Session {
  Session::with_options(ClientOptions {
    resolvers: resolvers(),
    ..listening_options()
  })
  .await
}

/// a scripted peer named bob, connected and known to the client once it answered a request
async fn bob(session: &Session) -> PeerSide {
  let mut bob = session.peer("bob").await;
  bob.send(&PeerMessage::InfoRequest.encode()).await;
  assert!(matches!(
    bob.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  bob
}

#[tokio::test]
async fn a_failed_browse_resolver_sends_an_empty_listing() {
  let mut options = listening_options();
  options.resolvers.browse_response_resolver = Some(Arc::new(|_, _| {
    Box::pin(async { Err(ClientError::client("share cache missing", None)) })
  }));
  let session = Session::with_options(options).await;
  let mut bob = session.peer("bob").await;
  bob.send(&PeerMessage::BrowseRequest.encode()).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::BrowseResponse(BrowseResponse {
      directories: Vec::new(),
      locked_directories: Vec::new(),
    })
  );
}

#[tokio::test]
async fn queueing_a_download_is_answered_with_the_place_or_the_refusal() {
  let logs = Logs::capture();
  let session = session().await;
  let mut bob = bob(&session).await;
  let queue = |filename: &str| {
    PeerMessage::QueueDownload(QueueDownload {
      filename: filename.into(),
    })
    .encode()
  };
  bob.send(&queue("@@music\\fine.mp3")).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: "@@music\\fine.mp3".into(),
      place: 3,
    })
  );
  bob.send(&queue("@@music\\banned.mp3")).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::UploadDenied(UploadDenied {
      filename: "@@music\\banned.mp3".into(),
      reason: "Banned".into(),
    })
  );
  // an internal failure never reaches the peer as it is
  bob.send(&queue("@@music\\broken.mp3")).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::UploadDenied(UploadDenied {
      filename: "@@music\\broken.mp3".into(),
      reason: "Enqueue failed due to internal error".into(),
    })
  );
  assert_eq!(
    logs
      .lines_with(&["Failed to invoke QueueDownload action", "disk on fire"])
      .len(),
    1
  );
}

#[tokio::test]
async fn a_legacy_download_request_is_queued_never_started() {
  let session = session().await;
  let mut bob = bob(&session).await;
  let request = |token, filename: &str| {
    PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Download,
      token,
      filename: filename.into(),
      file_size: None,
    })
    .encode()
  };
  bob.send(&request(5, "@@music\\fine.mp3")).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::TransferResponse(TransferResponse::Denied {
      token: 5,
      message: "Queued".into()
    })
  );
  assert!(matches!(
    bob.read_peer().await,
    PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse { place: 3, .. })
  ));
  bob.send(&request(6, "@@music\\banned.mp3")).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::TransferResponse(TransferResponse::Denied {
      token: 6,
      message: "Banned".into()
    })
  );
  assert!(matches!(
    bob.read_peer().await,
    PeerMessage::UploadDenied(UploadDenied { reason, .. }) if reason == "Banned"
  ));
}

// an offer for a file we do download goes to that download; the transfer suites cover it
#[tokio::test]
async fn an_upload_offer_for_a_file_we_do_not_download_is_refused() {
  let session = session().await;
  let mut bob = bob(&session).await;
  let offer = |token, filename: &str| {
    PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Upload,
      token,
      filename: filename.into(),
      file_size: Some(4_000_000),
    })
    .encode()
  };
  bob.send(&offer(7, "@@music\\unknown.mp3")).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::TransferResponse(TransferResponse::Denied {
      token: 7,
      message: "Cancelled".into()
    })
  );
}

#[tokio::test]
async fn an_invalid_browse_response_fails_the_browse() {
  let session = session().await;
  let mut bob = bob(&session).await;
  // a browse follows its response from the header on, and waits for the whole of it
  let _header = session.client.waiter().wait::<IncomingBrowse>(
    WaitKey::BrowseResponseConnection {
      username: "bob".into(),
    },
    None,
    CancellationToken::new(),
  );
  let response = session.client.waiter().wait::<Arc<BrowseResponse>>(
    WaitKey::Peer {
      code: PeerCode::BrowseResponse,
      username: "bob".into(),
    },
    None,
    CancellationToken::new(),
  );
  bob
    .send(&[9, 0, 0, 0, 5, 0, 0, 0, b'n', b'o', b'p', b'e', b'!'])
    .await;
  let error = response.await.unwrap_err();
  assert!(
    matches!(&error, ClientError::Message { kind: MessageErrorKind::Read, message, .. } if message == "The peer returned an invalid browse response"),
    "{error:?}"
  );
}

/// `message` encoded, its zlib stream cut short before its checksum
fn cut_short(message: PeerMessage) -> Vec<u8> {
  let mut frame = message.encode();
  frame.truncate(frame.len() - 4);
  let len = frame.len() as u32 - 4;
  frame[..4].copy_from_slice(&len.to_le_bytes());
  frame
}

#[tokio::test]
async fn responses_nobody_asked_for_are_dropped_before_inflating() {
  let logs = Logs::capture();
  let session = session().await;
  // anyone can connect and give any name; nothing was asked of this one
  let mut mallory = session.peer("mallory").await;
  let mut frames = cut_short(PeerMessage::BrowseResponse(BrowseResponse {
    directories: Vec::new(),
    locked_directories: Vec::new(),
  }));
  frames.extend(cut_short(PeerMessage::FolderContentsResponse(
    FolderContentsResponse {
      token: 1,
      directory_name: "@@share\\music".into(),
      directories: Vec::new(),
    },
  )));
  mallory.send(&frames).await;
  logs
    .wait_for(&["browse response nobody follows", "skipped"])
    .await;
  logs
    .wait_for(&[
      "folder contents response nobody waits for",
      "dropped before inflating",
    ])
    .await;
  // inflating either would have found it cut short
  assert!(
    logs.lines_with(&["malformed message rejected"]).is_empty(),
    "{}",
    logs.text()
  );
}

#[tokio::test]
async fn ignored_and_unknown_messages_leave_the_connection_up() {
  let logs = Logs::capture();
  let session = session().await;
  let mut bob = bob(&session).await;
  let mut frames = PeerMessage::UploadQueueNotification.encode();
  // a code no message has
  frames.extend([4, 0, 0, 0, 99, 0, 0, 0]);
  frames.extend(PeerMessage::InfoRequest.encode());
  bob.send(&frames).await;
  assert!(matches!(
    bob.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  assert_eq!(
    logs
      .lines_with(&["unhandled peer message", "code=99"])
      .len(),
    1
  );
  assert!(
    logs
      .lines_with(&["unhandled peer message", "code=52"])
      .is_empty()
  );
}

#[tokio::test]
async fn a_frame_longer_than_any_message_closes_the_connection() {
  let session = session().await;
  let mut bob = bob(&session).await;
  bob.send(&u32::MAX.to_le_bytes()).await;
  bob.closed().await;
}

#[tokio::test]
async fn a_frame_longer_than_its_message_is_skipped_unread() {
  let logs = Logs::capture();
  let session = session().await;
  let mut bob = bob(&session).await;
  // a queue request just over its bound, then a request on the same connection
  let len = Limits::DEFAULT.peer_message_bytes(PeerCode::QueueDownload) + 1;
  let mut frames = len.to_le_bytes().to_vec();
  frames.extend(PeerCode::QueueDownload.code().to_le_bytes());
  frames.extend(vec![0; len as usize - 4]);
  frames.extend(PeerMessage::InfoRequest.encode());
  bob.send(&frames).await;
  assert!(matches!(
    bob.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  assert_eq!(
    logs.lines_with(&["frame skipped", "code=43"]).len(),
    1,
    "{}",
    logs.text()
  );
}

#[tokio::test]
async fn a_replaced_connection_closing_never_takes_its_replacement_with_it() {
  let logs = Logs::capture();
  let session = session().await;
  let mut first = bob(&session).await;
  let mut second = bob(&session).await;
  // the replaced connection is still read and answered on until bob closes it
  first.send(&PeerMessage::InfoRequest.encode()).await;
  assert!(matches!(
    first.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  drop(first);
  logs
    .wait_for(&["connection closed", "Remote connection closed"])
    .await;
  // the replacement still answers, and it is the one the client hands out
  second.send(&PeerMessage::InfoRequest.encode()).await;
  assert!(matches!(
    second.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  let connections = session.client.message_connections();
  assert_eq!(connections.len(), 1, "{connections:?}");
  assert_eq!(connections[0].1, second.stream.local_addr().unwrap());
  let cached = session
    .client
    .get_or_add_message_connection("bob", None, &CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(
    cached.handle().remote(),
    second.stream.local_addr().unwrap()
  );
}

#[tokio::test]
async fn a_peer_reconnecting_over_and_over_keeps_two_replaced_connections() {
  let session = session().await;
  let mut sides = Vec::new();
  for _ in 0..5 {
    sides.push(bob(&session).await);
  }
  // the oldest ones are closed as newer ones replace them
  for oldest in &mut sides[..2] {
    oldest.closed().await;
  }
  // the two replaced last are still answered on, and the newest is the current one
  for kept in &mut sides[2..] {
    kept.send(&PeerMessage::InfoRequest.encode()).await;
    assert!(matches!(
      kept.read_peer().await,
      PeerMessage::InfoResponse(_)
    ));
  }
  let connections = session.client.message_connections();
  assert_eq!(connections.len(), 1, "{connections:?}");
  assert_eq!(connections[0].1, sides[4].stream.local_addr().unwrap());
}

#[tokio::test]
async fn one_address_posing_as_many_users_cannot_evict_other_connections() {
  let mut options = listening_options();
  // room for our connection to carol and the eight one address may open to us
  options.limits.max_cached_peer_connections = 9;
  let session = Session::with_options(options).await;
  let carol = FakePeer::start().await;
  let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, u16::try_from(carol.port()).unwrap()));
  session
    .client
    .get_or_add_message_connection("carol", Some(endpoint), &CancellationToken::new())
    .await
    .unwrap();
  let mut ours = carol.accept().await;
  assert!(matches!(ours.read_init().await, InitMessage::PeerInit(_)));
  // each busier than ours, so ours is the one a full cache would evict
  let mut posers = Vec::new();
  for n in 1..=8 {
    let mut poser = session.peer(&format!("poser{n}")).await;
    poser.send(&PeerMessage::InfoRequest.encode()).await;
    assert!(matches!(
      poser.read_peer().await,
      PeerMessage::InfoResponse(_)
    ));
    posers.push(poser);
  }
  session.peer("poser9").await.closed().await;
  ours.send(&PeerMessage::InfoRequest.encode()).await;
  assert!(matches!(
    ours.read_peer().await,
    PeerMessage::InfoResponse(_)
  ));
  let mut connected: Vec<String> = session
    .client
    .message_connections()
    .into_iter()
    .map(|(username, _)| username)
    .collect();
  connected.sort();
  let mut expected: Vec<String> = (1..=8).map(|n| format!("poser{n}")).collect();
  expected.push("carol".into());
  expected.sort();
  assert_eq!(connected, expected);
}

#[tokio::test]
async fn messages_past_the_handler_bound_wait_instead_of_being_dropped() {
  // user info answered only once the test lets it, so every handler stays busy
  let release = Arc::new(tokio::sync::Semaphore::new(0));
  let asked = Arc::new(AtomicUsize::new(0));
  let mut options = listening_options();
  // every answer is queued at once when the handlers are released
  options.peer_connection_options.write_queue_size = 1000;
  options.resolvers.user_info_resolver = Some(Arc::new({
    let release = release.clone();
    let asked = asked.clone();
    move |_, _| {
      asked.fetch_add(1, Ordering::SeqCst);
      let release = release.clone();
      Box::pin(async move {
        release.acquire().await.unwrap().forget();
        Ok(UserInfo {
          description: String::new(),
          picture: None,
          upload_slots: 1,
          queue_length: 0,
          has_free_upload_slot: true,
          upload_permitted: None,
        })
      })
    }
  }));
  let session = Session::with_options(options).await;
  let mut bob = session.peer("bob").await;
  let requests = 300;
  let frames: Vec<u8> = (0..requests)
    .flat_map(|_| PeerMessage::InfoRequest.encode())
    .collect();
  bob.send(&frames).await;
  // 16 messages, the most one connection may have handled, are handled at once
  tokio::time::timeout(PATIENCE, async {
    while asked.load(Ordering::SeqCst) < 16 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
  tokio::time::sleep(Duration::from_millis(100)).await;
  assert_eq!(asked.load(Ordering::SeqCst), 16);
  release.add_permits(requests);
  for _ in 0..requests {
    assert!(matches!(
      bob.read_peer().await,
      PeerMessage::InfoResponse(_)
    ));
  }
}

#[tokio::test]
async fn a_search_takes_one_response_per_user_and_only_under_its_connections_name() {
  let mut session = session().await;
  let mut bob = bob(&session).await;
  let options = SearchOptions {
    search_timeout: Duration::from_millis(500),
    response_limit: 2,
    ..SearchOptions::default()
  };
  let searching = tokio::spawn({
    let client = session.client.clone();
    async move {
      client
        .search(
          "daft punk",
          SearchScope::Network,
          Some(7),
          options,
          &CancellationToken::new(),
        )
        .await
    }
  });
  session
    .event(|event| match event {
      Event::SearchStateChanged { search, .. } if search.state == SearchState::IN_PROGRESS => {
        Some(())
      }
      _ => None,
    })
    .await;
  let answer = |username: &str| {
    PeerMessage::SearchResponse(SearchResponse {
      username: username.into(),
      token: 7,
      files: vec![File {
        code: 1,
        filename: "@@music\\around the world.mp3".into(),
        size: 4_000_000,
        extension: "mp3".into(),
        attributes: Vec::new(),
      }],
      has_free_upload_slot: true,
      upload_speed: 1000,
      queue_length: 0,
      locked_files: Vec::new(),
    })
    .encode()
  };
  // counting the misnamed one or bob's second would reach the response limit
  let mut frames = answer("mallory");
  frames.extend(answer("bob"));
  frames.extend(answer("bob"));
  bob.send(&frames).await;
  let (search, responses) = tokio::time::timeout(PATIENCE, searching)
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert_eq!(
    search.state,
    SearchState::COMPLETED | SearchState::TIMED_OUT
  );
  assert_eq!(responses.len(), 1, "{responses:?}");
  assert_eq!(responses[0].username, "bob");
}
