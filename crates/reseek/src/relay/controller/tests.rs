//! An agent's socket served without a network, on a clock the test moves: an agent whose
//! shares are being scanned is waited for past the two minutes, but not past its deferral, an
//! agent whose configuration or API key no longer admits it is disconnected, a login checked
//! against a configuration that changed meanwhile is refused, a restarted agent replaces its
//! session while another device under its name is refused, a share upload cut off partway
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
use crate::shares::Shares;
use crate::testing::app;
use reseek_relay_proto::credential::{Key, Purpose};
use reseek_relay_proto::frames;

const SECRET: &str = "nas-secret-0123456789";

/// The device `nas` logs in from.
const DEVICE: Uuid = Uuid::from_u128(0x1a2b3c4d_0000_4000_8000_000000000001);

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
  let schema = Some(Shares::schema_version());
  let (to_socket, mut from_socket, served) =
    send_login(controller, address, secret, schema, Some(DEVICE), admitted).await;
  assert!(matches!(
    read(&written(&mut from_socket)[0]),
    RelayMessage::LoginOk { .. }
  ));
  (to_socket, from_socket, served)
}

/// A socket of `controller`'s from `address`, served on a task of its own, that answered its
/// challenge as `nas` with `secret`, the share schema `shares_schema` and the device id
/// `device_id`, opened with a credential that lets the agent in while `admitted` holds.
async fn send_login(
  controller: &Arc<Controller>,
  address: IpAddr,
  secret: &str,
  shares_schema: Option<u32>,
  device_id: Option<Uuid>,
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
    shares_schema,
    device_id,
    capabilities: Vec::new(),
  };
  to_socket.send(text(login)).await.unwrap();
  settle().await;
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
  let begun = controller.begin_shares("nas", socket, Shares::schema_version(), 1024);
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
  let schema = Shares::schema_version();
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
      device: None,
    },
    outbound,
    address: home,
    secret: SECRET.into(),
    key: Key::derive(SECRET, "nas"),
    device: None,
    left: tokio_util::sync::CancellationToken::new(),
  };
  assert_eq!(
    controller.register(registration),
    Err(LoginFailure::UnknownAgent)
  );
  assert!(controller.agents().is_empty());
}

/// A login on a share schema other than the controller's is refused for it; one that names no
/// schema, from an agent built before logins carried it, is let in for its share upload to be
/// checked.
#[tokio::test]
async fn a_login_on_another_share_schema_is_refused_for_it() {
  let (_app, controller) = controller().await;
  let home: IpAddr = [127, 0, 0, 1].into();
  let admitted = Arc::new(AtomicBool::new(true));
  let other = Some(Shares::schema_version() + 1);
  let (_to_socket, mut from_socket, served) = send_login(
    &controller,
    home,
    SECRET,
    other,
    Some(DEVICE),
    admitted.clone(),
  )
  .await;
  let answered = written(&mut from_socket);
  assert_eq!(
    read(&answered[0]),
    RelayMessage::LoginFailed {
      reason: LoginFailure::SchemaMismatch
    }
  );
  assert!(
    matches!(&answered[1], Message::Close(Some(f)) if f.code == frames::close::UNAUTHORIZED),
    "{answered:?}"
  );
  served.await.unwrap();
  assert!(controller.agents().is_empty());

  let (_to_socket, mut from_socket, _served) =
    send_login(&controller, home, SECRET, None, Some(DEVICE), admitted).await;
  assert!(matches!(
    read(&written(&mut from_socket)[0]),
    RelayMessage::LoginOk { .. }
  ));
}

/// A login as `nas` from `device`, and how the controller answered it.
async fn log_in_from(controller: &Arc<Controller>, device: Option<Uuid>) -> (Served, Vec<Message>) {
  let home: IpAddr = [127, 0, 0, 1].into();
  let admitted = Arc::new(AtomicBool::new(true));
  let schema = Some(Shares::schema_version());
  let (to_socket, mut from_socket, served) =
    send_login(controller, home, SECRET, schema, device, admitted).await;
  let answered = written(&mut from_socket);
  ((to_socket, from_socket, served), answered)
}

/// The relay's warnings among the lines `logs` was sent since the last look.
fn warnings(
  logs: &mut tokio::sync::broadcast::Receiver<Arc<api_types::logs::LogRecord>>,
) -> Vec<String> {
  std::iter::from_fn(|| logs.try_recv().ok())
    .filter(|r| r.level == api_types::logs::LogLevel::Warning && r.context.contains("relay"))
    .map(|r| r.message.clone())
    .collect()
}

