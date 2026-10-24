//! Connecting, logging in and disconnecting against a fake server: the state transitions and
//! their events, every login outcome, cancellation, being kicked and losing the connection.

mod support;

use std::time::Duration;

use soulseek_client::{
  Client, ClientError, ClientOptions, ClientState, ConnectionErrorKind, Event, ServerInfo, WaitKey,
};
use soulseek_proto::server::{
  AcceptChildren, BranchLevel, BranchRoot, HaveNoParents, LoginResponse, PrivateRoomToggle,
  SetListenPort,
};
use soulseek_proto::{
  PROTOCOL_MAJOR_VERSION, PROTOCOL_MINOR_VERSION, PeerCode, ServerCode, ServerRequest,
  ServerResponse,
};
use support::{FakeServer, PASSWORD, PUBLIC_IP, Session, USERNAME, drain, next_event, options};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

fn connect(
  client: &Client,
  server: &FakeServer,
  cancel: CancellationToken,
) -> JoinHandle<Result<(), ClientError>> {
  let client = client.clone();
  let address = server.address();
  tokio::spawn(async move {
    client
      .connect(
        &address.ip().to_string(),
        address.port(),
        USERNAME,
        PASSWORD,
        &cancel,
      )
      .await
  })
}

/// the state transitions among `events`, as (previous, state, message)
fn transitions(events: &[Event]) -> Vec<(ClientState, ClientState, String)> {
  events
    .iter()
    .filter_map(|event| match event {
      Event::StateChanged {
        previous,
        state,
        message,
        ..
      } => Some((*previous, *state, message.clone())),
      _ => None,
    })
    .collect()
}

#[tokio::test]
async fn login_succeeds_and_configures_the_session() {
  let fake = FakeServer::start().await;
  let (client, _handoffs) = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;

  let (login, port) = server.read_login().await;
  assert_eq!(login.username, USERNAME);
  assert_eq!(login.password.expose(), PASSWORD);
  assert_eq!(login.major_version, PROTOCOL_MAJOR_VERSION);
  assert_eq!(login.minor_version, PROTOCOL_MINOR_VERSION);
  assert_eq!(login.hash.expose(), "bb6de2fa07892f34b0e1f62df88f3384");
  assert_eq!(port, 50_000);
  assert_eq!(
    client.state(),
    ClientState::CONNECTED | ClientState::LOGGING_IN
  );
  server.send(&support::login_success()).await;

  assert_eq!(
    server.read_configuration().await,
    [
      ServerRequest::SetListenPort(SetListenPort {
        port: 50_000,
        obfuscation: None
      }),
      ServerRequest::PrivateRoomToggle(PrivateRoomToggle {
        accept_invitations: false
      }),
      ServerRequest::BranchLevel(BranchLevel { level: 0 }),
      ServerRequest::BranchRoot(BranchRoot {
        root: USERNAME.into()
      }),
      ServerRequest::AcceptChildren(AcceptChildren { accept: false }),
      ServerRequest::HaveNoParents(HaveNoParents {
        have_no_parents: true
      }),
    ]
  );
  connecting.await.unwrap().unwrap();

  assert_eq!(client.state(), ClientState::READY);
  assert_eq!(client.username().as_deref(), Some(USERNAME));
  assert_eq!(client.address(), Some(fake.address().ip().to_string()));
  assert_eq!(client.ip_endpoint(), Some(fake.address()));
  assert_eq!(
    client.server_info(),
    ServerInfo {
      is_supporter: Some(true),
      own_public_ip: Some(PUBLIC_IP),
      ..ServerInfo::default()
    }
  );

  let events = drain(&mut events);
  let order: Vec<&str> = events
    .iter()
    .map(|event| match event {
      Event::StateChanged { .. } => "StateChanged",
      Event::Connected => "Connected",
      Event::LoggedIn => "LoggedIn",
      Event::ServerInfoReceived(_) => "ServerInfoReceived",
      other => panic!("unexpected {other:?}"),
    })
    .collect();
  assert_eq!(
    order,
    [
      "StateChanged",
      "StateChanged",
      "Connected",
      "StateChanged",
      "ServerInfoReceived",
      "StateChanged",
      "LoggedIn"
    ]
  );
  let connected = format!("Connected to {}", fake.address());
  assert_eq!(
    transitions(&events),
    [
      (
        ClientState::DISCONNECTED,
        ClientState::CONNECTING,
        "Connecting".into()
      ),
      (ClientState::CONNECTING, ClientState::CONNECTED, connected),
      (
        ClientState::CONNECTED,
        ClientState::CONNECTED | ClientState::LOGGING_IN,
        "Logging in".into()
      ),
      (
        ClientState::CONNECTED | ClientState::LOGGING_IN,
        ClientState::READY,
        "Logged in".into()
      ),
    ]
  );
}

