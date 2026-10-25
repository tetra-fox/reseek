//! A fake Soulseek server over loopback, a client logged in to it, and helpers to watch what
//! the client raises and logs.

#![allow(dead_code)] // each test binary uses its own subset of these helpers

pub mod peer;
pub mod soulfind;
pub mod tree;

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use soulseek_client::{Client, ClientError, ClientOptions, ClientState, Event, WaitKey};
use soulseek_proto::frame::{self, FrameKind};
use soulseek_proto::init::PeerInit;
use soulseek_proto::server::{
  GlobalAdminMessage, LoginRequest, LoginResponse, PrivateMessageRequest, UserData, UserStats,
};
use soulseek_proto::{
  InitMessage, Limits, PeerConnectionType, Secret, ServerCode, ServerRequest, ServerResponse,
  UserPresence,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
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

/// options for a client that listens on loopback, on a free port
pub fn listening_options() -> ClientOptions {
  ClientOptions {
    listen_ip_address: Ipv4Addr::LOCALHOST.into(),
    listen_port: free_port(),
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

  /// a server whose sockets take in only about `bytes` while it is not reading, so what a client
  /// writes piles up in its own send buffer
  pub async fn with_receive_buffer(bytes: u32) -> Self {
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(bytes).unwrap();
    socket.bind((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
    Self {
      listener: socket.listen(16).unwrap(),
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

  /// the next message `pick` accepts, skipping the others
  pub async fn request_matching<T>(
    &mut self,
    mut pick: impl FnMut(ServerRequest) -> Option<T>,
  ) -> T {
    loop {
      if let Some(picked) = pick(self.request().await) {
        return picked;
      }
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

  /// reads and throws away everything the client sends, for as long as it is polled
  pub async fn discard(&mut self) {
    let mut chunk = vec![0; 64 << 10];
    loop {
      let read = self.stream.read(&mut chunk).await.unwrap();
      assert!(read > 0, "the client closed the connection");
    }
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
  pub server: ServerSide,
  pub events: broadcast::Receiver<Event>,
  pub fake: FakeServer,
}

impl Session {
  pub async fn start() -> Self {
    Self::with_options(options()).await
  }

  pub async fn with_options(options: ClientOptions) -> Self {
    Self::on(FakeServer::start().await, options).await
  }

  /// a client logged in to `fake`
  pub async fn on(fake: FakeServer, options: ClientOptions) -> Self {
    let client = Client::new(options).unwrap();
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
      server,
      events,
      fake,
    }
  }

  /// the next event that `pick` accepts, skipping the others
  pub async fn event<T>(&mut self, pick: impl FnMut(Event) -> Option<T>) -> T {
    next_event(&mut self.events, pick).await
  }

  /// Has the client write more to the server than the sockets hold. Until the server reads
  /// again, with [`ServerSide::discard`], every later write to it waits behind this one; the
  /// server needs a small receive buffer and the client a write queue long enough for the
  /// messages that pile up meanwhile.
  pub fn stall_server_writes(&self) {
    let filler = ServerRequest::PrivateMessage(PrivateMessageRequest {
      username: "filler".into(),
      message: "x".repeat(60 << 10),
    });
    let client = self.client.clone();
    tokio::spawn(async move {
      // the server reads it only when the test is done with the stall, or never. 15 MB is
      // several times what a loopback socket pair holds with a small receive window
      let _ = client.write_server_messages(&vec![filler; 256]).await;
    });
  }

  /// where the client listens, when it does
  pub fn listen_address(&self) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, self.client.options().listen_port))
  }

  /// a scripted peer named `username` that connected to the client for messages
  pub async fn peer(&self, username: &str) -> peer::PeerSide {
    let mut peer = peer::PeerSide::connect(self.listen_address()).await;
    let introduce = InitMessage::PeerInit(PeerInit {
      username: username.into(),
      connection_type: PeerConnectionType::Peer,
      token: 0,
    });
    peer.send(&introduce.encode()).await;
    peer
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

/// the state transitions among `events`, as (previous, state, message)
pub fn transitions(events: &[Event]) -> Vec<(ClientState, ClientState, String)> {
  events
    .iter()
    .filter_map(|event| match event {
      Event::StateChanged {
        previous_state,
        state,
        message,
        ..
      } => Some((*previous_state, *state, message.clone())),
      _ => None,
    })
    .collect()
}

/// the message and cause of the one `Disconnected` among `events`
pub fn disconnected(events: &[Event]) -> (String, Option<ClientError>) {
  let mut disconnects = events.iter().filter_map(|event| match event {
    Event::Disconnected { message, error } => Some((message.clone(), error.clone())),
    _ => None,
  });
  let disconnect = disconnects.next().expect("Disconnected is raised");
  assert!(disconnects.next().is_none(), "Disconnected is raised once");
  disconnect
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

  /// Waits until a line with every one of `parts` is written; for what another task logs as it
  /// happens
  pub async fn wait_for(&self, parts: &[&str]) {
    let found = tokio::time::timeout(PATIENCE, async {
      while self.lines_with(parts).is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await;
    assert!(found.is_ok(), "no line with {parts:?} in\n{}", self.text());
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