/// An agent that logs in again from its device, as after a restart its controller has not
/// noticed yet, replaces its earlier session without a warning; another device logging in under
/// its name is refused for it, warned about once while it keeps trying and listed in the state
/// until it stops or the first one leaves, and then gets in.
#[tokio::test]
async fn a_restarted_agent_replaces_its_session_and_another_device_is_refused() {
  let (app, controller) = controller().await;
  let (buffer, _capturing) = crate::testing::captured_logs();
  let mut logs = buffer.subscribe();
  let ((_first, mut first, _), answered) = log_in_from(&controller, Some(DEVICE)).await;
  assert!(matches!(read(&answered[0]), RelayMessage::LoginOk { .. }));

  let ((_restarted, mut restarted, _), answered) = log_in_from(&controller, Some(DEVICE)).await;
  assert!(matches!(read(&answered[0]), RelayMessage::LoginOk { .. }));
  assert_eq!(closed(&mut first), Some(Some(frames::close::SUPERSEDED)));
  assert_eq!(warnings(&mut logs), Vec::<String>::new());
  let agents = controller.agents();
  assert_eq!(agents.len(), 1);
  assert_eq!(agents[0].device.as_deref(), Some("1a2b3c4d"));

  let other = Uuid::from_u128(0x5e6f7a8b_0000_4000_8000_000000000002);
  for attempt in 0..2 {
    let ((_, _, served), answered) = log_in_from(&controller, Some(other)).await;
    assert_eq!(
      read(&answered[0]),
      RelayMessage::LoginFailed {
        reason: LoginFailure::AnotherDevice
      },
      "attempt {attempt}"
    );
    assert!(
      matches!(&answered[1], Message::Close(Some(f)) if f.code == frames::close::UNAUTHORIZED),
      "{answered:?}"
    );
    served.await.unwrap();
  }
  let warned = warnings(&mut logs);
  assert_eq!(warned.len(), 1, "{warned:?}");
  assert!(
    warned[0].contains("another device is connected as this agent")
      && warned[0].contains(&other.to_string()),
    "{warned:?}"
  );
  assert_eq!(closed(&mut restarted), None, "the connected agent stays");
  let state = app.state.state.current();
  let refused = &state.relay.refused_devices;
  assert_eq!(refused.len(), 1);
  assert_eq!(
    (refused[0].entry.as_str(), refused[0].name.as_str()),
    ("nas", "nas")
  );
  assert_eq!(refused[0].device, "5e6f7a8b");

  // a device that stopped dialing is forgotten, and warned about again once it is back
  for refused in lock(&controller.registry).refused_devices.values_mut() {
    refused.at -= super::REFUSED_LISTED;
  }
  controller.forget_refused_devices();
  assert!(app.state.state.current().relay.refused_devices.is_empty());
  let (_, answered) = log_in_from(&controller, Some(other)).await;
  assert_eq!(
    read(&answered[0]),
    RelayMessage::LoginFailed {
      reason: LoginFailure::AnotherDevice
    }
  );
  assert_eq!(warnings(&mut logs).len(), 1);
  assert_eq!(app.state.state.current().relay.refused_devices.len(), 1);

  assert!(controller.kick("nas"));
  assert!(app.state.state.current().relay.refused_devices.is_empty());
  let (_, answered) = log_in_from(&controller, Some(other)).await;
  assert!(matches!(read(&answered[0]), RelayMessage::LoginOk { .. }));
  assert_eq!(controller.agents()[0].device.as_deref(), Some("5e6f7a8b"));
}

