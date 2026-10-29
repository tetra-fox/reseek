//! What the client does with each peer message, sent by a scripted peer on a message connection:
//! answers from the application's resolvers, replies that complete the waits of our requests,
//! refusals and failures that become events, and what a hostile or vanishing peer causes.

mod support;

use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use soulseek_client::{
  BrowseReply, ClientError, ClientOptions, EncodedFrame, Event, IncomingBrowse, MessageErrorKind,
  Resolvers, WaitKey,
};
use soulseek_proto::peer::*;
use soulseek_proto::{PeerCode, PeerMessage, RemoteFilename, TransferDirection};
use support::peer::PeerSide;
use support::{Logs, PATIENCE, Session, listening_options};
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

fn listing() -> BrowseResponse {
  BrowseResponse {
    directories: vec![Directory {
      name: "@@music\\album".into(),
      files: vec![mp3("01.mp3"), mp3("02.mp3")],
    }],
    locked_directories: Vec::new(),
  }
}

fn resolvers() -> Resolvers {
  Resolvers {
    browse_response_resolver: Some(Arc::new(|_, _| {
      Box::pin(async {
        let frame = PeerMessage::BrowseResponse(listing()).encode();
        Ok(BrowseReply::Encoded(EncodedFrame {
          len: frame.len() as u64,
          reader: Box::pin(Cursor::new(frame)),
        }))
      })
    })),
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

fn file_key(code: PeerCode, filename: &str) -> WaitKey {
  WaitKey::PeerFile {
    code,
    username: "bob".into(),
    filename: filename.into(),
  }
}

#[tokio::test]
async fn a_browse_request_streams_the_encoded_listing() {
  let session = session().await;
  let mut bob = bob(&session).await;
  bob.send(&PeerMessage::BrowseRequest.encode()).await;
  assert_eq!(
    bob.read_peer().await,
    PeerMessage::BrowseResponse(listing())
  );
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
async fn replies_complete_the_waits_of_our_requests() {
  let session = session().await;
  let mut bob = bob(&session).await;
  let waiter = session.client.waiter();
  let transfer = waiter.wait::<TransferResponse>(
    WaitKey::PeerToken {
      code: PeerCode::TransferResponse,
      username: "bob".into(),
      token: 4,
    },
    None,
    CancellationToken::new(),
  );
  let place = waiter.wait::<PlaceInQueueResponse>(
    file_key(PeerCode::PlaceInQueueResponse, "a.mp3"),
    None,
    CancellationToken::new(),
  );
  let folder = waiter.wait::<FolderContentsResponse>(
    WaitKey::PeerToken {
      code: PeerCode::FolderContentsResponse,
      username: "bob".into(),
      token: 12,
    },
    None,
    CancellationToken::new(),
  );
  let info = waiter.wait::<UserInfo>(
    WaitKey::Peer {
      code: PeerCode::InfoResponse,
      username: "bob".into(),
    },
    None,
    CancellationToken::new(),
  );
  let mut frames = PeerMessage::TransferResponse(TransferResponse::Allowed {
    token: 4,
    file_size: None,
  })
  .encode();
  frames.extend(
    PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: "a.mp3".into(),
      place: 2,
    })
    .encode(),
  );
  frames.extend(
    PeerMessage::FolderContentsResponse(FolderContentsResponse {
      token: 12,
      directory_name: "d".into(),
      directories: Vec::new(),
    })
    .encode(),
  );
  frames.extend(
    PeerMessage::InfoResponse(UserInfo {
      description: "bob's".into(),
      picture: None,
      upload_slots: 1,
      queue_length: 0,
      has_free_upload_slot: true,
      upload_permitted: None,
    })
    .encode(),
  );
  bob.send(&frames).await;
  let (transfer, place, folder, info) = tokio::join!(transfer, place, folder, info);
  assert!(matches!(
    transfer.unwrap(),
    TransferResponse::Allowed { token: 4, .. }
  ));
  assert_eq!(place.unwrap().place, 2);
  assert_eq!(folder.unwrap().token, 12);
  assert_eq!(info.unwrap().description, "bob's");
}

#[tokio::test]
async fn a_browse_response_is_announced_by_its_header_before_it_completes() {
  let session = session().await;
  let mut bob = bob(&session).await;
  let header = session.client.waiter().wait::<IncomingBrowse>(
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
  let frame = PeerMessage::BrowseResponse(listing()).encode();
  // the header first, the rest after the header was seen
  bob.send(&frame[..8]).await;
  let incoming = tokio::time::timeout(PATIENCE, header)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(incoming.length as usize, frame.len() - 4);
  assert_eq!(*incoming.received.borrow(), 0);
  bob.send(&frame[8..]).await;
  assert_eq!(*response.await.unwrap(), listing());
  assert_eq!(*incoming.received.borrow(), frame.len() - 8);
}

#[tokio::test]
async fn an_invalid_browse_response_fails_the_browse() {
  let session = session().await;
  let mut bob = bob(&session).await;
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

#[tokio::test]
async fn upload_failures_and_refusals_fail_the_download_and_are_raised() {
  let mut session = session().await;
  let mut bob = bob(&session).await;
  let failed = session
    .client
    .waiter()
    .wait_indefinitely::<TransferRequest>(
      file_key(PeerCode::TransferRequest, "a.mp3"),
      CancellationToken::new(),
    );
  bob
    .send(
      &PeerMessage::UploadFailed(UploadFailed {
        filename: "a.mp3".into(),
      })
      .encode(),
    )
    .await;
  assert!(matches!(
    failed.await.unwrap_err(),
    ClientError::TransferReportedFailed(message) if message == "Download reported as failed by remote client"
  ));
  let raised = session
    .event(|event| match event {
      Event::DownloadFailed { username, filename } => Some((username, filename)),
      _ => None,
    })
    .await;
  assert_eq!(raised, ("bob".into(), "a.mp3".into()));

  let denied = session
    .client
    .waiter()
    .wait_indefinitely::<TransferRequest>(
      file_key(PeerCode::TransferRequest, "b.mp3"),
      CancellationToken::new(),
    );
  bob
    .send(
      &PeerMessage::UploadDenied(UploadDenied {
        filename: "b.mp3".into(),
        reason: "File not shared.".into(),
      })
      .encode(),
    )
    .await;
  assert!(matches!(
    denied.await.unwrap_err(),
    ClientError::TransferRejected(reason) if reason == "File not shared."
  ));
  let raised = session
    .event(|event| match event {
      Event::DownloadDenied {
        username,
        filename,
        message,
      } => Some((username, filename, message)),
      _ => None,
    })
    .await;
  assert_eq!(
    raised,
    ("bob".into(), "b.mp3".into(), "File not shared.".into())
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
async fn a_peer_whose_connection_closes_fails_the_waits_on_it() {
  let session = session().await;
  let bob = bob(&session).await;
  let info = session.client.waiter().wait::<UserInfo>(
    WaitKey::Peer {
      code: PeerCode::InfoResponse,
      username: "bob".into(),
    },
    Some(Duration::from_secs(30)),
    CancellationToken::new(),
  );
  drop(bob);
  let error = tokio::time::timeout(PATIENCE, info)
    .await
    .unwrap()
    .unwrap_err();
  assert!(
    error
      .to_string()
      .starts_with("The message connection to bob closed"),
    "{error}"
  );
  assert!(session.client.message_connections().is_empty());
}

#[tokio::test]
async fn a_frame_longer_than_the_limit_closes_the_connection() {
  let mut options = ClientOptions {
    resolvers: resolvers(),
    ..listening_options()
  };
  options.limits.max_message_size = 1024;
  let session = Session::with_options(options).await;
  let mut bob = bob(&session).await;
  bob.send(&2048u32.to_le_bytes()).await;
  bob.closed().await;
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
  // 256 messages are handled at once
  tokio::time::timeout(PATIENCE, async {
    while asked.load(Ordering::SeqCst) < 256 {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
  tokio::time::sleep(Duration::from_millis(100)).await;
  assert_eq!(asked.load(Ordering::SeqCst), 256);
  release.add_permits(requests);
  for _ in 0..requests {
    assert!(matches!(
      bob.read_peer().await,
      PeerMessage::InfoResponse(_)
    ));
  }
}
