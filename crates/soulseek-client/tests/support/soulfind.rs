//! soulfind, the open-source Soulseek server, started for the suites that run against a real
//! server. The binary is `SOULFIND_BIN`, else `soulfind` on `PATH`; without one the suites skip
//! with a line on stderr, unless `SOULFIND_REQUIRED=1` makes that a failure. The tests of a binary
//! that run at the same time share one server on a free port with a temporary data directory;
//! the last of them to let go stops it and removes the directory.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use soulseek_client::{Client, ClientOptions, Event, WaitKey};
use soulseek_proto::server::{GetStatusRequest, UserStatus};
use soulseek_proto::{ServerCode, ServerRequest};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// how long a fresh server may take to accept connections
const STARTUP: Duration = Duration::from_secs(10);
/// ports tried before giving up, each after the previous one turned out to be taken
const ATTEMPTS: u32 = 5;

static SHARED: Mutex<Weak<Soulfind>> = Mutex::new(Weak::new());
static USERS: AtomicU32 = AtomicU32::new(0);

/// A running soulfind, stopped when dropped
pub struct Soulfind {
  address: SocketAddr,
  child: Child,
  data: PathBuf,
}

/// The server the tests running now share, or `None` when there is no soulfind to run
pub fn server() -> Option<Arc<Soulfind>> {
  let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
  if let Some(server) = shared.upgrade() {
    return Some(server);
  }
  let binary = binary()?;
  let server = Arc::new(Soulfind::start(&binary));
  *shared = Arc::downgrade(&server);
  Some(server)
}

fn binary() -> Option<PathBuf> {
  if let Some(path) = std::env::var_os("SOULFIND_BIN").filter(|path| !path.is_empty()) {
    return Some(path.into());
  }
  let on_path = std::env::var_os("PATH").and_then(|paths| {
    std::env::split_paths(&paths)
      .map(|dir| dir.join("soulfind"))
      .find(|candidate| candidate.is_file())
  });
  if on_path.is_none() {
    assert!(
      std::env::var_os("SOULFIND_REQUIRED").is_none_or(|required| required != "1"),
      "SOULFIND_REQUIRED=1 but soulfind is neither at SOULFIND_BIN nor on PATH"
    );
    eprintln!("soulfind not found at SOULFIND_BIN or on PATH; skipping this soulfind test");
  }
  on_path
}

impl Soulfind {
  fn start(binary: &Path) -> Self {
    for _ in 0..ATTEMPTS {
      let port = super::free_port();
      let data = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("soulfind-{}-{port}", std::process::id()));
      std::fs::create_dir_all(&data).unwrap();
      let child = Command::new(binary)
        .arg("-p")
        .arg(port.to_string())
        .arg("-d")
        .arg(data.join("soulfind.db"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", binary.display()));
      let mut server = Self {
        address: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        child,
        data,
      };
      if server.ready() {
        return server;
      }
      // dropping it removes the data directory of the failed attempt
    }
    panic!("soulfind did not start on any of {ATTEMPTS} ports");
  }

  /// Waits until the server accepts connections; false when it exited first, which is what it
  /// does when another process holds the port
  fn ready(&mut self) -> bool {
    let deadline = Instant::now() + STARTUP;
    while Instant::now() < deadline {
      if self.exited() {
        return false;
      }
      if TcpStream::connect_timeout(&self.address, Duration::from_millis(100)).is_ok() {
        // soulfind announces itself before it binds, so a connection can reach whoever held the
        // port; a soulfind that failed to bind is gone a moment later
        std::thread::sleep(Duration::from_millis(50));
        return !self.exited();
      }
      std::thread::sleep(Duration::from_millis(10));
    }
    panic!("soulfind did not accept connections within {STARTUP:?}");
  }

  fn exited(&mut self) -> bool {
    self.child.try_wait().unwrap().is_some()
  }

  /// where clients connect
  pub fn address(&self) -> SocketAddr {
    self.address
  }

  /// Logs in a fresh user named after `prefix` and returns it with its event receiver. The name
  /// is unique among the users of this server.
  pub async fn login(self: &Arc<Self>, prefix: &str, options: ClientOptions) -> User {
    self.login_as(username(prefix), options).await
  }

  /// Logs in as `username`, for a test that needs the name before the client exists
  pub async fn login_as(self: &Arc<Self>, username: String, options: ClientOptions) -> User {
    let client = Client::new(options).unwrap();
    let events = client.subscribe();
    self.connect(&client, &username, PASSWORD).await.unwrap();
    User {
      client,
      username,
      events,
      _server: self.clone(),
    }
  }

  /// connects and logs in `client` as `username`
  pub async fn connect(
    &self,
    client: &Client,
    username: &str,
    password: &str,
  ) -> Result<(), soulseek_client::ClientError> {
    tokio::time::timeout(
      super::PATIENCE,
      client.connect(
        &self.address.ip().to_string(),
        self.address.port(),
        username,
        password,
        &CancellationToken::new(),
      ),
    )
    .await
    .expect("soulfind answers the login")
  }
}

impl Drop for Soulfind {
  fn drop(&mut self) {
    // the server may have exited already; either way it is gone afterwards
    let _ = self.child.kill();
    let _ = self.child.wait();
    // a directory that cannot be removed only leaves files in the target directory
    let _ = std::fs::remove_dir_all(&self.data);
  }
}

/// the password every test user registers with
pub const PASSWORD: &str = "soulfind-test";

/// a username starting with `prefix` that no other test of this process uses
pub fn username(prefix: &str) -> String {
  format!("{prefix}{}", USERS.fetch_add(1, Ordering::Relaxed))
}

/// A user logged in to soulfind, which keeps the server running while it exists
pub struct User {
  pub client: Client,
  pub username: String,
  pub events: broadcast::Receiver<Event>,
  _server: Arc<Soulfind>,
}

impl User {
  /// the next event that `pick` accepts, skipping the others
  pub async fn event<T>(&mut self, pick: impl FnMut(Event) -> Option<T>) -> T {
    super::next_event(&mut self.events, pick).await
  }

  /// writes `request` to the server
  pub async fn send(&self, request: ServerRequest) {
    self.client.write_server_message(&request).await.unwrap();
  }

  /// Asks the server about ourselves and waits for the answer. The server handles a user's
  /// messages in order, so everything written before has been handled when this returns.
  pub async fn settle(&self) {
    let _: UserStatus = self
      .ask(
        WaitKey::ServerUser {
          code: ServerCode::GetStatus,
          username: self.username.clone(),
        },
        ServerRequest::GetStatus(GetStatusRequest {
          username: self.username.clone(),
        }),
      )
      .await;
  }

  /// writes `request` and returns what the wait under `key` completes with
  pub async fn ask<T: Send + 'static>(&self, key: WaitKey, request: ServerRequest) -> T {
    let wait = self
      .client
      .waiter()
      .wait::<T>(key, None, CancellationToken::new());
    self.send(request).await;
    tokio::time::timeout(super::PATIENCE, wait)
      .await
      .expect("the server answers")
      .unwrap()
  }
}
