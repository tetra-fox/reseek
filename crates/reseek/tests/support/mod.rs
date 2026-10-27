//! Running the daemon program for a test: on a free loopback port, with its directory, and
//! stopped when the test is done with it.

#![allow(dead_code, reason = "each test binary uses a part of it")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::state::State;
use soulfind::{PASSWORD, Soulfind};
use soulseek_client::{ClientOptions, Event};
use tokio::sync::broadcast;

/// The client's test harness: the Soulseek server the network tests run against, a scripted
/// server and scripted peers for what a real one never does, and files to send.
#[path = "../../../soulseek-client/tests/support/mod.rs"]
pub mod client;

#[allow(unused_imports, reason = "each test binary uses a part of it")]
pub use client::{peer, soulfind, transfers};

/// How long a test waits for something that should happen promptly.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// The next client event `pick` accepts, skipping the others.
pub async fn next_event<T>(
  events: &mut broadcast::Receiver<Event>,
  mut pick: impl FnMut(Event) -> Option<T>,
) -> T {
  tokio::time::timeout(PATIENCE, async {
    loop {
      if let Some(picked) = pick(events.recv().await.unwrap()) {
        return picked;
      }
    }
  })
  .await
  .expect("the event is raised")
}

/// The headless daemon program.
pub const PROGRAM: &str = env!("CARGO_BIN_EXE_reseek-headless");

/// A port nothing listens on, for a moment.
pub fn free_port() -> u16 {
  TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap()
    .port()
}

/// The raw answer to a body-less request.
pub fn send(port: u16, method: &str, path: &str) -> Option<String> {
  let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
  write!(
    stream,
    "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
  )
  .ok()?;
  let mut answer = String::new();
  stream.read_to_string(&mut answer).ok()?;
  Some(answer)
}

/// The daemon running for a test.
pub struct Daemon {
  /// the process
  pub child: Child,
  /// the loopback port it serves HTTP on
  pub port: u16,
}

impl Daemon {
  /// Starts the daemon in `dir` on a free loopback port with `args` and only the variables in
  /// `env`, and waits until it answers `/health`.
  pub fn start(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    Self::start_on(free_port(), dir, args, env)
  }

  /// Starts the daemon as [`Daemon::start`] does, on `port`.
  pub fn start_on(port: u16, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    let child = Command::new(PROGRAM)
      // never the release feed, which a release build would read
      .args([
        "--app-dir",
        dir.to_str().unwrap(),
        "-n",
        "--no-https",
        "--no-version-check",
      ])
      .args([
        "--http-ip-address",
        "127.0.0.1",
        "--http-port",
        &port.to_string(),
      ])
      .args(args)
      .env_clear()
      .envs(env.iter().copied())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    let daemon = Self { child, port };
    let started = Instant::now();
    while send(port, "GET", "/health").is_none() {
      assert!(
        started.elapsed() < Duration::from_secs(20),
        "never answered"
      );
      std::thread::sleep(Duration::from_millis(50));
    }
    daemon
  }

  /// The base URL to reach it at.
  pub fn url(&self) -> String {
    format!("http://127.0.0.1:{}", self.port)
  }

  /// Everything it printed, once it has exited.
  pub fn output(&mut self) -> String {
    let mut log = String::new();
    if let Some(mut stdout) = self.child.stdout.take() {
      stdout.read_to_string(&mut log).unwrap();
    }
    log
  }
}

impl Drop for Daemon {
  fn drop(&mut self) {
    // the daemon may have exited already, and then there is nothing to kill or reap
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

/// The administrator account the daemon is started with.
pub const ACCOUNT: [(&str, &str); 2] =
  [("RESEEK_USERNAME", "admin"), ("RESEEK_PASSWORD", "hunter2")];

/// The daemon in `dir` logging in to `server` as `username`, with `extra` arguments.
pub fn daemon(dir: &Path, server: &Soulfind, username: &str, extra: &[&str]) -> Daemon {
  daemon_listening(dir, server, username, free_port(), extra)
}

/// [`daemon`], taking peers' connections on `listen`.
pub fn daemon_listening(
  dir: &Path,
  server: &Soulfind,
  username: &str,
  listen: u16,
  extra: &[&str],
) -> Daemon {
  let address = server.address();
  let port = address.port().to_string();
  let listen = listen.to_string();
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

/// An API client of `daemon`, logged in as its administrator.
pub async fn signed_in(daemon: &Daemon) -> Client {
  let client = Client::new(&daemon.url()).unwrap();
  client.login("admin", "hunter2").await.unwrap();
  client
}

/// The state once `ready` holds for it.
pub async fn until(client: &Client, what: &str, ready: impl Fn(&State) -> bool) -> State {
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

/// Options for a client of the test's own, which takes no listening port.
pub fn quiet() -> ClientOptions {
  ClientOptions {
    enable_listener: false,
    ..ClientOptions::default()
  }
}

/// Whether the daemon is logged in to the server.
pub fn logged_in(state: &State) -> bool {
  state.server.is_logged_in
}

/// The daemon in `dir` logged in to the client harness's scripted server as its user, with the
/// server's end of the connection for the test to read and answer.
pub async fn scripted(dir: &Path) -> (Daemon, Client, client::ServerSide) {
  let fake = client::FakeServer::start().await;
  let port = fake.address().port().to_string();
  let listen = free_port().to_string();
  let args = [
    "--slsk-address",
    "127.0.0.1",
    "--slsk-port",
    &port,
    "--slsk-username",
    client::USERNAME,
    "--slsk-password",
    client::PASSWORD,
    "--slsk-listen-ip-address",
    "127.0.0.1",
    "--slsk-listen-port",
    &listen,
  ];
  let running = Daemon::start(dir, &args, &ACCOUNT);
  let mut server = fake.accept().await;
  server.accept_login().await;
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  (running, api, server)
}