#[tokio::test]
async fn connecting_twice_is_refused() {
  let mut session = Session::start().await;
  let address = session.fake.address();
  let error = session
    .client
    .connect(
      &address.ip().to_string(),
      address.port(),
      USERNAME,
      PASSWORD,
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::InvalidOperation(message) if message == "The client is already connected")
  );
  drain(&mut session.events);
}

async fn rejected_login(response: LoginResponse) -> (ClientError, Vec<Event>, Client) {
  let fake = FakeServer::start().await;
  let (client, _handoffs) = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&ServerResponse::Login(response)).await;
  let error = connecting.await.unwrap().unwrap_err();
  (error, drain(&mut events), client)
}

fn disconnected(events: &[Event]) -> (String, Option<ClientError>) {
  let mut disconnects = events.iter().filter_map(|event| match event {
    Event::Disconnected { message, error } => Some((message.clone(), error.clone())),
    _ => None,
  });
  let disconnect = disconnects.next().expect("Disconnected is raised");
  assert!(disconnects.next().is_none(), "Disconnected is raised once");
  disconnect
}

#[tokio::test]
async fn a_rejected_login_reports_the_reason() {
  let (error, events, client) = rejected_login(LoginResponse::Failure {
    reason: "INVALIDPASS".into(),
    detail: None,
  })
  .await;
  assert!(
    matches!(&error, ClientError::LoginRejected(message) if message == "The server rejected login attempt: INVALIDPASS")
  );
  let (message, cause) = disconnected(&events);
  assert_eq!(message, "The server rejected login attempt: INVALIDPASS");
  assert!(matches!(cause, Some(ClientError::LoginRejected(_))));
  assert_eq!(client.state(), ClientState::DISCONNECTED);
  assert_eq!(client.username(), None);
}

#[tokio::test]
async fn a_rejected_login_appends_the_detail() {
  let (error, ..) = rejected_login(LoginResponse::Failure {
    reason: "INVALIDUSERNAME".into(),
    detail: Some("Nick too long.".into()),
  })
  .await;
  assert!(
    matches!(&error, ClientError::LoginRejected(message) if message == "The server rejected login attempt: INVALIDUSERNAME: Nick too long.")
  );
}

#[tokio::test]
async fn no_login_reply_is_reported_as_a_possible_ban() {
  let fake = FakeServer::start().await;
  let (client, _handoffs) = Client::new(ClientOptions {
    message_timeout: Duration::from_millis(300),
    ..options()
  })
  .unwrap();
  let mut events = client.subscribe();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  let error = connecting.await.unwrap().unwrap_err();
  assert!(
    matches!(&error, ClientError::LoginNoResponse(message) if message == "server did not respond to login; the account may be banned"),
    "{error:?}"
  );
  let (_, cause) = disconnected(&drain(&mut events));
  assert!(matches!(cause, Some(ClientError::LoginNoResponse(_))));
  server.closed().await;
}

