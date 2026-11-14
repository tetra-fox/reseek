//! An agent's socket served without a network, on a clock the test moves: an agent whose
//! shares are being scanned is waited for past the two minutes, but not past its deferral, an
//! agent whose configuration or API key no longer admits it is disconnected, a login checked
//! against a configuration that changed meanwhile is refused, a share upload cut off partway
//! leaves no file behind, and a file in flight stops when its agent is kicked.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use api_types::relay::{
  LoginFailure, RELAY_PROTOCOL, RelayAgentState, RelayEnvelope, RelayMessage,
};
use axum::extract::FromRequest;
use axum::extract::ws::Message;
use cascade::Lasts;
use futures_util::{Sink, Stream, StreamExt};
use reseek_shares::lock;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::{Controller, Registration};
use crate::services::AgentCredential;
use crate::testing::app;
use reseek_relay_proto::credential::{Key, Purpose};
use reseek_relay_proto::frames;

const SECRET: &str = "nas-secret-0123456789";

/// A socket whose frames the test sends and reads.
struct Fake {
  frames: mpsc::Receiver<Result<Message, axum::Error>>,
  sent: mpsc::Sender<Message>,
}

impl Stream for Fake {
  type Item = Result<Message, axum::Error>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.frames.poll_recv(cx)
  }
}

impl Sink<Message> for Fake {
  type Error = axum::Error;

  fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), axum::Error>> {
    Poll::Ready(Ok(()))
  }

  fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), axum::Error> {
    self.sent.try_send(item).map_err(axum::Error::new)
  }

  fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), axum::Error>> {
    Poll::Ready(Ok(()))
  }

  fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), axum::Error>> {
    Poll::Ready(Ok(()))
  }
}

/// Lets the served socket handle what was sent to it.
async fn settle() {
  for _ in 0..50 {
    tokio::task::yield_now().await;
  }
}

/// The frames the socket sent since the last look, without its pings.
fn written(from: &mut mpsc::Receiver<Message>) -> Vec<Message> {
  std::iter::from_fn(|| from.try_recv().ok())
    .filter(|m| !matches!(m, Message::Ping(_)))
    .collect()
}

fn text(message: RelayMessage) -> Result<Message, axum::Error> {
  let envelope = RelayEnvelope {
    v: RELAY_PROTOCOL,
    id: None,
    message,
  };
  Ok(Message::Text(
    serde_json::to_string(&envelope).unwrap().into(),
  ))
}

fn read(message: &Message) -> RelayMessage {
  let Message::Text(text) = message else {
    panic!("not a text frame: {message:?}");
  };
  frames::decode(text.as_str()).unwrap().message
}

/// The frames sent to a socket, what it wrote, and the task serving it.
type Served = (
  mpsc::Sender<Result<Message, axum::Error>>,
  mpsc::Receiver<Message>,
  JoinHandle<()>,
);

/// A socket of `controller`'s from `address`, served on a task of its own and logged in as
/// `nas` with `secret`, opened with a credential that keeps letting it in.
async fn log_in(controller: &Arc<Controller>, address: IpAddr, secret: &str) -> Served {
  log_in_while(controller, address, secret, Arc::new(AtomicBool::new(true))).await
}

/// [`log_in`] with a credential that lets the agent in while `admitted` holds.
async fn log_in_while(
  controller: &Arc<Controller>,
  address: IpAddr,
  secret: &str,
  admitted: Arc<AtomicBool>,
) -> Served {
  let (to_socket, frames) = mpsc::channel(64);
  let (sent, mut from_socket) = mpsc::channel(1024);
  let recheck: crate::live::Revalidate = Arc::new(move || {
    let admitted = admitted.load(Ordering::Relaxed);
    Box::pin(async move { admitted })
  });
  let served = tokio::spawn(controller.clone().serve_socket(
    Fake { frames, sent },
    address,
    recheck,
  ));
  settle().await;
  let RelayMessage::Challenge { token, .. } = read(&written(&mut from_socket)[0]) else {
    panic!("no challenge");
  };
  let credential = Key::derive(secret, "nas").credential(Purpose::Login, &token);
  let login = RelayMessage::Login {
    agent: "nas".into(),
    credential,
    version: crate::version::info().current,
    capabilities: Vec::new(),
  };
  to_socket.send(text(login)).await.unwrap();
  settle().await;
  assert!(matches!(
    read(&written(&mut from_socket)[0]),
    RelayMessage::LoginOk { .. }
  ));
  (to_socket, from_socket, served)
}

