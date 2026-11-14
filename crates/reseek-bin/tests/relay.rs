//! Relay mode between the daemon and the relay agent program: an agent logs in to its
//! controller, uploads its shares, and a third user on soulfind downloads one of the agent's
//! files from the controller with the bytes intact; a kicked agent's shares go with it and it
//! comes back on its own, as it does after its controller restarts; an agent that stops takes
//! its shares with it. The controller refuses a wrong secret and a replayed credential, and an
//! agent with a read-only key as it dials. A
//! controller's finished download is copied to an agent that asks for them, a controller that
//! stops closes its agents' sockets as shutting down, and an agent stopped by a signal closes
//! its own.

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use api_client::{Client, Credential, RelayReceived};
use api_types::relay::{LoginFailure, RELAY_PROTOCOL, RelayEnvelope, RelayMessage};
use api_types::state::State;
use api_types::transfers::{TransferDirection, TransferOutcome, TransferStates};
use reseek_relay_proto::credential::{Key, Purpose};
use soulseek_client::{
  ClientError, ClientOptions, DownloadOutput, DownloadRequest, UploadInput, UploadRequest,
};
use support::soulfind::{self, Soulfind};
use support::transfers::{self, asked, content, file, scratch};
use support::{
  ACCOUNT, AGENT, Daemon, PATIENCE, Process, daemon_on, enqueue, free_port, signed_in, until,
};
use tokio_util::sync::CancellationToken;

const KEY: &str = "agents-key-0123456789";
const SECRET: &str = "nas-secret-0123456789";

/// The controller's relay configuration: the agent `nas`.
const CONTROLLER: &str = "[relay]\nenabled = true\nmode = \"controller\"\n\
  [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"nas-secret-0123456789\"\n\
  [web.authentication.api_keys.agents]\nkey = \"agents-key-0123456789\"\nrole = \"readwrite\"\n";

/// The controller in `dir`, logged in to `server` as `name`, serving HTTP on `port`.
fn controller(dir: &Path, server: &Soulfind, name: &str, port: u16) -> Daemon {
  std::fs::write(dir.join("config.toml"), CONTROLLER).unwrap();
  daemon_on(port, dir, server, name, free_port(), &[])
}

/// The relay agent program `nas` in `dir`, sharing `dir/share`, connecting to the controller
/// on `port`, with `extra` in its `[relay.controller]` table.
fn agent(dir: &Path, port: u16, extra: &str) -> Process {
  let config = format!(
    "instance_name = \"nas\"\n\
     [relay.controller]\naddress = \"http://127.0.0.1:{port}/\"\napi_key = \"{KEY}\"\n\
     secret = \"{SECRET}\"\n{extra}[shares]\ndirectories = [\"{}\"]\n",
    dir.join("share").display()
  );
  std::fs::write(dir.join("config.toml"), config).unwrap();
  Process::spawn(
    std::process::Command::new(AGENT)
      .args(["--app-dir", dir.to_str().unwrap()])
      .env_clear(),
  )
}

/// The line the agent logs once the controller took its shares.
const READY: &[&str] = &["Shares uploaded. Ready to relay files."];

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

/// The value of the unlabeled metric `name`.
async fn value_of(api: &Client, name: &str) -> f64 {
  api.metrics().await.unwrap()[name].samples.as_ref().unwrap()[0].value
}