#[tokio::test]
async fn a_refused_connection_is_a_connect_error() {
  let fake = FakeServer::start().await;
  let address = fake.address();
  drop(fake);
  let (client, _handoffs) = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let error = client
    .connect(
      &address.ip().to_string(),
      address.port(),
      USERNAME,
      PASSWORD,
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
  let ClientError::Connect { message, .. } = &error else {
    panic!("expected a connect error, got {error:?}");
  };
  assert!(
    message.starts_with(&format!(
      "Failed to connect: Failed to connect to {address}: "
    )),
    "{message}"
  );
  let events = drain(&mut events);
  assert_eq!(
    transitions(&events)
      .iter()
      .map(|(_, state, _)| *state)
      .collect::<Vec<_>>(),
    [
      ClientState::CONNECTING,
      ClientState::DISCONNECTING,
      ClientState::DISCONNECTED
    ]
  );
  assert!(matches!(
    disconnected(&events).1,
    Some(ClientError::Connect { .. })
  ));
}

#[tokio::test]
async fn the_server_closing_during_login_is_a_connect_error() {
  let fake = FakeServer::start().await;
  let (client, _handoffs) = Client::new(options()).unwrap();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  drop(server);
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Connect { .. }), "{error:?}");
}

#[tokio::test]
async fn cancelling_the_connect_while_logging_in() {
  let fake = FakeServer::start().await;
  let (client, _handoffs) = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let cancel = CancellationToken::new();
  let connecting = connect(&client, &fake, cancel.clone());
  let mut server = fake.accept().await;
  server.read_login().await;
  cancel.cancel();
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Cancelled), "{error:?}");
  assert!(matches!(
    disconnected(&drain(&mut events)).1,
    Some(ClientError::Cancelled)
  ));
  assert_eq!(client.state(), ClientState::DISCONNECTED);
  server.closed().await;
}

#[tokio::test]
async fn disconnecting_cancels_a_connect_in_progress() {
  let fake = FakeServer::start().await;
  let (client, _handoffs) = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  client.disconnect(Some("user asked"), None).await;
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Cancelled), "{error:?}");
  let (message, cause) = disconnected(&drain(&mut events));
  assert_eq!(message, "user asked");
  assert!(cause.is_none());
  assert_eq!(client.state(), ClientState::DISCONNECTED);
  server.closed().await;
}

#[tokio::test]
async fn being_kicked_disconnects_with_the_kick_as_the_cause() {
  let mut session = Session::start().await;
  drain(&mut session.events);
  session.server.send(&ServerResponse::KickedFromServer).await;
  session
    .event(|event| matches!(event, Event::KickedFromServer).then_some(()))
    .await;
  let (message, cause) = session
    .event(|event| match event {
      Event::Disconnected { message, error } => Some((message, error)),
      _ => None,
    })
    .await;
  assert_eq!(message, "Kicked from server");
  assert!(matches!(cause, Some(ClientError::KickedFromServer)));
  assert_eq!(session.client.state(), ClientState::DISCONNECTED);
  session.server.closed().await;
}

#[tokio::test]
async fn the_server_closing_the_connection_ends_the_session() {
  let mut session = Session::start().await;
  drain(&mut session.events);
  let address = session.fake.address();
  drop(session.server.stream);
  let (message, cause) = next_event(&mut session.events, |event| match event {
    Event::Disconnected { message, error } => Some((message, error)),
    _ => None,
  })
  .await;
  assert_eq!(message, "Read error: Remote connection closed");
  let cause = cause.expect("the read error is the cause");
  assert_eq!(cause.connection_kind(), Some(ConnectionErrorKind::Read));
  assert!(
    cause
      .to_string()
      .starts_with(&format!("Failed to read 4 bytes from {address}")),
    "{cause}"
  );
  assert_eq!(session.client.state(), ClientState::DISCONNECTED);
  assert_eq!(session.client.username(), None);
  assert_eq!(session.client.address(), None);
  assert_eq!(session.client.ip_endpoint(), None);
}

