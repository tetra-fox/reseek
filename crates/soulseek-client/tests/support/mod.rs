//! A fake Soulseek server over loopback, a client logged in to it, and helpers to watch what
//! the client raises and logs.

#![allow(dead_code)] // each test binary uses its own subset of these helpers

pub mod soulfind;

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use soulseek_client::{Client, ClientOptions, Event, Handoffs, ServerHandoff, WaitKey};
use soulseek_proto::frame::{self, FrameKind};
use soulseek_proto::server::{
  GlobalAdminMessage, LoginRequest, LoginResponse, UserData, UserStats,
};
use soulseek_proto::{Limits, Secret, ServerCode, ServerRequest, ServerResponse, UserPresence};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub const USERNAME: &str = "alice";
pub const PASSWORD: &str = "hunter2";
pub const PUBLIC_IP: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);

/// how long a test waits for something that should happen promptly
pub const PATIENCE: Duration = Duration::from_secs(10);

/// options for a client that does not listen, so tests need no free port
pub fn options() -> ClientOptions {
  ClientOptions {
    enable_listener: false,
    ..ClientOptions::default()
  }
}

/// a port nothing listens on right now
pub fn free_port() -> u16 {
  std::net::TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap()
    .port()
}

/// The server's listening socket
pub struct FakeServer {
  listener: TcpListener,
}

impl FakeServer {
  pub async fn start() -> Self {
    Self {
      listener: TcpListener::bind("127.0.0.1:0").await.unwrap(),
    }
  }

  pub fn address(&self) -> SocketAddr {
    self.listener.local_addr().unwrap()
  }

  pub async fn accept(&self) -> ServerSide {
    let (stream, _) = tokio::time::timeout(PATIENCE, self.listener.accept())
      .await
      .expect("the client connects")
      .unwrap();
    ServerSide {
      stream,
      buffer: Vec::new(),
    }
  }
}

/// The server's end of one client connection
pub struct ServerSide {
  pub stream: TcpStream,
  buffer: Vec<u8>,
}

impl ServerSide {
  /// the next frame the client sent, whole
  pub async fn frame(&mut self) -> Vec<u8> {
    loop {
      if let Some((_, consumed)) =
        frame::split(FrameKind::Server, &self.buffer, &Limits::DEFAULT).unwrap()
      {
        return self.buffer.drain(..consumed).collect();
      }
      let mut chunk = [0; 4096];
      let read = tokio::time::timeout(PATIENCE, self.stream.read(&mut chunk))
        .await
        .expect("the client sends a message")
        .unwrap();
      assert!(read > 0, "the client closed the connection");
      self.buffer.extend_from_slice(&chunk[..read]);
    }
  }

  /// the next message the client sent, skipping write-liveness pings
  pub async fn request(&mut self) -> ServerRequest {
    loop {
      let request = ServerRequest::decode(&self.frame().await).unwrap();
      if request != ServerRequest::Ping {
        return request;
      }
    }
  }

  pub async fn send(&mut self, response: &ServerResponse) {
    self.send_bytes(&response.encode()).await;
  }

  pub async fn send_bytes(&mut self, bytes: &[u8]) {
    self.stream.write_all(bytes).await.unwrap();
  }

  /// reads the login and the listen port written with it
  pub async fn read_login(&mut self) -> (LoginRequest, u32) {
    let ServerRequest::Login(login) = self.request().await else {
      panic!("the first message is the login");
    };
    let ServerRequest::SetListenPort(port) = self.request().await else {
      panic!("the listen port follows the login");
    };
    (login, port.port)
  }

  pub async fn accept_login(&mut self) {
    self.read_login().await;
    self.send(&login_success()).await;
  }

  /// reads the messages a client sends right after logging in
  pub async fn read_configuration(&mut self) -> Vec<ServerRequest> {
    let mut messages = Vec::new();
    for _ in 0..6 {
      messages.push(self.request().await);
    }
    messages
  }

  /// waits for the client to close the connection, failing if it sends anything but pings
  pub async fn closed(&mut self) {
    loop {
      let mut chunk = [0; 4096];
      let read = tokio::time::timeout(PATIENCE, self.stream.read(&mut chunk))
        .await
        .expect("the client closes the connection")
        .unwrap_or(0);
      if read == 0 {
        return;
      }
    }
  }
}

pub fn login_success() -> ServerResponse {
  ServerResponse::Login(LoginResponse::Success {
    greeting: "welcome".into(),
    ip: PUBLIC_IP,
    hash: Secret::new("0123456789abcdef0123456789abcdef"),
    is_supporter: true,
  })
}

