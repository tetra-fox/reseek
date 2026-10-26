//! The daemon against a soulfind server: it logs in from its options, the state says so, it
//! tells the server what it shares, it disconnects and connects when asked, the watchdog brings
//! it back after the server went away, and option changes reach the client while it runs.

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::options::OptionsPatch;
use api_types::state::State;
use serde_json::json;
use soulseek_client::ClientOptions;
use support::soulfind::{self, PASSWORD, Soulfind};
use support::{Daemon, PATIENCE};

const ACCOUNT: [(&str, &str); 2] = [("RESEEK_USERNAME", "admin"), ("RESEEK_PASSWORD", "hunter2")];

/// The daemon in `dir` logging in to `server` as `username`, with `extra` arguments.
fn daemon(dir: &Path, server: &Soulfind, username: &str, extra: &[&str]) -> Daemon {
  let address = server.address();
  let port = address.port().to_string();
  let listen = support::free_port().to_string();
  let mut args = vec![
    "--slsk-address",
    "127.0.0.1",
    "--slsk-port",
    &port,
    "--slsk-username",
    username,
    "--slsk-password",
    PASSWORD,
    "--slsk-listen-ip-address",
    "127.0.0.1",
    "--slsk-listen-port",
    &listen,
    "--remote-configuration",
  ];
  args.extend_from_slice(extra);
  Daemon::start(dir, &args, &ACCOUNT)
}

async fn signed_in(daemon: &Daemon) -> Client {
  let client = Client::new(&daemon.url()).unwrap();
  client.login("admin", "hunter2").await.unwrap();
  client
}