/// The agent program logs in to a daemon controller over loopback, uploads its share, and
/// serves one of its files to a peer downloading it from the controller.
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
  let agent = agent(agent_dir.path(), port, "");
  let state = until(&api, "serving the agent", |s| serving(s, 1)).await;
  assert_eq!(
    state.shares.hosts.as_deref(),
    Some(&["local".to_string(), "nas".to_string()][..])
  );
  assert_eq!(state.relay.agents[0].ip_address, "127.0.0.1");
  assert_eq!(state.relay.agents[0].version, env!("CARGO_PKG_VERSION"));
  let ready = agent.printed_line(READY).await;
  // the daemon's line: `[time LVL] [module] spans: message`
  assert!(
    ready.starts_with('[') && ready.contains(" INF] [reseek_agent::client::sync] "),
    "{ready}"
  );
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
  let metrics = api.metrics().await.unwrap();
  let value = |name: &str| metrics[name].samples.as_ref().unwrap()[0].value;
  assert_eq!(value("reseek_relay_agents_connected"), 1.0);
  let requests = metrics["reseek_relay_requests_total"]
    .samples
    .clone()
    .unwrap();
  let counted = |kind: &str, outcome: &str| {
    requests.iter().any(|sample| {
      let labels = sample.labels.as_ref().unwrap();
      labels["type"] == kind && labels["outcome"] == outcome && sample.value >= 1.0
    })
  };
  assert!(
    counted("login", "accepted") && counted("shares", "loaded"),
    "{requests:?}"
  );
  assert!(
    counted("file_info", "answered") && counted("file", "streamed"),
    "{requests:?}"
  );
  let started = Instant::now();
  while value_of(&api, "reseek_relay_uploads_in_flight").await != 0.0 {
    assert!(started.elapsed() < PATIENCE, "the relayed upload ends");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }

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
  let mut agent = agent(agent_dir.path(), port, "");
  until(&api, "serving the agent", |s| serving(s, 2)).await;

  drop(first);
  agent
    .printed_line(&["relay controller connection lost"])
    .await;
  let second = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&second).await;
  until(&api, "serving the agent after the restart", |s| {
    serving(s, 2)
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

/// A peer asking for a file an agent had when it left, while it is away but still configured, is
/// told the upload failed, which it takes as a reason to ask again, across a restart of the
/// controller too, and gets the file once the agent is back; a file the agent never had is not
/// shared, and neither is any once the agent is no longer configured.
#[tokio::test]
async fn an_away_agents_file_is_failed_until_the_agent_is_back_or_no_longer_configured() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  let album = agent_dir.path().join("share").join("album");
  std::fs::create_dir_all(&album).unwrap();
  file(&album, "one.flac", 1_000);
  let name = soulfind::username("controller");
  let port = free_port();
  let first = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&first).await;
  until(&api, "logged in", support::logged_in).await;
  let mut away = agent(agent_dir.path(), port, "");
  until(&api, "serving the agent", |s| serving(s, 1)).await;
  away.child.kill().unwrap();
  away.child.wait().unwrap();
  until(&api, "the agent gone", |s| s.relay.agents.is_empty()).await;

  let options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let other = server.login("other", options).await;
  let into = scratch().join("away");
  let ask = |filename: &'static str| {
    let request = DownloadRequest::new(&name, filename, 1_000, DownloadOutput::File(into.clone()));
    let client = other.client.clone();
    async move {
      tokio::time::timeout(
        PATIENCE,
        client.download(request, &CancellationToken::new()),
      )
      .await
      .expect("the download ends")
    }
  };
  let theirs = "share\\album\\one.flac";
  let failed =
    |answer: &Result<_, ClientError>| matches!(answer, Err(ClientError::TransferReportedFailed(_)));
  let not_shared = |answer: &Result<_, ClientError>| matches!(answer, Err(ClientError::TransferRejected(reason)) if reason == "File not shared.");
  let answer = ask(theirs).await;
  assert!(failed(&answer), "{answer:?}");
  let answer = ask("share\\album\\never.flac").await;
  assert!(not_shared(&answer), "{answer:?}");

  drop(first);
  let second = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&second).await;
  until(&api, "logged in again", support::logged_in).await;
  let answer = ask(theirs).await;
  assert!(failed(&answer), "after the restart: {answer:?}");

  let _back = agent(agent_dir.path(), port, "");
  until(&api, "serving the agent again", |s| serving(s, 1)).await;
  let done = ask(theirs).await.unwrap();
  assert_eq!(done.bytes_transferred, 1_000);
  assert_eq!(std::fs::read(&into).unwrap(), content(1_000));

  let unconfigured = CONTROLLER.replace(
    "[relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"nas-secret-0123456789\"\n",
    "",
  );
  assert_ne!(unconfigured, CONTROLLER);
  std::fs::write(controller_dir.path().join("config.toml"), unconfigured).unwrap();
  until(&api, "the unconfigured agent gone", |s| {
    s.relay.agents.is_empty()
  })
  .await;
  let answer = ask(theirs).await;
  assert!(not_shared(&answer), "{answer:?}");
}

/// The files under `dir` that hold the index of the agent `nas`.
fn kept_indexes(dir: &Path) -> Vec<std::path::PathBuf> {
  let mut found = Vec::new();
  for entry in std::fs::read_dir(dir).unwrap() {
    let path = entry.unwrap().path();
    if path.is_dir() {
      found.extend(kept_indexes(&path));
    } else if path
      .file_name()
      .is_some_and(|name| name.to_string_lossy().starts_with("shares.nas."))
    {
      found.push(path);
    }
  }
  found
}

