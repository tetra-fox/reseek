//! Running the daemon program for a test: on a loopback port the system picks, with its
//! directory, and stopped when the test is done with it; and the relay agent program beside
//! it.

#![allow(dead_code, reason = "each test binary uses a part of it")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use api_client::Client;
use api_types::state::State;
use api_types::transfers::{
  EnqueueDownloadFile, EnqueueDownloadGroup, EnqueueDownloadJobRequest, Transfer, TransferDirection,
};
use soulfind::{PASSWORD, Soulfind};
use uuid::Uuid;

/// The client's test harness: the Soulseek server the network tests run against, a scripted
/// server and scripted peers for what a real one never does, and files to send.
#[path = "../../../soulseek-client/tests/support/mod.rs"]
pub mod client;
pub mod scripted;

#[allow(unused_imports, reason = "each test binary uses a part of it")]
pub use client::{free_port, options as quiet, peer, soulfind, transfers};

/// A download job of `filename`, `size` bytes, from `username`.
pub fn job(username: &str, filename: &str, size: u64) -> EnqueueDownloadJobRequest {
  EnqueueDownloadJobRequest {
    groups: vec![EnqueueDownloadGroup {
      username: username.to_string(),
      files: vec![EnqueueDownloadFile {
        filename: filename.to_string(),
        size,
      }],
      destination: None,
      search_id: None,
    }],
  }
}

/// Asks the daemon to download `filename` of `size` bytes from `username` as a job of its own,
/// and returns the download's id.
pub async fn enqueue(api: &Client, username: &str, filename: &str, size: u64) -> Uuid {
  let answer = api
    .enqueue_download_job(&job(username, filename, size))
    .await
    .unwrap();
  let group = &answer.groups[0];
  assert!(
    group.error.is_none() && group.failures.is_empty(),
    "not enqueued: {group:?}"
  );
  group.enqueued[0].id
}

