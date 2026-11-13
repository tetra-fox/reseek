//! The daemon against a soulfind server: it logs in from its options, the state says so, it
//! tells the server what it shares, it disconnects and connects when asked, the watchdog brings
//! it back after the server went away, and an option change that needs a reconnect is flagged
//! until one happens.

mod support;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use api_types::options::OptionsPatch;
use api_types::state::ServerConnectionState;
use serde_json::json;
use support::soulfind;
use support::{ACCOUNT, Daemon, PATIENCE, daemon, logged_in, quiet, signed_in, until};

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
  assert_eq!(state.server.state, ServerConnectionState::LoggedIn);
  assert!(state.server.address.is_some() && state.server.ip_end_point.is_some());
  assert!(
    !state.connection_watchdog.is_enabled,
    "stopped once connected"
  );
  assert_eq!(state.shares.files, 2);

  // the server learned what we share, which another user reads from it
  let watcher = server.login("watcher", quiet()).await;
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
  // not connected already holds while the session is torn down
  let state = until(&client, "disconnected", |s| {
    s.server.state == ServerConnectionState::Disconnected
  })
  .await;
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
async fn a_refused_login_is_not_tried_again() {
  let Some(server) = soulfind::server() else {
    return;
  };
  // the account exists once someone logged in with it
  let owner = server.login("taken", quiet()).await;
  let dir = tempfile::tempdir().unwrap();
  let address = server.address().port().to_string();
  let listen = support::free_port().to_string();
  let daemon = Daemon::start(
    dir.path(),
    &[
      "--slsk-address",
      "127.0.0.1",
      "--slsk-port",
      &address,
      "--slsk-username",
      &owner.username,
      "--slsk-password",
      "not the password",
      "--slsk-listen-ip-address",
      "127.0.0.1",
      "--slsk-listen-port",
      &listen,
    ],
    &ACCOUNT,
  );
  let client = signed_in(&daemon).await;
  until(&client, "refused and stopped", |s| {
    !s.server.is_connected
      && !s.connection_watchdog.is_enabled
      && !s.connection_watchdog.is_attempting_connection
  })
  .await;
  tokio::time::sleep(Duration::from_millis(500)).await;
  let state = client.application().await.unwrap();
  assert!(!state.server.is_connected && !state.connection_watchdog.is_enabled);
}

#[tokio::test]
async fn an_option_change_that_needs_it_is_flagged_until_a_reconnect() {
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
    revision: None,
  };
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
  // the session's end, which comes after it reads not connected, ends what waited for it
  until(
    &client,
    "disconnected, which ends what waited for it",
    |s| !s.server.is_connected && !s.pending_reconnect,
  )
  .await;
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