/// a client logged in to a fake server, with everything a test looks at
pub struct Session {
  pub client: Client,
  pub handoffs: Handoffs,
  pub server: ServerSide,
  pub events: broadcast::Receiver<Event>,
  pub fake: FakeServer,
}

impl Session {
  pub async fn start() -> Self {
    Self::with_options(options()).await
  }

  pub async fn with_options(options: ClientOptions) -> Self {
    let fake = FakeServer::start().await;
    let (client, handoffs) = Client::new(options).unwrap();
    let events = client.subscribe();
    let address = fake.address();
    let connecting = {
      let client = client.clone();
      tokio::spawn(async move {
        client
          .connect(
            &address.ip().to_string(),
            address.port(),
            USERNAME,
            PASSWORD,
            &CancellationToken::new(),
          )
          .await
      })
    };
    let mut server = fake.accept().await;
    server.accept_login().await;
    server.read_configuration().await;
    connecting.await.unwrap().unwrap();
    Self {
      client,
      handoffs,
      server,
      events,
      fake,
    }
  }

  /// the next event that `pick` accepts, skipping the others
  pub async fn event<T>(&mut self, pick: impl FnMut(Event) -> Option<T>) -> T {
    next_event(&mut self.events, pick).await
  }

  /// registers a wait under `key`, sends `response` and returns what the wait completed with
  pub async fn reply<T: Send + 'static>(&mut self, key: WaitKey, response: ServerResponse) -> T {
    let wait = self
      .client
      .waiter()
      .wait::<T>(key, None, CancellationToken::new());
    self.server.send(&response).await;
    tokio::time::timeout(PATIENCE, wait)
      .await
      .expect("the wait completes")
      .unwrap()
  }

  /// sends `response` and returns the next server hand-off
  pub async fn handoff(&mut self, response: ServerResponse) -> ServerHandoff {
    self.server.send(&response).await;
    tokio::time::timeout(PATIENCE, self.handoffs.server.recv())
      .await
      .expect("the message is handed on")
      .unwrap()
  }

  /// sends a message every client handles by raising an event, and waits for that event, so
  /// everything sent before it has been handled
  pub async fn settle(&mut self) {
    let marker = ServerResponse::GlobalAdminMessage(GlobalAdminMessage {
      message: "marker".into(),
    });
    self.server.send(&marker).await;
    self
      .event(|event| {
        matches!(event, Event::GlobalMessageReceived(message) if message == "marker").then_some(())
      })
      .await;
  }
}

pub fn user_key(code: ServerCode, username: &str) -> WaitKey {
  WaitKey::ServerUser {
    code,
    username: username.into(),
  }
}

pub fn room_key(code: ServerCode, room: &str) -> WaitKey {
  WaitKey::ServerRoom {
    code,
    room: room.into(),
  }
}

pub fn room_user_key(code: ServerCode, room: &str, username: &str) -> WaitKey {
  WaitKey::ServerRoomUser {
    code,
    room: room.into(),
    username: username.into(),
  }
}

pub fn stats() -> UserStats {
  UserStats {
    average_speed: 1000,
    upload_count: 5,
    file_count: 300,
    directory_count: 20,
  }
}

pub fn user_data(username: &str) -> UserData {
  UserData {
    username: username.into(),
    status: UserPresence::Online,
    stats: stats(),
    slots_full: true,
    country_code: "NL".into(),
  }
}

/// the next event that `pick` accepts, skipping the others
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

/// every event received so far
pub fn drain(events: &mut broadcast::Receiver<Event>) -> Vec<Event> {
  let mut drained = Vec::new();
  while let Ok(event) = events.try_recv() {
    drained.push(event);
  }
  drained
}

/// Log lines written while the guard lives, for the thread the test runs on. Tests that use it
/// run on the current-thread runtime, so the client's tasks log on that thread too.
pub struct Logs {
  lines: Arc<Mutex<Vec<u8>>>,
  _guard: tracing::subscriber::DefaultGuard,
}

impl Logs {
  pub fn capture() -> Self {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(lines.clone());
    let subscriber = tracing_subscriber::fmt()
      .with_max_level(tracing::Level::TRACE)
      .with_ansi(false)
      .with_writer(move || writer.clone())
      .finish();
    Self {
      lines,
      _guard: tracing::subscriber::set_default(subscriber),
    }
  }

  pub fn text(&self) -> String {
    String::from_utf8_lossy(&self.lines.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
  }

  /// the captured lines that contain every one of `parts`
  pub fn lines_with(&self, parts: &[&str]) -> Vec<String> {
    self
      .text()
      .lines()
      .filter(|line| parts.iter().all(|part| line.contains(part)))
      .map(str::to_owned)
      .collect()
  }
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    self
      .0
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .extend_from_slice(bytes);
    Ok(bytes.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}