/// How long a test waits for something that should happen promptly.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// The data of the events of `kind` in the daemon's event log, oldest first, once there are at
/// least `count`.
pub async fn events(api: &Client, kind: &str, count: usize) -> Vec<serde_json::Value> {
  let query: api_types::events::EventsQuery =
    serde_json::from_value(serde_json::json!({ "type": kind })).unwrap();
  let started = Instant::now();
  loop {
    let items = api.events(&query).await.unwrap().items;
    if items.len() >= count {
      return items
        .iter()
        .rev()
        .map(|record| serde_json::to_value(record).unwrap()["data"].clone())
        .collect();
    }
    assert!(started.elapsed() < PATIENCE, "{kind}: {items:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

/// The daemon program. Tests run it with its UI: the API's password login, which every test
/// signs in with, is refused under `--headless`.
pub const PROGRAM: &str = env!("CARGO_BIN_EXE_reseek");

/// The relay agent program.
pub const AGENT: &str = env!("CARGO_BIN_EXE_reseek-agent");

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

/// A program running for a test, killed when the test is done with it.
pub struct Process {
  pub child: Child,
  printed: Arc<Printed>,
  reader: Option<JoinHandle<()>>,
}

/// The daemon running for a test.
pub struct Daemon {
  pub process: Process,
  /// the loopback port it serves HTTP on
  pub port: u16,
}

/// What a program printed so far, read off its stdout as it is written so the pipe never fills
/// up.
#[derive(Default)]
struct Printed {
  text: Mutex<String>,
  /// signaled at every line and when the output ends
  more: Condvar,
}

impl Printed {
  /// Appends every line of `stdout` until the daemon closes it.
  fn read(&self, stdout: ChildStdout) {
    for line in BufReader::new(stdout).lines() {
      let Ok(line) = line else { break };
      let mut text = self.text.lock().unwrap_or_else(PoisonError::into_inner);
      text.push_str(&line);
      text.push('\n');
      self.more.notify_all();
    }
    self.more.notify_all();
  }
}

/// How long the daemon may take to start: far past what it takes on a busy machine, as a start
/// that never ends is a failure of its own, not a slow one.
const STARTUP: Duration = Duration::from_secs(120);

/// The line the daemon writes once it has started, just before it takes requests.
const STARTED: &str = "reseek started";

/// The port in the log line the daemon writes once its HTTP listener is bound.
fn listening_port(printed: &str) -> Option<u16> {
  let line = printed
    .lines()
    .find(|line| line.contains("Listening for HTTP requests"))?;
  let url = &line[line.find("url=http://")? + "url=http://".len()..];
  let address = url.split('/').next()?;
  address.rsplit_once(':')?.1.parse().ok()
}

impl Daemon {
  /// Starts the daemon in `dir` on a loopback port the system picks, with `args` and only the
  /// variables in `env`, and waits until it says it has started.
  pub fn start(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    Self::start_on(0, dir, args, env)
  }

  /// Starts the daemon as [`Daemon::start`] does, on `port`. A daemon that exits before it has
  /// started fails the test with what it printed.
  pub fn start_on(port: u16, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    Self::launch(Command::new(PROGRAM), port, dir, args, env)
  }

  /// [`Daemon::start`] run from the directory `cwd`, which a relative `dir` is read against.
  pub fn start_from(cwd: &Path, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    let mut command = Command::new(PROGRAM);
    command.current_dir(cwd);
    Self::launch(command, 0, dir, args, env)
  }

  fn launch(
    mut command: Command,
    port: u16,
    dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
  ) -> Self {
    command
      // never the release feed, which a release build would read
      .args([
        "--app-dir",
        dir.to_str().unwrap(),
        "-n",
        "--no-https",
        "--no-version-check",
        "--no-color",
      ])
      .args([
        "--http-ip-address",
        "127.0.0.1",
        "--http-port",
        &port.to_string(),
      ])
      .args(args)
      .env_clear()
      .envs(env.iter().copied());
    let mut process = Process::spawn(&mut command);
    let port = Self::started(&mut process);
    Self { process, port }
  }

  /// Waits until the daemon says it has started, and gives the port it said it listens on. Fails
  /// the test when the program exits first, or has not started after [`STARTUP`].
  fn started(process: &mut Process) -> u16 {
    let begun = Instant::now();
    loop {
      let text = process
        .printed
        .text
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
      if text.contains(STARTED) {
        return listening_port(&text).expect("the daemon names its HTTP port before it starts");
      }
      // woken by each line the program writes and when its output ends
      let (text, _) = process
        .printed
        .more
        .wait_timeout(text, Duration::from_millis(100))
        .unwrap_or_else(PoisonError::into_inner);
      drop(text);
      if let Some(status) = process.child.try_wait().unwrap() {
        panic!(
          "the program exited with {status} before it started:\n{}",
          process.output()
        );
      }
      assert!(
        begun.elapsed() < STARTUP,
        "not started after {STARTUP:?}:\n{}",
        process.printed()
      );
    }
  }

  /// The base URL to reach it at.
  pub fn url(&self) -> String {
    format!("http://127.0.0.1:{}", self.port)
  }
}

impl Process {
  /// Starts `command` with its standard output read as it is written.
  pub fn spawn(command: &mut Command) -> Self {
    let mut child = command
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    let printed = Arc::new(Printed::default());
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn({
      let printed = printed.clone();
      move || printed.read(stdout)
    });
    Self {
      child,
      printed,
      reader: Some(reader),
    }
  }

  /// What it printed so far.
  pub fn printed(&self) -> String {
    self
      .printed
      .text
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .clone()
  }

  /// Waits until it has printed a line holding every one of `parts`, and returns that line.
  pub async fn printed_line(&self, parts: &[&str]) -> String {
    let started = Instant::now();
    loop {
      let printed = self.printed();
      if let Some(line) = printed
        .lines()
        .find(|line| parts.iter().all(|part| line.contains(part)))
      {
        return line.to_string();
      }
      assert!(
        started.elapsed() < PATIENCE,
        "never printed {parts:?}:\n{printed}"
      );
      tokio::time::sleep(Duration::from_millis(50)).await;
    }
  }

  /// Everything it printed, once it has exited.
  pub fn output(&mut self) -> String {
    if let Some(reader) = self.reader.take() {
      reader.join().unwrap();
    }
    self
      .printed
      .text
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .clone()
  }
}

impl Drop for Process {
  fn drop(&mut self) {
    // the program may have exited already, and then there is nothing to kill or reap
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
  daemon_on(0, dir, server, username, listen, extra)
}

/// [`daemon_listening`], serving HTTP on `http`, or a port the system picks when it is 0.
pub fn daemon_on(
  http: u16,
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
    // never the router of the machine the tests run on
    "--slsk-no-port-mapping",
    "--remote-configuration",
  ];
  args.extend_from_slice(extra);
  Daemon::start_on(http, dir, &args, &ACCOUNT)
}

/// An API client of `daemon`, logged in as its administrator.
pub async fn signed_in(daemon: &Daemon) -> Client {
  let client = Client::new(&daemon.url()).await.unwrap();
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

/// Whether the daemon is logged in to the server.
pub fn logged_in(state: &State) -> bool {
  state.server.is_logged_in
}

/// The daemon in `dir` logged in to the client harness's scripted server as its user, with the
/// server's end of the connection for the test to read and answer.
pub async fn scripted(dir: &Path) -> (Daemon, Client, client::ServerSide) {
  let (running, mut server) = scripted_login(dir).await;
  server.accept_login().await;
  let api = signed_in(&running).await;
  until(&api, "logged in", logged_in).await;
  (running, api, server)
}

/// The daemon in `dir` connected to the client harness's scripted server as its user, with the
/// server's end of the connection, whose login is the test's to answer.
pub async fn scripted_login(dir: &Path) -> (Daemon, client::ServerSide) {
  scripted_login_with(dir, &["--slsk-password", client::PASSWORD]).await
}

/// [`scripted_login`] with `extra` arguments, which give the password unless the test's
/// configuration file does.
pub async fn scripted_login_with(dir: &Path, extra: &[&str]) -> (Daemon, client::ServerSide) {
  let fake = client::FakeServer::start().await;
  let port = fake.address().port().to_string();
  let listen = free_port().to_string();
  let mut args = vec![
    "--slsk-address",
    "127.0.0.1",
    "--slsk-port",
    &port,
    "--slsk-username",
    client::USERNAME,
    "--slsk-listen-ip-address",
    "127.0.0.1",
    "--slsk-listen-port",
    &listen,
    "--slsk-no-port-mapping",
  ];
  args.extend_from_slice(extra);
  let running = Daemon::start(dir, &args, &ACCOUNT);
  let server = fake.accept().await;
  (running, server)
}

/// The download `id` from `username`, with its history, once `ready` holds for it.
pub async fn download_when(
  api: &Client,
  username: &str,
  id: Uuid,
  what: &str,
  ready: impl Fn(&Transfer) -> bool,
) -> Transfer {
  let started = Instant::now();
  loop {
    let transfer = api
      .transfer(TransferDirection::Download, username, id, true)
      .await
      .unwrap();
    if ready(&transfer) {
      return transfer;
    }
    assert!(
      started.elapsed() < PATIENCE * 2,
      "never {what}: {transfer:?}"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}