/// A daemon started with relay mode off deletes the index an earlier run kept for an agent
/// that was away.
#[tokio::test]
async fn a_daemon_without_relay_mode_deletes_the_kept_agent_indexes() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  let album = agent_dir.path().join("share").join("album");
  std::fs::create_dir_all(&album).unwrap();
  file(&album, "one.flac", 1_000);
  let name = soulfind::username("controller");
  let port = free_port();
  let first = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&first).await;
  let mut away = agent(agent_dir.path(), port, "");
  until(&api, "serving the agent", |s| serving(s, 1)).await;
  away.child.kill().unwrap();
  away.child.wait().unwrap();
  until(&api, "the agent gone", |s| s.relay.agents.is_empty()).await;
  assert!(!kept_indexes(controller_dir.path()).is_empty());

  drop(first);
  let off = CONTROLLER.replace("enabled = true", "enabled = false");
  assert_ne!(off, CONTROLLER);
  std::fs::write(controller_dir.path().join("config.toml"), off).unwrap();
  let _plain = daemon_on(
    port,
    controller_dir.path(),
    &server,
    &name,
    free_port(),
    &[],
  );
  let started = Instant::now();
  while !kept_indexes(controller_dir.path()).is_empty() {
    assert!(started.elapsed() < PATIENCE, "the kept index stays");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

#[cfg(unix)]
#[tokio::test]
async fn a_stopping_controller_closes_its_agents_as_shutting_down() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  std::fs::create_dir_all(agent_dir.path().join("share")).unwrap();
  let name = soulfind::username("controller");
  let port = free_port();
  let mut stopping = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&stopping).await;
  let agent = agent(agent_dir.path(), port, "");
  until(&api, "serving the agent", |s| serving(s, 0)).await;
  until(&api, "logged in", support::logged_in).await;
  let pid = stopping.process.child.id().to_string();
  let signaled = std::process::Command::new("kill")
    .args(["-TERM", &pid])
    .status()
    .unwrap();
  assert!(signaled.success());
  agent
    .printed_line(&["relay controller connection lost", "code=4010"])
    .await;
  assert!(stopping.process.child.wait().unwrap().success());
  let log = stopping.process.output();
  assert!(
    log.contains("the client is shutting down") && log.contains("Client stopped"),
    "{log}"
  );
}

/// The agent program, stopped by a signal, closes its socket, says so and exits cleanly, and its
/// controller lets it go.
#[cfg(unix)]
#[tokio::test]
async fn a_signal_stops_an_agent_cleanly() {
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  std::fs::write(controller_dir.path().join("config.toml"), CONTROLLER).unwrap();
  let controller = Daemon::start(controller_dir.path(), &[], &ACCOUNT);
  let api = signed_in(&controller).await;
  std::fs::create_dir_all(agent_dir.path().join("share")).unwrap();
  let mut agent = agent(agent_dir.path(), controller.port, "");
  until(&api, "serving the agent", |s| serving(s, 0)).await;
  let signaled = std::process::Command::new("kill")
    .args(["-TERM", &agent.child.id().to_string()])
    .status()
    .unwrap();
  assert!(signaled.success());
  assert!(agent.child.wait().unwrap().success());
  agent.printed_line(&["reseek-agent stopped"]).await;
  until(&api, "the agent gone", |s| s.relay.agents.is_empty()).await;
}