/// The state once `ready` holds for it.
async fn until(client: &Client, what: &str, ready: impl Fn(&State) -> bool) -> State {
  let started = Instant::now();
  loop {
    let state = client.application().await.unwrap();
    if ready(&state) {
      return state;
    }
    assert!(
      started.elapsed() < PATIENCE,
      "never {what}: {:?} {:?}",
      state.server,
      state.connection_watchdog
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

fn logged_in(state: &State) -> bool {
  state.server.is_logged_in
}

#[tokio::test]
async fn the_daemon_logs_in_with_its_shares_counted_and_connects_when_asked() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let shared = dir.path().join("music");
  std::fs::create_dir_all(shared.join("album")).unwrap();
  std::fs::write(shared.join("album").join("one.mp3"), b"one").unwrap();
  std::fs::write(shared.join("album").join("two.mp3"), b"two").unwrap();
  let name = soulfind::username("daemon");
  let daemon = daemon(
    dir.path(),
    &server,
    &name,
    &["--shared", shared.to_str().unwrap()],
  );
  let client = signed_in(&daemon).await;
  let state = until(&client, "logged in", logged_in).await;
  assert_eq!(state.user.username.as_deref(), Some(name.as_str()));
  assert_eq!(state.server.state.to_string(), "Connected, LoggedIn");
  assert!(state.server.address.is_some() && state.server.ip_end_point.is_some());
  assert!(
    !state.connection_watchdog.is_enabled,
    "stopped once connected"
  );
  assert_eq!(
    state.shares.files, 2,
    "the shares were ready before the login"
  );

  // the server learnt what we share, which another user reads from it
  let watcher = server.login("watcher", ClientOptions::default()).await;
  let started = Instant::now();
  loop {
    let statistics = watcher.client.get_user_statistics(&name).await.unwrap();
    if statistics.stats.file_count == 2 {
      assert_eq!(statistics.stats.directory_count, state.shares.directories);
      break;
    }
    assert!(started.elapsed() < PATIENCE, "{statistics:?}");
    tokio::time::sleep(Duration::from_millis(100)).await;
  }

  client
    .disconnect("client disconnected from web UI")
    .await
    .unwrap();
  let state = until(&client, "disconnected", |s| !s.server.is_connected).await;
  assert_eq!(state.server.state.to_string(), "Disconnected");
  assert_eq!(state.user.username, None);
  assert!(!state.connection_watchdog.is_enabled);
  tokio::time::sleep(Duration::from_millis(500)).await;
  assert!(
    !client.application().await.unwrap().server.is_connected,
    "a disconnect asked for is not undone"
  );

  assert_eq!(client.connect().await.unwrap().as_u16(), 200);
  until(&client, "logged in again", logged_in).await;
}

#[tokio::test]
async fn the_watchdog_reconnects_once_the_server_is_back() {
  let Some(mut server) = soulfind::own() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let daemon = daemon(dir.path(), &server, &soulfind::username("daemon"), &[]);
  let client = signed_in(&daemon).await;
  until(&client, "logged in", logged_in).await;

  server.stop();
  let waiting = until(&client, "waiting to reconnect", |s| {
    !s.server.is_connected
      && s.connection_watchdog.is_enabled
      && s.connection_watchdog.next_attempt_at.is_some()
  })
  .await;
  assert!(waiting.connection_watchdog.is_attempting_connection);
  assert!(!waiting.connection_watchdog.is_login_unanswered);

  // trying already, so it starts over at once rather than at the end of its back-off
  assert_eq!(client.connect().await.unwrap().as_u16(), 205);
  server.start_again();
  let state = until(&client, "logged in again", logged_in).await;
  assert!(!state.connection_watchdog.is_enabled);
  assert_eq!(state.connection_watchdog.next_attempt_at, None);
}

#[tokio::test]
async fn option_changes_reach_the_client_and_flag_a_reconnect_only_when_needed() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let daemon = daemon(dir.path(), &server, &soulfind::username("daemon"), &[]);
  let client = signed_in(&daemon).await;
  until(&client, "logged in", logged_in).await;

  let set = |key: &str, value: serde_json::Value| OptionsPatch {
    set: BTreeMap::from([(key.to_string(), value)]),
    unset: Vec::new(),
    expires_in: None,
  };
  client
    .patch_options(&set("transfers.upload.speed_limit", json!(64)))
    .await
    .unwrap();
  tokio::time::sleep(Duration::from_millis(500)).await;
  let state = client.application().await.unwrap();
  assert!(state.server.is_logged_in, "a live change keeps the session");
  assert!(!state.pending_reconnect);

  client
    .patch_options(&set("soulseek.server_keepalive.probes", json!(5)))
    .await
    .unwrap();
  until(&client, "flagged for a reconnect", |s| s.pending_reconnect).await;
  assert_eq!(
    client.options_view().await.unwrap().pending_reconnect,
    ["soulseek.server_keepalive.probes"]
  );

  client.disconnect("reconnecting").await.unwrap();
  let state = until(&client, "disconnected", |s| !s.server.is_connected).await;
  assert!(
    !state.pending_reconnect,
    "a disconnect ends what waited for it"
  );
  client.connect().await.unwrap();
  let state = until(&client, "logged in again", logged_in).await;
  assert!(!state.pending_reconnect);
  assert!(
    client
      .options_view()
      .await
      .unwrap()
      .pending_reconnect
      .is_empty()
  );
}

#[cfg(unix)]
#[tokio::test]
async fn a_connected_daemon_stops_on_sigterm_and_exits_zero() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let dir = tempfile::tempdir().unwrap();
  let mut daemon = daemon(dir.path(), &server, &soulfind::username("daemon"), &[]);
  let client = signed_in(&daemon).await;
  until(&client, "logged in", logged_in).await;
  let pid = daemon.child.id().to_string();
  let killed = std::process::Command::new("kill")
    .args(["-TERM", &pid])
    .status()
    .unwrap();
  assert!(killed.success());
  let started = Instant::now();
  let status = loop {
    if let Some(status) = daemon.child.try_wait().unwrap() {
      break status;
    }
    assert!(started.elapsed() < PATIENCE, "still running");
    std::thread::sleep(Duration::from_millis(50));
  };
  assert_eq!(status.code(), Some(0));
  let log = daemon.output();
  assert!(
    log.contains("the client is shutting down") && log.contains("Client stopped"),
    "{log}"
  );
}
