//! The API through its typed client, against the daemon program: signing in, a route the
//! daemon answers, one whose engine is not built yet, and the event stream with a resume and a
//! reconnect.

mod support;

use api_client::{Client, Error};
use api_types::live::LiveFrame;
use api_types::search::SearchListQuery;
use futures_util::StreamExt;
use support::Daemon;

const ACCOUNT: [(&str, &str); 2] = [("RESEEK_USERNAME", "admin"), ("RESEEK_PASSWORD", "hunter2")];

#[tokio::test]
async fn a_signed_in_client_reads_what_is_built_and_hears_what_is_not() {
  let dir = tempfile::tempdir().unwrap();
  let daemon = Daemon::start(dir.path(), &[], &ACCOUNT);
  let client = Client::new(&daemon.url()).unwrap();
  let refused = client
    .searches(&SearchListQuery::default())
    .await
    .unwrap_err();
  assert_eq!(refused.status().map(|s| s.as_u16()), Some(401), "{refused}");
  let wrong = client.login("admin", "wrong").await.unwrap_err();
  assert_eq!(wrong.status().map(|s| s.as_u16()), Some(401));

  client.login("admin", "hunter2").await.unwrap();
  let session = client.session().await.unwrap();
  assert_eq!(session.principal.name, "admin");
  assert_eq!(client.version().await.unwrap(), env!("CARGO_PKG_VERSION"));
  let state = client.application().await.unwrap();
  assert!(!state.server.is_connected);
  match client.searches(&SearchListQuery::default()).await {
    Err(Error::Api { status, body }) => {
      assert_eq!(status.as_u16(), 503);
      assert_eq!(body.error, "soulseek_unavailable");
      assert_eq!(body.detail.as_deref(), Some("searches not implemented yet"));
    }
    other => panic!("{:?}", other.map(|_| ())),
  }
  let joined = client.join_room(" bad", false).await.unwrap_err();
  assert_eq!(
    joined.status().map(|s| s.as_u16()),
    Some(400),
    "checked before the service"
  );
}

#[tokio::test]
async fn the_event_stream_says_hello_and_resumes_after_what_was_seen() {
  let dir = tempfile::tempdir().unwrap();
  let daemon = Daemon::start(dir.path(), &[], &ACCOUNT);
  let client = Client::new(&daemon.url()).unwrap();
  client.login("admin", "hunter2").await.unwrap();

  let mut stream = client.live(&["application"]).open().await.unwrap();
  let hello = stream.next().await.unwrap().unwrap();
  let LiveFrame::Hello(hello_frame) = hello.frame else {
    panic!("{hello:?}");
  };
  assert!(!hello_frame.resumed);
  assert_eq!(hello_frame.topics, ["application"]);
  let snapshot = stream.next().await.unwrap().unwrap();
  assert!(
    matches!(snapshot.frame, LiveFrame::ApplicationState(_)),
    "{snapshot:?}"
  );
  let seen = snapshot.id.expect("a snapshot has a sequence number");
  drop(stream);

  let mut resumed = client
    .live(&["application"])
    .resume_from(seen)
    .open()
    .await
    .unwrap();
  let again = resumed.next().await.unwrap().unwrap();
  let LiveFrame::Hello(again) = again.frame else {
    panic!("{again:?}");
  };
  assert!(
    again.resumed,
    "what was missed follows, not fresh snapshots"
  );
}

#[tokio::test]
async fn the_event_stream_opens_again_when_the_daemon_comes_back() {
  let dir = tempfile::tempdir().unwrap();
  let trusted = [("RESEEK_TRUSTED_NETWORKS", "127.0.0.0/8")];
  let first = Daemon::start(dir.path(), &[], &trusted);
  let port = first.port;
  let client = Client::new(&first.url()).unwrap();
  let mut stream = client.live(&["application"]).open().await.unwrap();
  // the hello, then the state and the options
  for _ in 0..3 {
    stream.next().await.unwrap().unwrap();
  }
  drop(first);
  let _second = Daemon::start_on(port, dir.path(), &[], &trusted);
  let mut frames = Vec::new();
  while !frames
    .iter()
    .any(|frame| matches!(frame, LiveFrame::Lagged(_)))
  {
    assert!(frames.len() < 10, "{frames:?}");
    match stream.next().await.unwrap() {
      Ok(event) => frames.push(event.frame),
      // the attempts to open the stream while the daemon was away
      Err(_) => continue,
    }
  }
  let LiveFrame::Hello(hello) = &frames[0] else {
    panic!("{frames:?}");
  };
  assert!(
    !hello.resumed,
    "a sequence number from an earlier run is not resumed after"
  );
  assert!(
    frames
      .iter()
      .any(|frame| matches!(frame, LiveFrame::ApplicationState(_))),
    "fresh snapshots: {frames:?}"
  );
}
