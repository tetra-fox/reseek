//! Relay mode between two daemons: an agent logs in to its controller, uploads its shares, and
//! a third user on soulfind downloads one of the agent's files from the controller with the
//! bytes intact; a kicked agent's shares go with it and it comes back on its own, as it does
//! after its controller restarts; an agent that stops takes its shares with it. The controller
//! refuses a wrong secret, an unknown name, a replayed credential and an address outside the
//! agent's networks.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use api_client::{Client, Credential, RelayReceived};
use api_types::relay::{
  LoginFailure, RELAY_PROTOCOL, RelayConnectionState, RelayEnvelope, RelayMessage,
};
use api_types::state::State;
use reseek::relay::credential::{Key, Purpose};
use soulseek_client::{ClientOptions, DownloadOutput, DownloadRequest};
use support::soulfind::{self, Soulfind};
use support::transfers::{content, file, scratch};
use support::{ACCOUNT, Daemon, PATIENCE, daemon_on, free_port, signed_in, until};
use tokio_util::sync::CancellationToken;

const KEY: &str = "agents-key-0123456789";
const SECRET: &str = "nas-secret-0123456789";

/// The controller's relay configuration: the agent `nas`, and `fenced`, which may connect only
/// from 10.0.0.0/8.
const CONTROLLER: &str = "[relay]\nenabled = true\nmode = \"controller\"\n\
  [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"nas-secret-0123456789\"\n\
  [relay.agents.fenced]\ninstance_name = \"fenced\"\nsecret = \"fenced-secret-0123456\"\n\
  cidr = \"10.0.0.0/8\"\n\
  [web.authentication.api_keys.agents]\nkey = \"agents-key-0123456789\"\nrole = \"readwrite\"\n";

/// The controller in `dir`, logged in to `server` as `name`, serving HTTP on `port`.
fn controller(dir: &Path, server: &Soulfind, name: &str, port: u16) -> Daemon {
  std::fs::write(dir.join("config.toml"), CONTROLLER).unwrap();
  daemon_on(port, dir, server, name, free_port(), &[])
}

/// The agent `nas` in `dir`, sharing `dir/share`, connecting to the controller on `port`.
fn agent(dir: &Path, port: u16) -> Daemon {
  let config = format!(
    "instance_name = \"nas\"\n[relay]\nenabled = true\nmode = \"agent\"\n\
     [relay.controller]\naddress = \"http://127.0.0.1:{port}/\"\napi_key = \"{KEY}\"\n\
     secret = \"{SECRET}\"\n[shares]\ndirectories = [\"{}\"]\n",
    dir.join("share").display()
  );
  std::fs::write(dir.join("config.toml"), config).unwrap();
  Daemon::start(dir, &[], &ACCOUNT)
}

/// Whether the controller serves the agent `nas` with `files` files.
fn serving(state: &State, files: u32) -> bool {
  state
    .relay
    .agents
    .iter()
    .any(|a| a.name == "nas" && a.ready)
    && state.shares.files == files
}

/// The number of events of `kind` in the log once it holds exactly `count`, which it must
/// still hold a moment later.
async fn exactly(api: &Client, kind: &str, count: usize) {
  support::events(api, kind, count).await;
  tokio::time::sleep(Duration::from_millis(300)).await;
  let all = support::events(api, kind, 0).await;
  assert_eq!(all.len(), count, "{kind}: {all:?}");
}

#[tokio::test]
async fn an_agents_file_is_downloaded_through_its_controller() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  let size = 150_000;
  let album = agent_dir.path().join("share").join("album");
  std::fs::create_dir_all(&album).unwrap();
  file(&album, "one.flac", size);
  let name = soulfind::username("controller");
  let port = free_port();
  let controller = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&controller).await;
  until(&api, "logged in", support::logged_in).await;
  let agent = agent(agent_dir.path(), port);
  let agent_api = signed_in(&agent).await;
  let state = until(&api, "serving the agent", |s| serving(s, 1)).await;
  assert_eq!(
    state.shares.hosts.as_deref(),
    Some(&["local".to_string(), "nas".to_string()][..])
  );
  assert_eq!(state.relay.agents[0].ip_address, "127.0.0.1");
  until(&agent_api, "ready", |s| {
    s.relay.controller.state == RelayConnectionState::Ready
  })
  .await;
  exactly(&api, "RelayAgentConnected", 1).await;
  exactly(&api, "RelayAgentSharesUpdated", 1).await;
  exactly(&api, "RelayAgentDisconnected", 0).await;

  let options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let other = server.login("other", options).await;
  let into = scratch().join("relayed");
  let request = DownloadRequest::new(
    &name,
    "share\\album\\one.flac",
    size as u64,
    DownloadOutput::File(into.clone()),
  );
  let done = tokio::time::timeout(
    PATIENCE,
    other.client.download(request, &CancellationToken::new()),
  )
  .await
  .expect("the download ends")
  .unwrap();
  assert_eq!(done.bytes_transferred, size as u64);
  assert_eq!(std::fs::read(&into).unwrap(), content(size));

  api.kick_agent("nas").await.unwrap();
  let dropped = until(&api, "the agent's shares dropped", |s| {
    s.shares.files == 0 && s.relay.agents.is_empty()
  })
  .await;
  assert_eq!(dropped.shares.hosts.unwrap(), ["local"]);
  exactly(&api, "RelayAgentDisconnected", 1).await;
  until(&api, "serving the agent again", |s| serving(s, 1)).await;
  exactly(&api, "RelayAgentConnected", 2).await;
}