/// The code of the close frame among what the socket wrote since the last look.
fn closed(from: &mut mpsc::Receiver<Message>) -> Option<Option<u16>> {
  written(from).into_iter().find_map(|m| match m {
    Message::Close(frame) => Some(frame.map(|f| f.code)),
    _ => None,
  })
}

#[tokio::test]
async fn a_scanning_agent_is_waited_for_until_its_deferral_runs_out() {
  let config = format!(
    "[relay]\nenabled = true\nmode = \"controller\"\n\
     [relay.controller_timeouts]\nsync_deferral = \"3m\"\n\
     [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"{SECRET}\"\n"
  );
  let app = app(&config, &[]).await;
  let controller = app.state.relay.controller.clone().unwrap();
  tokio::time::pause();
  let (to_socket, mut from_socket, served) =
    log_in(&controller, [127, 0, 0, 1].into(), SECRET).await;

  // a pong every 15 seconds keeps the socket from going silent, a deferral every minute
  let deferral = RelayMessage::SyncDeferred {
    reason: "scanning".into(),
  };
  for quarter in 1..=24u32 {
    tokio::time::advance(Duration::from_secs(15)).await;
    settle().await;
    let closed = closed(&mut from_socket);
    // the deferral ends 5 minutes after the login, which the timer may round up past
    let elapsed = quarter * 15;
    if elapsed < 300 {
      assert_eq!(closed, None, "open at {elapsed}s, past the two minutes");
    } else if closed.is_some() || elapsed > 300 {
      assert_eq!(
        closed,
        Some(Some(frames::close::TIMEOUT)),
        "closed at {elapsed}s"
      );
      break;
    }
    to_socket
      .send(Ok(Message::Pong(Default::default())))
      .await
      .unwrap();
    if quarter % 4 == 0 {
      to_socket.send(text(deferral.clone())).await.unwrap();
    }
    settle().await;
  }
  served.await.unwrap();
  assert!(controller.agents().is_empty());
}

