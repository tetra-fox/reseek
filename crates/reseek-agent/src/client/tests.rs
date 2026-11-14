//! The agent's share uploads against a controller the test plays: the socket's frames come to
//! the test, and the uploads go to a server that answers as the test says. The upload waits
//! only while a load or scan fills the index, or runs after an upload on the socket; a
//! re-upload that fails is tried again with no change to ask for it, a first upload refused as
//! too large copies nothing and is a refusal, and an upload stops when its socket closes. A
//! run of upload failures is warned about once for each way it fails, and a refusal or another
//! agent taking the socket once however many uploads succeed between. A login gives the agent's
//! device id. A file sent or a download
//! fetched whose call fails says what failed without the call's URL.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use api_types::relay::{
  DownloadOutcome, LoginFailure, RELAY_PROTOCOL, RELAY_SUBPROTOCOL, RelayErrorCode, RelayMessage,
};
use cascade::{Layers, Watcher};
use reseek_relay_proto::frames;
use reseek_shares::{ScanNotice, ScanOrigin, Setup, Shares, lock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::AgentClient;
use super::dial::Settings;
use super::session::{Ended, Link};
use crate::options::Options;

/// How the test's controller answers a call.
#[derive(Clone, Copy)]
enum Answer {
  /// reads the upload and answers with this status
  Status(u16),
  /// reads the request and closes the connection without an answer
  Dropped,
  /// reads the request and closes the connection partway through a successful answer's body
  Cut,
  /// takes the connection and never answers
  Silent,
}

/// How the agent's share engine starts.
#[derive(Clone, Copy, PartialEq)]
enum Start {
  /// its startup scan fills the index, and the test waits for it
  Scanned,
  /// its startup scan is held at its start until the test lets it go on
  ScanHeld,
  /// no scan at startup, so nothing fills the index
  Unscanned,
}

/// An agent sharing one file, logged in on a socket whose frames the test reads.
struct Agent {
  dir: tempfile::TempDir,
  agent: Arc<AgentClient>,
  link: Arc<Link>,
  frames: mpsc::Receiver<String>,
  /// the calls the test's controller took
  calls: Arc<AtomicUsize>,
  /// each scan waits at its start for one permit
  scans: Arc<Semaphore>,
  shutdown: CancellationToken,
}

impl Drop for Agent {
  fn drop(&mut self) {
    self.link.close(frames::close::NORMAL);
    self.shutdown.cancel();
  }
}

impl Agent {
  /// The next frame the agent sent, without its pings.
  async fn next(&mut self) -> RelayMessage {
    let text = tokio::time::timeout(Duration::from_secs(10), self.frames.recv())
      .await
      .expect("the agent sends a frame")
      .expect("the socket is open");
    frames::decode(&text).unwrap().message
  }

  /// Answers the agent's next ask for a share upload token with `answer`.
  async fn answer_ask(&mut self, answer: RelayMessage) {
    let asked = self.next().await;
    assert!(
      matches!(asked, RelayMessage::SharesBegin { .. }),
      "{asked:?}"
    );
    self.agent.handle(&self.link, &frames::encode(None, answer));
  }

  /// What the agent sends first once asked to upload: an ask for a token or a deferral.
  async fn asked_to_upload(&mut self) -> RelayMessage {
    self.agent.request_sync();
    self.next().await
  }

  fn copied(&self) -> bool {
    self.dir.path().join("uploads/share_index.db").exists()
  }
}

/// The lines logged on this thread while it lives.
struct Logged {
  text: Arc<std::sync::Mutex<Vec<u8>>>,
  _logging: tracing::subscriber::DefaultGuard,
}

impl Logged {
  /// The warnings and errors.
  fn warnings() -> Self {
    Self::at(tracing::Level::WARN)
  }

  /// The lines at `level` and above.
  fn at(level: tracing::Level) -> Self {
    let text = Arc::new(std::sync::Mutex::new(Vec::new()));
    let written = text.clone();
    let subscriber = tracing_subscriber::fmt()
      .with_max_level(level)
      .with_ansi(false)
      .with_writer(move || Written(written.clone()))
      .finish();
    Self {
      text,
      _logging: tracing::subscriber::set_default(subscriber),
    }
  }

  /// How many lines hold `message`.
  fn count(&self, message: &str) -> usize {
    String::from_utf8_lossy(&lock(&self.text))
      .lines()
      .filter(|line| line.contains(message))
      .count()
  }
}

/// Where [`Logged`] lines are written.
struct Written(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Written {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    lock(&self.0).extend_from_slice(bytes);
    Ok(bytes.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

/// A token for a share upload.
fn token() -> RelayMessage {
  RelayMessage::SharesToken {
    token: Uuid::new_v4(),
    expires_in: 300_000,
  }
}

/// Waits until `check` holds, for at most ten seconds.
async fn eventually(check: impl Fn() -> bool) {
  for _ in 0..200 {
    if check() {
      return;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  panic!("never held");
}

/// A controller's HTTP side that answers the agent's calls with `answers` in turn, counting
/// them in `calls`; its address.
async fn controller(answers: Vec<Answer>, calls: Arc<AtomicUsize>) -> String {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    let mut answers = answers.into_iter();
    loop {
      let (mut connection, _) = listener.accept().await.unwrap();
      calls.fetch_add(1, Ordering::SeqCst);
      let answer = answers.next().expect("no more calls than answers");
      tokio::spawn(async move {
        let status = match answer {
          Answer::Status(status) => status,
          // held open, never answered
          Answer::Silent => return std::future::pending::<()>().await,
          Answer::Dropped => return read_request(&mut connection).await,
          Answer::Cut => {
            read_request(&mut connection).await;
            let head = "HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n{";
            return connection.write_all(head.as_bytes()).await.unwrap();
          }
        };
        read_request(&mut connection).await;
        let head =
          format!("HTTP/1.1 {status} Answered\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        connection.write_all(head.as_bytes()).await.unwrap();
      });
    }
  });
  format!("http://{address}/")
}

/// A controller's relay socket that takes one agent and holds it open; its address.
async fn socket_controller() -> std::net::SocketAddr {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    let (connection, _) = listener.accept().await.unwrap();
    // tungstenite sets the callback's error type, an HTTP response
    #[allow(clippy::result_large_err)]
    let choose = |_: &Request, mut response: Response| {
      response.headers_mut().insert(
        "sec-websocket-protocol",
        HeaderValue::from_static(RELAY_SUBPROTOCOL),
      );
      Ok(response)
    };
    let _socket = tokio_tungstenite::accept_hdr_async(connection, choose)
      .await
      .unwrap();
    std::future::pending::<()>().await
  });
  address
}

/// Reads one request from `connection`, its body as long as its `content-length`.
async fn read_request(connection: &mut tokio::net::TcpStream) {
  let mut read = Vec::new();
  let mut buffer = [0u8; 8192];
  let (head, length) = loop {
    let n = connection.read(&mut buffer).await.unwrap();
    assert!(n > 0, "the request ends early");
    read.extend_from_slice(&buffer[..n]);
    if let Some(end) = read.windows(4).position(|w| w == b"\r\n\r\n") {
      let head = String::from_utf8_lossy(&read[..end]).to_lowercase();
      let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map_or(0, |value| value.trim().parse::<usize>().unwrap());
      break (end + 4, length);
    }
  };
  while read.len() < head + length {
    let n = connection.read(&mut buffer).await.unwrap();
    assert!(n > 0, "the request ends early");
    read.extend_from_slice(&buffer[..n]);
  }
}

/// An agent sharing one file in `dir/share`, with its controller's calls answered by
/// `answers`, logged in on a socket that had an upload when `ready`, once its startup scan
/// filled the index.
async fn agent(answers: Vec<Answer>, ready: bool) -> Agent {
  agent_from(Start::Scanned, answers, ready).await
}

/// [`agent`] with its share engine started as `start`.
async fn agent_from(start: Start, answers: Vec<Answer>, ready: bool) -> Agent {
  let calls = Arc::new(AtomicUsize::new(0));
  let address = controller(answers, calls.clone()).await;
  agent_at(start, &address, calls, ready).await
}

/// [`agent_from`] with its controller at `address`, which counts its calls in `calls`.
async fn agent_at(start: Start, address: &str, calls: Arc<AtomicUsize>, ready: bool) -> Agent {
  let dir = tempfile::tempdir().unwrap();
  let share = dir.path().join("share");
  std::fs::create_dir_all(&share).unwrap();
  std::fs::write(share.join("one.flac"), "one").unwrap();
  let config = format!(
    "instance_name = \"nas\"\n\
     [relay.controller]\naddress = \"{address}\"\napi_key = \"agents-key-0123456789\"\n\
     secret = \"nas-secret-0123456789\"\ndownloads = true\n[shares]\ndirectories = [\"{}\"]\n",
    share.display()
  );
  std::fs::write(dir.path().join("config.toml"), config).unwrap();
  let config = Watcher::new(Layers::<Options>::new().files(dir.path(), ["config.toml"])).unwrap();
  let path = |name: &str| {
    let made = dir.path().join(name);
    std::fs::create_dir_all(&made).unwrap();
    made
  };
  let scans = Arc::new(Semaphore::new(usize::from(start == Start::Scanned)));
  let gate = scans.clone();
  let setup = Setup {
    cache: path("cache"),
    backups: path("backups"),
    downloads: path("downloads"),
    scan_at_startup: start != Start::Unscanned,
    force_scan: false,
    volatile: false,
    case_sensitive: false,
    rescan_on_schedule: false,
    browse_caches: false,
    scans: Box::new(move |notice| {
      let gate = gate.clone();
      Box::pin(async move {
        if matches!(notice, ScanNotice::Started { .. }) {
          gate.acquire().await.unwrap().forget();
        }
      })
    }),
    publish: Box::new(|_| {}),
  };
  let shutdown = CancellationToken::new();
  let shares = Shares::open(&config, |o: &Options| &o.shares, setup, shutdown.clone())
    .await
    .unwrap();
  match start {
    Start::Scanned => {
      shares.initialized().await;
      assert!(!shares.loading_or_scanning() && shares.index_filled());
    }
    Start::ScanHeld => {
      eventually(|| shares.loading_or_scanning()).await;
      assert!(!shares.index_filled());
    }
    Start::Unscanned => assert!(!shares.loading_or_scanning() && !shares.index_filled()),
  }
  let current = config.current();
  let settings = Arc::new(
    Settings::new(&current.values, &current.values.instance_name)
      .await
      .unwrap(),
  );
  let uploads = path("uploads");
  let agent = AgentClient::new(
    config,
    shares,
    path("received"),
    uploads,
    Uuid::new_v4(),
    shutdown.clone(),
  );
  let (link, frames) = Link::opened(settings, ready);
  *lock(&agent.link) = Some(link.clone());
  Agent {
    dir,
    agent,
    link,
    frames,
    calls,
    scans,
    shutdown,
  }
}

/// Whether `sent` asks for a share upload token.
fn asks(sent: &RelayMessage) -> bool {
  matches!(sent, RelayMessage::SharesBegin { .. })
}

#[tokio::test]
async fn the_upload_waits_only_while_a_load_or_scan_fills_the_index_or_follows_an_upload() {
  // a first scan runs: the controller is told the shares come after it, and its end uploads
  let mut client = agent_from(Start::ScanHeld, Vec::new(), false).await;
  let sent = client.asked_to_upload().await;
  assert!(
    matches!(sent, RelayMessage::SyncDeferred { .. }),
    "{sent:?}"
  );
  client.scans.add_permits(1);
  let sent = client.next().await;
  assert!(asks(&sent), "the scan's end uploads: {sent:?}");
  drop(client);

  // idle over a filled index
  let mut client = agent(Vec::new(), false).await;
  assert!(asks(&client.asked_to_upload().await));
  drop(client);

  // a scan over a filled index, on a socket with no upload yet: that index goes up at once
  let mut client = agent(Vec::new(), false).await;
  client.agent.shares.scan(false, ScanOrigin::Asked).unwrap();
  assert!(client.agent.shares.loading_or_scanning() && client.agent.shares.index_filled());
  assert!(asks(&client.asked_to_upload().await));
  drop(client);

  // the same on a socket that had an upload: it keeps that one until the scan's end
  let mut client = agent(Vec::new(), true).await;
  client.agent.shares.scan(false, ScanOrigin::Asked).unwrap();
  // an upload would have been begun before the ask returns, and a deferral queued
  client.agent.request_sync();
  assert!(!client.agent.syncing.load(Ordering::SeqCst));
  assert!(client.frames.try_recv().is_err(), "nothing is sent");
  client.scans.add_permits(1);
  let sent = client.next().await;
  assert!(asks(&sent), "the scan's end uploads: {sent:?}");
  drop(client);

  // nothing filled the index and nothing will, with no scan at startup: it goes up as it is
  let mut client = agent_from(Start::Unscanned, Vec::new(), false).await;
  assert!(asks(&client.asked_to_upload().await));
}

#[tokio::test]
async fn a_failed_upload_after_one_succeeded_is_tried_again_without_another_change() {
  let mut agent = agent(vec![Answer::Status(502), Answer::Status(200)], true).await;
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  eventually(|| agent.link.sync_failures() == 1).await;
  // nothing changed locally; the wait after the failure asks again
  agent.answer_ask(token()).await;
  eventually(|| {
    agent.calls.load(Ordering::SeqCst) == 2
      && agent.link.sync_failures() == 0
      && !agent.agent.syncing.load(Ordering::SeqCst)
  })
  .await;
  assert!(!agent.link.is_closed());
}

#[tokio::test]
async fn a_first_upload_refused_as_too_large_copies_nothing_and_is_a_refusal() {
  let mut agent = agent(Vec::new(), false).await;
  agent.agent.request_sync();
  let asked = agent.next().await;
  let RelayMessage::SharesBegin { database_bytes, .. } = asked else {
    panic!("{asked:?}");
  };
  assert!(database_bytes > 0, "the ask carries the index's size");
  assert!(
    !agent.copied(),
    "the index is copied only once a token came"
  );
  let too_large = RelayMessage::Error {
    code: RelayErrorCode::TooLarge,
    detail: Some("at most 1 byte".into()),
    id: None,
  };
  agent
    .agent
    .handle(&agent.link, &frames::encode(None, too_large));
  eventually(|| agent.link.is_closed()).await;
  let refusal = agent.link.refusal().expect("the refusal is kept");
  assert!(refusal.contains("TooLarge"), "{refusal}");
  assert!(!agent.copied());
  assert_eq!(agent.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_upload_stops_when_its_socket_closes() {
  let mut agent = agent(vec![Answer::Silent], true).await;
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  eventually(|| agent.calls.load(Ordering::SeqCst) == 1).await;
  assert!(agent.agent.syncing.load(Ordering::SeqCst));
  agent.link.close(frames::close::NORMAL);
  eventually(|| !agent.agent.syncing.load(Ordering::SeqCst)).await;
  assert_eq!(
    agent.link.sync_failures(),
    0,
    "a closed socket is no failure"
  );
}

/// Uploads once on `agent`'s socket, answering its ask with a token, and waits for the outcome.
async fn upload_once(agent: &mut Agent) {
  agent.answer_ask(token()).await;
  eventually(|| !agent.agent.syncing.load(Ordering::SeqCst)).await;
}

#[tokio::test]
async fn a_run_of_upload_failures_after_one_succeeded_is_warned_about_again() {
  let warnings = Logged::warnings();
  let answers = [502, 200, 502, 200].map(Answer::Status).to_vec();
  let mut agent = agent(answers, true).await;
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  // the failure's retry succeeds
  upload_once(&mut agent).await;
  assert_eq!(agent.link.sync_failures(), 0);
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  upload_once(&mut agent).await;
  assert_eq!(agent.calls.load(Ordering::SeqCst), 4);
  assert_eq!(
    warnings.count("Failed to upload shares to relay controller"),
    2
  );
}

#[tokio::test]
async fn a_run_of_uploads_whose_connection_drops_is_warned_about_once() {
  let warnings = Logged::warnings();
  let answers = vec![Answer::Dropped, Answer::Dropped, Answer::Status(200)];
  let mut agent = agent(answers, true).await;
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  // each drop's retry asks again, the second with a token of its own
  agent.answer_ask(token()).await;
  upload_once(&mut agent).await;
  assert_eq!(agent.link.sync_failures(), 0);
  assert_eq!(agent.calls.load(Ordering::SeqCst), 3);
  assert_eq!(
    warnings.count("Failed to upload shares to relay controller"),
    1
  );
  assert_eq!(
    warnings.count("relay/controller/shares"),
    0,
    "no token is logged"
  );
}

#[tokio::test]
async fn another_agent_taking_the_socket_is_warned_about_once_across_uploads() {
  let warnings = Logged::warnings();
  let mut agent = agent(vec![Answer::Status(200)], false).await;
  let superseded = Ended {
    ready: true,
    code: Some(frames::close::SUPERSEDED),
    refusal: None,
  };
  agent.agent.report_end(&superseded);
  agent.agent.request_sync();
  upload_once(&mut agent).await;
  assert!(agent.link.ready(), "the upload succeeded");
  agent.agent.report_end(&superseded);
  assert_eq!(
    warnings.count("the relay controller dropped this agent for another"),
    1
  );
}

impl Agent {
  /// Hands the agent the controller's refusal of its login for `reason`.
  fn login_refused(&self, reason: LoginFailure) {
    let refused = RelayMessage::LoginFailed { reason };
    self
      .agent
      .handle(&self.link, &frames::encode(None, refused));
  }

  /// Ends the agent's socket, ready or not, with the refusal its link holds when `refused`.
  fn socket_ended(&self, ready: bool, refused: bool) {
    self.agent.report_end(&Ended {
      ready,
      code: None,
      refusal: if refused { self.link.refusal() } else { None },
    });
  }
}

#[tokio::test]
async fn a_login_refused_for_the_agents_configuration_is_warned_about_once_until_a_socket_is_ready()
{
  let warnings = Logged::warnings();
  let agent = agent(Vec::new(), false).await;
  for _ in 0..3 {
    agent.login_refused(LoginFailure::BadCredential);
    agent.socket_ended(false, true);
  }
  assert_eq!(warnings.count("refused"), 1);
  // the key was put right, and later turned away again
  agent.socket_ended(true, false);
  agent.login_refused(LoginFailure::BadCredential);
  agent.socket_ended(false, true);
  assert_eq!(warnings.count("the relay controller refused this agent"), 2);
}

/// The agent's login names its share index's schema, and a controller on another schema
/// refusing it is a refusal of the agent, not a failed challenge
#[tokio::test]
async fn a_login_names_the_share_schema_and_a_refusal_for_it_is_the_agents() {
  let warnings = Logged::warnings();
  let mut agent = agent(Vec::new(), false).await;
  let challenge = RelayMessage::Challenge {
    token: "abc".into(),
    expires_in: 10_000,
    protocol: RELAY_PROTOCOL,
  };
  agent
    .agent
    .handle(&agent.link, &frames::encode(None, challenge));
  let RelayMessage::Login { shares_schema, .. } = agent.next().await else {
    panic!("the agent does not log in");
  };
  assert_eq!(shares_schema, Some(Shares::schema_version()));
  agent.login_refused(LoginFailure::SchemaMismatch);
  agent.socket_ended(false, true);
  assert_eq!(warnings.count("authentication challenge"), 0);
  let refused = r#"reason="login refused: SchemaMismatch" the relay controller refused this agent"#;
  assert_eq!(warnings.count(refused), 1);
}

/// The agent's login gives its device id, and a refusal for another device connected under its
/// name says so in plain words, once however often it dials again
#[tokio::test]
async fn a_login_gives_the_device_id_and_a_refusal_for_another_device_says_so() {
  let warnings = Logged::warnings();
  let mut agent = agent(Vec::new(), false).await;
  let challenge = RelayMessage::Challenge {
    token: "abc".into(),
    expires_in: 10_000,
    protocol: RELAY_PROTOCOL,
  };
  agent
    .agent
    .handle(&agent.link, &frames::encode(None, challenge));
  let RelayMessage::Login { device_id, .. } = agent.next().await else {
    panic!("the agent does not log in");
  };
  assert_eq!(device_id, Some(agent.agent.device));
  for _ in 0..3 {
    agent.login_refused(LoginFailure::AnotherDevice);
    agent.socket_ended(false, true);
  }
  let refused = format!(
    r#"reason="another device is connected as {}" the relay controller refused this agent"#,
    agent.agent.name
  );
  assert_eq!(warnings.count(&refused), 1);
  assert_eq!(warnings.count("refused"), 1);
}

#[tokio::test]
async fn a_controller_out_of_reach_is_warned_about_once_until_a_socket_is_ready() {
  let warnings = Logged::warnings();
  let agent = agent(Vec::new(), false).await;
  for _ in 0..3 {
    agent.agent.dial_failed("connection refused");
  }
  for _ in 0..3 {
    agent.login_refused(LoginFailure::ChallengeExpired);
    agent.socket_ended(false, false);
  }
  assert_eq!(warnings.count("Failed attempt to connect"), 1);
  assert_eq!(warnings.count("authentication challenge"), 1);
  // failing another way is news
  agent.agent.dial_failed("certificate expired");
  assert_eq!(warnings.count("Failed attempt to connect"), 2);
  // the reasons taking turns, as a controller that is down can give
  agent.agent.dial_failed("connection refused");
  agent.agent.dial_failed("certificate expired");
  assert_eq!(warnings.count("Failed attempt to connect"), 2);
  agent.socket_ended(true, false);
  agent.agent.dial_failed("certificate expired");
  agent.login_refused(LoginFailure::ChallengeExpired);
  assert_eq!(warnings.count("Failed attempt to connect"), 3);
  assert_eq!(warnings.count("authentication challenge"), 2);
}

#[tokio::test]
async fn a_dial_that_connects_ends_the_outage_though_its_socket_is_never_ready() {
  let warnings = Logged::warnings();
  let socket = socket_controller().await;
  let calls = Arc::new(AtomicUsize::new(0));
  let agent = agent_at(Start::Scanned, &format!("http://{socket}/"), calls, false).await;
  agent.agent.dial_failed("connection refused");
  let running = tokio::spawn(agent.agent.clone().run());
  eventually(|| !lock(&agent.agent.dial_warned).ongoing()).await;
  agent.shutdown.cancel();
  running.await.unwrap();
  agent.agent.dial_failed("connection refused");
  assert_eq!(warnings.count("Failed attempt to connect"), 2);
}

/// Whether `text` names the call to the controller made with `token`.
fn names_call(text: &str, token: Uuid) -> bool {
  text.contains("relay/controller/") || text.contains(&token.to_string())
}

#[tokio::test]
async fn a_file_whose_call_drops_fails_without_the_call_url() {
  let warnings = Logged::warnings();
  let mut agent = agent(vec![Answer::Dropped, Answer::Cut], true).await;
  let shared = &agent.agent.shares.browse().await.unwrap()[0];
  let filename = format!("{}\\{}", shared.name, shared.files[0].filename);
  // dropped before the answer, then partway through it
  for failed in ["connection closed", "answer did not arrive"] {
    let token = Uuid::new_v4();
    let request = RelayMessage::FileRequest {
      filename: filename.clone(),
      start_offset: 0,
      transfer_id: Uuid::new_v4(),
      token,
    };
    agent
      .agent
      .handle(&agent.link, &frames::encode(Some(token), request));
    let acked = agent.next().await;
    assert!(matches!(acked, RelayMessage::FileAck {}), "{acked:?}");
    let RelayMessage::FileFailed { detail, .. } = agent.next().await else {
      panic!("the file fails");
    };
    let detail = detail.expect("the failure says what failed");
    assert!(detail.contains(failed), "{detail}");
    assert!(!names_call(&detail, token), "{detail}");
  }
  assert_eq!(warnings.count("Upload of file failed"), 2);
  assert_eq!(warnings.count("relay/controller/files"), 0);
}

#[tokio::test]
async fn a_download_whose_calls_drop_fails_without_the_call_url() {
  let warnings = Logged::warnings();
  let mut agent = agent(vec![Answer::Dropped; 3], true).await;
  let token = Uuid::new_v4();
  let offer = RelayMessage::DownloadAvailable {
    filename: "alice/Album/01.flac".to_string(),
    token,
    size: 3,
    expires_in: 300_000,
  };
  agent
    .agent
    .handle(&agent.link, &frames::encode(Some(token), offer));
  let RelayMessage::DownloadDone { outcome, detail } = agent.next().await else {
    panic!("the download ends");
  };
  assert_eq!(outcome, DownloadOutcome::Failed);
  let detail = detail.expect("the failure says what failed");
  assert!(detail.contains("connection closed"), "{detail}");
  assert!(!names_call(&detail, token), "{detail}");
  assert_eq!(
    warnings.count("Failed to handle file download notification"),
    3
  );
  assert_eq!(warnings.count("relay/controller/downloads"), 0);
}