#[tokio::test]
async fn disconnecting_fails_server_waits_and_keeps_peer_waits() {
  let mut session = Session::start().await;
  let waiter = session.client.waiter().clone();
  let cancel = CancellationToken::new();
  let server_wait = waiter.wait::<()>(
    WaitKey::ServerUser {
      code: ServerCode::GetStatus,
      username: "bob".into(),
    },
    None,
    cancel.clone(),
  );
  let peer_key = WaitKey::Peer {
    code: PeerCode::InfoResponse,
    username: "bob".into(),
  };
  let peer_wait = waiter.wait::<u32>(peer_key.clone(), None, cancel.clone());

  session.client.disconnect(Some("done"), None).await;

  let error = server_wait.await.unwrap_err();
  assert_eq!(
    error.to_string(),
    "server connection disconnected: done",
    "{error:?}"
  );
  assert_eq!(error.connection_kind(), Some(ConnectionErrorKind::Other));
  assert!(waiter.has_wait(&peer_key));
  assert!(waiter.complete(&peer_key, 7u32));
  assert_eq!(peer_wait.await.unwrap(), 7);

  let events = drain(&mut session.events);
  assert_eq!(disconnected(&events).0, "done");
  session.server.closed().await;
}

#[tokio::test]
async fn disconnecting_twice_raises_disconnected_once() {
  let mut session = Session::start().await;
  drain(&mut session.events);
  session.client.disconnect(None, None).await;
  session.client.disconnect(None, None).await;
  let events = drain(&mut session.events);
  assert_eq!(disconnected(&events).0, "Client disconnected");
}

#[tokio::test]
async fn reconnecting_after_a_disconnect() {
  let mut session = Session::start().await;
  session.client.disconnect(None, None).await;
  session.server.closed().await;
  let connecting = connect(&session.client, &session.fake, CancellationToken::new());
  let mut server = session.fake.accept().await;
  server.accept_login().await;
  server.read_configuration().await;
  connecting.await.unwrap().unwrap();
  assert_eq!(session.client.state(), ClientState::READY);
}

/// The clock is paused only while nothing waits on the socket: a paused clock jumps to the next
/// timer whenever the runtime is idle, which can be before loopback data is seen.
#[tokio::test]
async fn the_server_connection_never_times_out_and_is_pinged_each_minute() {
  let mut session = Session::start().await;
  tokio::time::pause();
  tokio::time::sleep(Duration::from_secs(59)).await;
  assert_eq!(session.client.state(), ClientState::READY);
  tokio::time::sleep(Duration::from_secs(2)).await;
  tokio::time::resume();
  let ping = session.server.frame().await;
  assert_eq!(ServerRequest::decode(&ping).unwrap(), ServerRequest::Ping);
  tokio::time::pause();
  tokio::time::sleep(Duration::from_secs(3600)).await;
  assert_eq!(session.client.state(), ClientState::READY);
}

#[tokio::test]
async fn a_disposed_client_refuses_to_connect() {
  let session = Session::start().await;
  session.client.dispose().await;
  let address = session.fake.address();
  let error = session
    .client
    .connect(
      &address.ip().to_string(),
      address.port(),
      USERNAME,
      PASSWORD,
      &CancellationToken::new(),
    )
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Disposed));
  assert!(session.client.cancellation().is_cancelled());
}

#[tokio::test]
async fn a_busy_listen_port_fails_the_connect_before_the_server_is_contacted() {
  let fake = FakeServer::start().await;
  let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let (client, _handoffs) = Client::new(ClientOptions {
    enable_listener: true,
    listen_ip_address: [127, 0, 0, 1].into(),
    listen_port: taken.local_addr().unwrap().port(),
    ..ClientOptions::default()
  })
  .unwrap();
  let error = connect(&client, &fake, CancellationToken::new())
    .await
    .unwrap()
    .unwrap_err();
  assert!(matches!(error, ClientError::Listen { .. }), "{error:?}");
  assert_eq!(client.state(), ClientState::DISCONNECTED);
  // nothing reached the server
  let accepted = tokio::time::timeout(Duration::from_millis(200), fake.accept()).await;
  assert!(accepted.is_err());
}