#[tokio::test]
async fn an_agent_comes_back_after_its_controller_restarts_and_leaves_with_its_shares() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  let album = agent_dir.path().join("share").join("album");
  std::fs::create_dir_all(&album).unwrap();
  file(&album, "one.flac", 1_000);
  file(&album, "two.flac", 1_000);
  let name = soulfind::username("controller");
  let port = free_port();
  let first = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&first).await;
  let mut agent = agent(agent_dir.path(), port);
  let agent_api = signed_in(&agent).await;
  until(&api, "serving the agent", |s| serving(s, 2)).await;

  drop(first);
  until(&agent_api, "reconnecting", |s| {
    s.relay.controller.state != RelayConnectionState::Ready
  })
  .await;
  let second = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&second).await;
  until(&api, "serving the agent after the restart", |s| {
    serving(s, 2)
  })
  .await;
  until(&agent_api, "ready again", |s| {
    s.relay.controller.state == RelayConnectionState::Ready
  })
  .await;

  agent.child.kill().unwrap();
  agent.child.wait().unwrap();
  let left = until(&api, "the agent's shares dropped", |s| {
    s.relay.agents.is_empty() && s.shares.files == 0
  })
  .await;
  assert_eq!(left.shares.hosts.unwrap(), ["local"]);
}

/// The next message on `socket`.
async fn message(socket: &mut api_client::RelaySocket) -> RelayMessage {
  let received = tokio::time::timeout(PATIENCE, socket.receive())
    .await
    .expect("the controller answers")
    .unwrap();
  match received {
    RelayReceived::Message(envelope) => envelope.message,
    closed => panic!("closed instead: {closed:?}"),
  }
}

/// A socket of `api`'s, logged in as `agent` with a credential made with `secret`, or for the
/// token `instead` when given; how the controller answered.
async fn log_in(
  api: &Client,
  agent: &str,
  secret: &str,
  instead: Option<&str>,
) -> (api_client::RelaySocket, RelayMessage, String) {
  let mut socket = api.relay_socket().await.unwrap();
  let RelayMessage::Challenge { token, .. } = message(&mut socket).await else {
    panic!("no challenge");
  };
  let credential = Key::derive(secret, agent).credential(Purpose::Login, instead.unwrap_or(&token));
  let login = RelayEnvelope {
    v: RELAY_PROTOCOL,
    id: None,
    message: RelayMessage::Login {
      agent: agent.to_string(),
      credential,
      version: "test".to_string(),
      capabilities: Vec::new(),
    },
  };
  socket.send(&login).await.unwrap();
  let answer = message(&mut socket).await;
  (socket, answer, token)
}

/// Whether the login was refused for `reason` and the socket then closed as unauthorized.
async fn refused(api: &Client, agent: &str, secret: &str, instead: Option<&str>) -> LoginFailure {
  let (mut socket, answer, _) = log_in(api, agent, secret, instead).await;
  let RelayMessage::LoginFailed { reason } = answer else {
    panic!("accepted: {answer:?}");
  };
  assert_eq!(
    socket.receive().await.unwrap(),
    RelayReceived::Closed(Some(4003))
  );
  reason
}

#[tokio::test]
async fn a_login_is_refused_for_each_wrong_part() {
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(dir.path().join("config.toml"), CONTROLLER).unwrap();
  let controller = Daemon::start(dir.path(), &[], &ACCOUNT);
  let api = Client::new(&controller.url())
    .unwrap()
    .with_credential(Credential::ApiKey(KEY.to_string()));

  let wrong = refused(&api, "nas", "not-the-secret-0123", None).await;
  assert_eq!(wrong, LoginFailure::BadCredential);
  let unknown = refused(&api, "stranger", SECRET, None).await;
  assert_eq!(unknown, LoginFailure::UnknownAgent);
  let outside = refused(&api, "fenced", "fenced-secret-0123456", None).await;
  assert_eq!(outside, LoginFailure::AddressNotAllowed);

  let (_logged_in, answer, used) = log_in(&api, "nas", SECRET, None).await;
  assert!(matches!(answer, RelayMessage::LoginOk { .. }), "{answer:?}");
  let replayed = refused(&api, "nas", SECRET, Some(&used)).await;
  assert_eq!(replayed, LoginFailure::BadCredential);

  let admin = signed_in(&controller).await;
  let started = Instant::now();
  while admin.relay_agents().await.unwrap().len() != 1 {
    assert!(started.elapsed() < PATIENCE, "the accepted login is listed");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}
