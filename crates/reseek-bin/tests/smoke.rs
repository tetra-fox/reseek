//! The daemon's smoke run, driven through its API alone. Against soulfind, with the soulseek-rs
//! CLI as the other user, it logs in, shares, searches, browses, downloads and uploads; run by
//! `just e2e`. Against the real Soulseek server it logs in, searches, browses and downloads one
//! small file; run by hand with `just live-smoke`, with a throwaway account in
//! `RESEEK_TEST_USERNAME` and `RESEEK_TEST_PASSWORD`, and skipped without them.

mod support;

use support::smoke;
use support::soulfind;
use support::soulseek_rs::Peer;
use support::transfers::scratch;
use support::{ACCOUNT, Daemon, daemon, free_port, signed_in};

/// Logs the run's steps where the test harness shows them: on failure, or with `--no-capture`.
fn log_steps() {
  // another test in this process may have installed it already
  let _ = tracing_subscriber::fmt()
    .with_env_filter("info")
    .with_test_writer()
    .try_init();
}

#[tokio::test]
#[ignore = "drives the soulseek-rs CLI; run by just e2e"]
async fn the_daemon_shares_searches_browses_and_transfers_with_soulseek_rs() {
  log_steps();
  let binary = std::env::var_os("SOULSEEK_RS_BIN")
    .expect("SOULSEEK_RS_BIN names the soulseek-rs CLI; run this through just e2e");
  let server = soulfind::server().expect("just e2e checks soulfind is there");
  let dir = tempfile::tempdir().unwrap();
  let share = dir.path().join("share");
  std::fs::create_dir_all(&share).unwrap();
  let name = soulfind::username("smoke");
  let running = daemon(
    dir.path(),
    &server,
    &name,
    &[
      "--shared",
      share.to_str().unwrap(),
      // soulfind gives the other user this machine's LAN address for us, not loopback
      "--slsk-listen-ip-address",
      "0.0.0.0",
    ],
  );
  let api = signed_in(&running).await;
  let peer = Peer {
    binary: binary.into(),
    server: server.address(),
    username: soulfind::username("rsuser"),
    password: soulfind::PASSWORD.to_string(),
    listen_port: free_port(),
    root: scratch(),
  };
  smoke::local(&api, &name, &share, &peer).await;
}

/// `name`'s value, when it is set and not empty.
fn variable(name: &str) -> Option<String> {
  std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[tokio::test]
#[ignore = "logs in to the real Soulseek server; run by hand with just live-smoke"]
async fn the_daemon_downloads_a_small_file_on_the_real_network() {
  let (Some(username), Some(password)) = (
    variable("RESEEK_TEST_USERNAME"),
    variable("RESEEK_TEST_PASSWORD"),
  ) else {
    eprintln!("skipped: RESEEK_TEST_USERNAME and RESEEK_TEST_PASSWORD name no throwaway account");
    return;
  };
  log_steps();
  let query = variable("RESEEK_TEST_QUERY").unwrap_or_else(|| "folder jpg".to_string());
  let max_size = variable("RESEEK_TEST_MAX_SIZE").map_or(1 << 20, |size| {
    size
      .parse()
      .expect("RESEEK_TEST_MAX_SIZE is a number of bytes")
  });
  let listen = variable("RESEEK_TEST_LISTEN_PORT").unwrap_or_else(|| "50000".to_string());
  let mut args = vec!["--slsk-listen-port", listen.as_str()];
  let server = variable("RESEEK_TEST_SERVER");
  let address = server.as_deref().map(|server| {
    server
      .rsplit_once(':')
      .expect("RESEEK_TEST_SERVER is host:port")
  });
  if let Some((host, port)) = address {
    args.extend(["--slsk-address", host, "--slsk-port", port]);
  }
  let mut env = ACCOUNT.to_vec();
  env.extend([
    ("RESEEK_SLSK_USERNAME", username.as_str()),
    ("RESEEK_SLSK_PASSWORD", password.as_str()),
  ]);
  let dir = tempfile::tempdir().unwrap();
  let running = Daemon::start(dir.path(), &args, &env);
  let api = signed_in(&running).await;
  smoke::live(&api, &query, max_size).await;
}