#[tokio::test]
async fn an_agent_is_disconnected_once_its_networks_or_secret_no_longer_admit_it() {
  let (app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  // a user-keyed map is one value to the configuration, so each change sets the whole entry
  let set = |cidr: &str, secret: &str| {
    let agents = serde_json::json!({
      "nas": { "instance_name": "nas", "secret": secret, "cidr": cidr }
    });
    let set = BTreeMap::from([("relay.agents".to_string(), agents)]);
    app
      .state
      .config
      .set_overrides(set, Vec::new(), "test", Lasts::UntilRestart, None)
  };

  let (_to_socket, mut from_socket, _served) = log_in(&controller, home, SECRET).await;
  set("10.0.0.0/8", SECRET).await.unwrap();
  settle().await;
  assert_eq!(closed(&mut from_socket), Some(Some(frames::close::KICKED)));
  assert!(
    controller.agents().is_empty(),
    "narrowed out of its networks"
  );

  set("127.0.0.0/8", SECRET).await.unwrap();
  settle().await;
  let (_to_socket, mut from_socket, _served) = log_in(&controller, home, SECRET).await;
  let rotated = "nas-secret-rotated-0123";
  set("127.0.0.0/8", rotated).await.unwrap();
  // a call made with the new secret before the change is followed is still refused, since the
  // socket logged in with the old one
  let token = Uuid::new_v4();
  let caller = AgentCredential {
    agent: "nas".into(),
    credential: Key::derive(rotated, "nas").credential(Purpose::Shares, &token.to_string()),
    address: home,
  };
  assert!(
    controller
      .authenticate(&caller, Purpose::Shares, token)
      .is_err()
  );
  settle().await;
  assert_eq!(closed(&mut from_socket), Some(Some(frames::close::KICKED)));
  assert!(controller.agents().is_empty(), "its secret rotated");
}

/// A controller with the agent `nas` configured.
async fn controller() -> (crate::testing::TestApp, Arc<Controller>) {
  let config = format!(
    "[relay]\nenabled = true\nmode = \"controller\"\n\
     [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"{SECRET}\"\n"
  );
  let app = app(&config, &[]).await;
  let controller = app.state.relay.controller.clone().unwrap();
  (app, controller)
}

/// A share upload token `controller` hands `nas` on the socket numbered `socket`.
fn share_token(controller: &Controller, socket: u64) -> Uuid {
  let begun = controller.begin_shares("nas", socket, crate::shares::Shares::schema_version(), 1024);
  match read(&Message::Text(begun.into())) {
    RelayMessage::SharesToken { token, .. } => token,
    refused => panic!("no share token: {refused:?}"),
  }
}

/// The share upload of `nas` from `home` with `token`, whose database part starts and then
/// nothing more comes, on a task of its own.
async fn stalled_share_upload(
  controller: &Arc<Controller>,
  home: IpAddr,
  token: Uuid,
) -> JoinHandle<Result<(), crate::api::error::ApiError>> {
  let caller = AgentCredential {
    agent: "nas".into(),
    credential: Key::derive(SECRET, "nas").credential(Purpose::Shares, &token.to_string()),
    address: home,
  };
  let start = "--cut\r\nContent-Disposition: form-data; name=\"database\"\r\n\r\nSQLite";
  let body = futures_util::stream::iter([Ok::<_, std::io::Error>(start.as_bytes().to_vec())])
    .chain(futures_util::stream::pending());
  let request = axum::http::Request::builder()
    .header("content-type", "multipart/form-data; boundary=cut")
    .body(axum::body::Body::from_stream(body))
    .unwrap();
  let form = axum::extract::Multipart::from_request(request, &())
    .await
    .unwrap();
  tokio::spawn({
    let controller = controller.clone();
    async move { controller.receive_shares(token, caller, form).await }
  })
}

/// How many share uploads have a file in `controller`'s cache directory.
fn upload_files(controller: &Controller) -> usize {
  let cache = controller.shares.upload_file("nas");
  std::fs::read_dir(cache.parent().unwrap())
    .unwrap()
    .filter_map(Result::ok)
    .filter(|entry| entry.file_name().to_string_lossy().ends_with(".db.tmp"))
    .count()
}

/// Waits until `check` holds, for at most ten seconds.
async fn eventually(what: &str, check: impl Fn() -> bool) {
  let waited = tokio::time::timeout(Duration::from_secs(10), async {
    while !check() {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  });
  waited.await.unwrap_or_else(|_| panic!("{what}"));
}

#[tokio::test]
async fn a_share_upload_cut_off_partway_leaves_no_file_behind() {
  let (_app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  let _socket = log_in(&controller, home, SECRET).await;
  let socket = lock(&controller.registry).agents["nas"].socket;
  let token = share_token(&controller, socket);
  let upload = stalled_share_upload(&controller, home, token).await;
  eventually("the upload's file is written", || {
    upload_files(&controller) > 0
  })
  .await;
  // the agent goes away: the call is dropped
  upload.abort();
  assert!(upload.await.unwrap_err().is_cancelled());
  eventually("the cut-off upload's file is removed", || {
    upload_files(&controller) == 0
  })
  .await;
}

/// An agent whose index is too large asks again each time it dials: the refusal is warned
/// about once however its size changes, and again only after an upload was let in
#[tokio::test]
async fn a_share_upload_refused_again_is_warned_about_once() {
  let (_app, controller) = controller().await;
  let schema = crate::shares::Shares::schema_version();
  let too_large = 1 << 40;
  let ((), logged) = reseek_shares::testing::logged(|| {
    // the index grows between dials
    for socket in 1..=3 {
      controller.begin_shares("nas", socket, schema, too_large + socket);
    }
    share_token(&controller, 4);
    controller.begin_shares("nas", 5, schema, too_large);
  });
  let warned = logged
    .lines()
    .filter(|line| line.contains("WARN") && line.contains("agent's share upload refused"))
    .count();
  assert_eq!(warned, 2, "{logged}");
}

#[tokio::test]
async fn a_share_upload_for_a_socket_no_longer_registered_is_refused_before_it_is_read() {
  let (_app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  let _socket = log_in(&controller, home, SECRET).await;
  // handed out on a socket that has closed since, the agent being registered on another
  let gone = lock(&controller.registry).agents["nas"].socket + 1;
  let token = share_token(&controller, gone);
  let upload = stalled_share_upload(&controller, home, token).await;
  let refused = tokio::time::timeout(Duration::from_secs(10), upload)
    .await
    .expect("the call is answered without reading the upload")
    .unwrap();
  assert_eq!(refused.unwrap_err().status(), 401);
  assert_eq!(upload_files(&controller), 0);
}

#[tokio::test]
async fn a_share_upload_stops_when_its_socket_leaves_so_the_next_one_may_send() {
  let (_app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  let _socket = log_in(&controller, home, SECRET).await;
  let socket = lock(&controller.registry).agents["nas"].socket;
  let token = share_token(&controller, socket);
  let upload = stalled_share_upload(&controller, home, token).await;
  eventually("the upload's file is written", || {
    upload_files(&controller) > 0
  })
  .await;
  assert!(controller.kick("nas"));
  let stopped = tokio::time::timeout(Duration::from_secs(10), upload)
    .await
    .expect("the upload stops with its socket")
    .unwrap();
  assert_eq!(stopped.unwrap_err().status(), 409);
  let _socket = log_in(&controller, home, SECRET).await;
  let socket = lock(&controller.registry).agents["nas"].socket;
  share_token(&controller, socket);
  eventually("the stopped upload's file is removed", || {
    upload_files(&controller) == 0
  })
  .await;
}

#[tokio::test]
async fn an_agent_whose_api_key_no_longer_lets_it_in_is_disconnected() {
  let (_app, controller) = controller().await;
  tokio::time::pause();
  let admitted = Arc::new(AtomicBool::new(true));
  let (to_socket, mut from_socket, served) =
    log_in_while(&controller, [127, 0, 0, 1].into(), SECRET, admitted.clone()).await;
  tokio::time::advance(Duration::from_secs(15)).await;
  settle().await;
  assert_eq!(closed(&mut from_socket), None);
  // a pong keeps the socket from going silent at the same time
  to_socket
    .send(Ok(Message::Pong(Default::default())))
    .await
    .unwrap();
  settle().await;
  admitted.store(false, Ordering::Relaxed);
  // past the check at 30 seconds, which the timer may round up past
  tokio::time::advance(Duration::from_secs(16)).await;
  settle().await;
  assert_eq!(
    closed(&mut from_socket),
    Some(Some(frames::close::UNAUTHORIZED))
  );
  served.await.unwrap();
  assert!(controller.agents().is_empty());
}

#[tokio::test]
async fn a_login_whose_agent_was_removed_while_it_was_checked_is_refused() {
  let (app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  // the login read the configuration and checked the credential with it; then the agent goes
  let removed = BTreeMap::from([("relay.agents".to_string(), serde_json::json!({}))]);
  app
    .state
    .config
    .set_overrides(removed, Vec::new(), "test", Lasts::UntilRestart, None)
    .await
    .unwrap();
  let (outbound, _queued) = mpsc::channel(8);
  let registration = Registration {
    socket: controller.next_socket(),
    record: RelayAgentState {
      name: "nas".into(),
      ip_address: home.to_string(),
      connected_at: chrono::Utc::now(),
      ready: false,
      shares_updated_at: None,
      version: crate::version::info().current,
    },
    outbound,
    address: home,
    secret: SECRET.into(),
    key: Key::derive(SECRET, "nas"),
    left: tokio_util::sync::CancellationToken::new(),
  };
  assert_eq!(
    controller.register(registration),
    Err(LoginFailure::UnknownAgent)
  );
  assert!(controller.agents().is_empty());
}

#[tokio::test]
async fn a_kicked_agents_file_in_flight_stops_and_the_upload_reading_it_fails() {
  let (_app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  let (_to_socket, mut from_socket, _served) = log_in(&controller, home, SECRET).await;
  lock(&controller.registry)
    .agents
    .get_mut("nas")
    .unwrap()
    .record
    .ready = true;
  let opening = tokio::spawn({
    let controller = controller.clone();
    async move {
      controller
        .open_stream("nas", "a.flac", 0, Uuid::new_v4())
        .await
    }
  });
  settle().await;
  let token = written(&mut from_socket)
    .iter()
    .find_map(|m| match read(m) {
      RelayMessage::FileRequest { token, .. } => Some(token),
      _ => None,
    })
    .expect("the agent is asked for the file");
  // the file's first bytes arrive, then nothing more for now
  let start = "--cut\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.flac\"\r\n\r\n\
    first bytes";
  let body = futures_util::stream::iter([Ok::<_, std::io::Error>(start.as_bytes().to_vec())])
    .chain(futures_util::stream::pending());
  let request = axum::http::Request::builder()
    .header("content-type", "multipart/form-data; boundary=cut")
    .body(axum::body::Body::from_stream(body))
    .unwrap();
  let form = axum::extract::Multipart::from_request(request, &())
    .await
    .unwrap();
  let caller = AgentCredential {
    agent: "nas".into(),
    credential: Key::derive(SECRET, "nas").credential(Purpose::File, &token.to_string()),
    address: home,
  };
  let sending = tokio::spawn({
    let controller = controller.clone();
    async move { controller.receive_file(token, caller, form).await }
  });
  let mut stream = opening.await.unwrap().expect("the file arrives");
  let mut first = [0u8; 5];
  stream.read_exact(&mut first).await.unwrap();
  assert_eq!(&first, b"first");

  assert!(controller.kick("nas"));
  let sent = tokio::time::timeout(Duration::from_secs(10), sending)
    .await
    .expect("the agent's call ends")
    .unwrap();
  assert_eq!(sent.unwrap_err().status(), 409);
  let mut rest = Vec::new();
  let read = tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut rest))
    .await
    .expect("the upload's read ends");
  assert_eq!(
    read.unwrap_err().kind(),
    std::io::ErrorKind::ConnectionAborted
  );
}

/// A controller with the agent `nas` configured, which has `file_info` to say whether it has a
/// file, and loopback trusted for the API.
async fn controller_asking_within(file_info: &str) -> (crate::testing::TestApp, Arc<Controller>) {
  let config = format!(
    "[relay]\nenabled = true\nmode = \"controller\"\n\
     [relay.controller_timeouts]\nfile_info = \"{file_info}\"\n\
     [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"{SECRET}\"\n\
     [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n"
  );
  let app = app(&config, &[]).await;
  let controller = app.state.relay.controller.clone().unwrap();
  (app, controller)
}

/// The file `nas` shares, which its index holds.
const AGENT_FILE: &str = "Agent\\a.flac";

/// Loads into `controller` the index of `nas`, which holds [`AGENT_FILE`], and marks the agent,
/// logged in already, ready.
async fn agent_ready(controller: &Controller) {
  crate::testing::load_agent_host(&controller.shares, "nas", "Agent").await;
  lock(&controller.registry)
    .agents
    .get_mut("nas")
    .unwrap()
    .record
    .ready = true;
}

/// The id of the next file info request the socket sends, waited for.
async fn file_info_request(from: &mut mpsc::Receiver<Message>) -> Uuid {
  tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      let asked = written(from).iter().find_map(|m| {
        let Message::Text(text) = m else {
          return None;
        };
        let envelope = frames::decode(text.as_str()).unwrap();
        matches!(envelope.message, RelayMessage::FileInfoRequest { .. })
          .then(|| envelope.id.unwrap())
      });
      if let Some(id) = asked {
        return id;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the agent is asked about the file")
}

/// The agent's answer `id` to a file info request.
fn file_info(id: Uuid, exists: bool, length: u64) -> Result<Message, axum::Error> {
  let answer = frames::encode(Some(id), RelayMessage::FileInfo { exists, length });
  Ok(Message::Text(answer.into()))
}

/// What a peer asking `app` for [`AGENT_FILE`] is told.
async fn asked(app: &crate::testing::TestApp) -> Result<(), soulseek_client::ClientError> {
  use crate::network::resolvers::tests::{edges, from};
  crate::network::resolvers::enqueue_download(
    &edges(app),
    "alice",
    from("198.51.100.7"),
    &soulseek_proto::RemoteFilename::from(AGENT_FILE),
  )
  .await
}

/// An agent that is logged in but does not say in time whether it has a file gets the peer
/// told to ask again; one that says it does not have it gets the file refused as not shared.
#[tokio::test]
async fn a_file_its_agent_is_slow_to_find_has_the_peer_ask_again() {
  let (app, controller) = controller_asking_within("100ms").await;
  let (to_socket, mut from_socket, _served) =
    log_in(&controller, [127, 0, 0, 1].into(), SECRET).await;
  agent_ready(&controller).await;

  let slow = asked(&app).await;
  assert!(
    matches!(slow, Err(soulseek_client::ClientError::DownloadUnavailable)),
    "{slow:?}"
  );

  let patient = BTreeMap::from([(
    "relay.controller_timeouts.file_info".to_string(),
    serde_json::json!("10s"),
  )]);
  app
    .state
    .config
    .set_overrides(patient, Vec::new(), "test", Lasts::UntilRestart, None)
    .await
    .unwrap();
  // the request the slow ask left unanswered
  file_info_request(&mut from_socket).await;
  let refused = {
    let asking = asked(&app);
    tokio::pin!(asking);
    let id = tokio::select! {
      id = file_info_request(&mut from_socket) => id,
      answer = &mut asking => panic!("answered before the agent: {answer:?}"),
    };
    to_socket.send(file_info(id, false, 0)).await.unwrap();
    asking.await
  };
  assert!(
    matches!(&refused, Err(soulseek_client::ClientError::DownloadEnqueue(reason)) if reason == "File not shared."),
    "{refused:?}"
  );
}

/// An upload accepted while its agent had the file, whose agent leaves before the upload's task
/// asks it again, is not failed as gone: the client runs it, and the agent is asked for the
/// file when its turn comes. The daemon here is logged out, so the client's upload ends at once.
#[tokio::test]
async fn an_upload_whose_agent_leaves_before_its_task_starts_is_left_to_its_turn() {
  let (app, controller) = controller_asking_within("10s").await;
  let (to_socket, mut from_socket, _served) =
    log_in(&controller, [127, 0, 0, 1].into(), SECRET).await;
  agent_ready(&controller).await;
  let sending = tokio::spawn({
    let uploads = app.state.uploads.clone();
    async move { uploads.send("alice", vec![AGENT_FILE.to_string()]).await }
  });
  let accepting = file_info_request(&mut from_socket).await;
  to_socket.send(file_info(accepting, true, 5)).await.unwrap();
  assert_eq!(sending.await.unwrap().queued, [AGENT_FILE]);
  // the task asks again as it starts, and the agent leaves before it answers
  file_info_request(&mut from_socket).await;
  assert!(controller.kick("nas"));

  let ended = tokio::time::timeout(Duration::from_secs(10), async {
    loop {
      let (_, body) = app
        .call("GET", "/api/v0/transfers/uploads/alice", None)
        .await;
      let user: api_types::transfers::UserResponse = serde_json::from_value(body).unwrap();
      let ended = user
        .directories
        .into_iter()
        .flat_map(|d| d.files)
        .find(|f| f.ended_at.is_some());
      if let Some(ended) = ended {
        return ended;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("the upload ends");
  let exception = ended.exception.unwrap();
  assert!(exception.contains("logged in to upload"), "{exception}");
}