/// A login that gives no device id, from an agent built before logins carried one, replaces
/// the session under its name as any login did then, and one with an id replaces it in turn.
#[tokio::test]
async fn a_login_without_a_device_id_replaces_the_session_under_its_name() {
  let (_app, controller) = controller().await;
  let ((_first, mut first, _), _) = log_in_from(&controller, Some(DEVICE)).await;
  let ((_older, mut older, _), answered) = log_in_from(&controller, None).await;
  assert!(matches!(read(&answered[0]), RelayMessage::LoginOk { .. }));
  assert_eq!(closed(&mut first), Some(Some(frames::close::SUPERSEDED)));
  let (_, answered) = log_in_from(&controller, Some(DEVICE)).await;
  assert!(matches!(read(&answered[0]), RelayMessage::LoginOk { .. }));
  assert_eq!(closed(&mut older), Some(Some(frames::close::SUPERSEDED)));
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

/// The configuration of a controller with the agent `nas` configured, which has `file_info` to
/// say whether it has a file, and loopback trusted for the API.
fn controller_config(file_info: &str) -> String {
  format!(
    "[relay]\nenabled = true\nmode = \"controller\"\n\
     [relay.controller_timeouts]\nfile_info = \"{file_info}\"\n\
     [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"{SECRET}\"\n\
     [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n"
  )
}

/// A controller of [`controller_config`], not connected.
async fn controller_asking_within(file_info: &str) -> (crate::testing::TestApp, Arc<Controller>) {
  let app = app(&controller_config(file_info), &[]).await;
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

/// An upload accepted while its agent had the file, whose agent says the file is gone when the
/// upload's task asks again, fails, and alice, who asked for it, is told it is not shared, the
/// answer her request would get now, so she stops waiting for it.
#[tokio::test]
async fn an_upload_whose_agent_lost_the_file_before_its_task_starts_is_denied_to_its_peer() {
  use soulseek_proto::frame::FrameKind;
  use soulseek_proto::peer::UploadDenied;
  use soulseek_proto::server::{
    GetPeerAddressResponse, UserStats, UserStatus, WatchUserResponse, WatchedUser,
  };
  use soulseek_proto::{InitMessage, PeerMessage, ServerRequest, ServerResponse, UserPresence};

  use crate::testing::soulseek::{Server, next_frame};

  let app = crate::testing::soulseek::app(&controller_config("10s")).await;
  let controller = app.state.relay.controller.clone().unwrap();
  // alice's address, where the daemon's connection to her is read
  let alice = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = alice.local_addr().unwrap().port();
  let server = Server::start(move |request| match request {
    ServerRequest::GetPeerAddress(asked) => {
      vec![ServerResponse::GetPeerAddress(GetPeerAddressResponse {
        username: asked.username.clone(),
        ip: std::net::Ipv4Addr::LOCALHOST,
        port: port.into(),
        obfuscation: None,
      })]
    }
    ServerRequest::GetStatus(asked) => vec![ServerResponse::GetStatus(UserStatus {
      username: asked.username.clone(),
      presence: UserPresence::Online,
      is_privileged: false,
    })],
    ServerRequest::WatchUser(asked) => vec![ServerResponse::WatchUser(WatchUserResponse {
      username: asked.username.clone(),
      user: Some(WatchedUser {
        status: UserPresence::Online,
        stats: UserStats::default(),
        country_code: None,
      }),
    })],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let (to_socket, mut from_socket, _served) =
    log_in(&controller, [127, 0, 0, 1].into(), SECRET).await;
  agent_ready(&controller).await;

  let queued = {
    let asking = asked(&app);
    tokio::pin!(asking);
    let id = tokio::select! {
      id = file_info_request(&mut from_socket) => id,
      answer = &mut asking => panic!("answered before the agent: {answer:?}"),
    };
    to_socket.send(file_info(id, true, 5)).await.unwrap();
    asking.await
  };
  assert!(queued.is_ok(), "{queued:?}");
  // the task asks again as it starts, and the agent no longer has the file
  let starting = file_info_request(&mut from_socket).await;
  to_socket.send(file_info(starting, false, 0)).await.unwrap();

  let within = Duration::from_secs(10);
  let (mut stream, _) = tokio::time::timeout(within, alice.accept())
    .await
    .expect("the daemon connects to alice")
    .unwrap();
  let mut buffer = Vec::new();
  let init = next_frame(&mut stream, &mut buffer, FrameKind::Init).await;
  assert!(matches!(
    InitMessage::decode(&init).unwrap(),
    InitMessage::PeerInit(_)
  ));
  let told = tokio::time::timeout(
    within,
    next_frame(&mut stream, &mut buffer, FrameKind::Peer),
  )
  .await
  .expect("alice is told");
  assert_eq!(
    PeerMessage::decode(&told).unwrap(),
    PeerMessage::UploadDenied(UploadDenied {
      filename: AGENT_FILE.into(),
      reason: "File not shared.".into(),
    })
  );
  let (_, body) = app
    .call("GET", "/api/v0/transfers/uploads/alice", None)
    .await;
  let user: api_types::transfers::UserResponse = serde_json::from_value(body).unwrap();
  let ended = user.directories.into_iter().flat_map(|d| d.files).next();
  assert_eq!(
    ended.and_then(|t| t.exception).as_deref(),
    Some("File could not be found"),
    "the upload is failed before alice is told"
  );
}