#[tokio::test]
async fn a_finished_download_is_copied_to_an_agent_that_asks_for_them() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  std::fs::create_dir_all(agent_dir.path().join("share")).unwrap();
  let name = soulfind::username("controller");
  let port = free_port();
  let controller = controller(controller_dir.path(), &server, &name, port);
  let api = signed_in(&controller).await;
  until(&api, "logged in", support::logged_in).await;
  let agent = agent(
    agent_dir.path(),
    port,
    "downloads = true\ndownloads_filter = [\"**/*.flac\"]\n",
  );
  until(&api, "serving the agent", |s| serving(s, 0)).await;

  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let mut requests = transfers::queueing(&mut options);
  let uploader = server.login("up", options).await;
  let size = 120_000;
  let source = scratch().join("song.flac");
  std::fs::write(&source, content(size)).unwrap();
  let remote = "@@share\\album\\01 song.flac";
  let id = enqueue(&api, &uploader.username, remote, size as u64).await;
  let (username, filename) = asked(&mut requests).await;
  let client = uploader.client.clone();
  tokio::spawn(async move {
    let request = UploadRequest::new(username, filename, UploadInput::File(source));
    client.upload(request, &CancellationToken::new()).await
  });
  let started = Instant::now();
  let finished = loop {
    let transfer = api
      .transfer(TransferDirection::Download, &uploader.username, id, true)
      .await
      .unwrap();
    if transfer.state == TransferStates::Completed(TransferOutcome::Succeeded) {
      break transfer;
    }
    assert!(
      started.elapsed() < PATIENCE,
      "never downloaded: {transfer:?}"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
  };
  let local = Path::new(finished.local_path.as_deref().unwrap());
  let downloads = controller_dir.path().join("data").join("downloads");
  let relative = local
    .strip_prefix(&downloads)
    .unwrap_or_else(|_| panic!("{} is not under {}", local.display(), downloads.display()));
  let copy = agent_dir
    .path()
    .join("data")
    .join("downloads")
    .join(relative);
  let started = Instant::now();
  while std::fs::read(&copy).ok() != Some(content(size)) {
    assert!(
      started.elapsed() < PATIENCE,
      "the agent never copied {}:\n{}",
      copy.display(),
      agent.printed()
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  let partial = copy.with_extension("flac.part");
  assert!(!partial.exists(), "the partial copy is renamed into place");
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

/// A socket of `api`'s, logged in as `agent` of `version` with a credential made with `secret`,
/// or for the token `instead` when given; how the controller answered.
async fn log_in(
  api: &Client,
  agent: &str,
  secret: &str,
  instead: Option<&str>,
  version: &str,
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
      version: version.to_string(),
      shares_schema: Some(reseek::shares::Shares::schema_version()),
      capabilities: Vec::new(),
    },
  };
  socket.send(&login).await.unwrap();
  let answer = message(&mut socket).await;
  (socket, answer, token)
}

/// Why the login of `agent` of `version` was refused, once the socket closed as unauthorized.
async fn refused(
  api: &Client,
  agent: &str,
  secret: &str,
  instead: Option<&str>,
  version: &str,
) -> LoginFailure {
  let (mut socket, answer, _) = log_in(api, agent, secret, instead, version).await;
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
async fn a_wrong_or_replayed_login_is_refused_and_a_right_one_listed() {
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(dir.path().join("config.toml"), CONTROLLER).unwrap();
  let controller = Daemon::start(dir.path(), &[], &ACCOUNT);
  let api = Client::new(&controller.url())
    .await
    .unwrap()
    .with_credential(Credential::ApiKey(KEY.to_string()));

  let ours = env!("CARGO_PKG_VERSION");
  let wrong = refused(&api, "nas", "not-the-secret-0123", None, ours).await;
  assert_eq!(wrong, LoginFailure::BadCredential);

  let (_logged_in, answer, used) = log_in(&api, "nas", SECRET, None, ours).await;
  assert!(matches!(answer, RelayMessage::LoginOk { .. }), "{answer:?}");
  let replayed = refused(&api, "nas", SECRET, Some(&used), ours).await;
  assert_eq!(replayed, LoginFailure::BadCredential);

  let admin = signed_in(&controller).await;
  let started = Instant::now();
  while admin.relay_agents().await.unwrap().len() != 1 {
    assert!(started.elapsed() < PATIENCE, "the accepted login is listed");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

/// An agent whose API key has only the read-only role is refused as it dials, before it logs in
/// or copies its index, and says so once.
#[tokio::test]
async fn an_agent_with_a_read_only_key_is_refused_as_it_dials() {
  let (controller_dir, agent_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
  std::fs::create_dir_all(agent_dir.path().join("share")).unwrap();
  let read_only = CONTROLLER.replace("role = \"readwrite\"", "role = \"readonly\"");
  std::fs::write(controller_dir.path().join("config.toml"), read_only).unwrap();
  let port = free_port();
  let controller = Daemon::start_on(port, controller_dir.path(), &[], &ACCOUNT);
  let agent = agent(agent_dir.path(), port, "");
  agent
    .printed_line(&["the relay controller refused this agent", "403"])
    .await;
  let printed = agent.printed();
  assert!(
    !printed.contains("Login succeeded") && !printed.contains("Beginning upload of shares"),
    "{printed}"
  );
  let admin = signed_in(&controller).await;
  assert!(admin.relay_agents().await.unwrap().is_empty());
}
