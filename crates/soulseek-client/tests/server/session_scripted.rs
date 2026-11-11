//! The session against a scripted server, for what soulfind cannot show: the bytes the client
//! writes at login and after it, a login refused with a detail, a server that never answers the
//! login or closes the connection, canceling a login in progress, the write-liveness ping, and
//! the configuration re-sent when the options change.

use std::time::Duration;

use crate::support::{
  FakeServer, PASSWORD, Session, USERNAME, disconnected, drain, listening_options, login_success,
  next_event, options,
};
use soulseek_client::{
  Client, ClientError, ClientOptions, ClientOptionsPatch, ClientState, ConnectionErrorKind, Event,
};
use soulseek_proto::server::{
  AcceptChildren, BranchLevel, BranchRoot, HaveNoParents, LoginResponse, PrivateRoomToggle,
  SetListenPort,
};
use soulseek_proto::{
  PROTOCOL_MAJOR_VERSION, PROTOCOL_MINOR_VERSION, ServerRequest, ServerResponse,
};
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

#[tokio::test]
async fn the_login_and_the_configuration_sent_after_it() {
  let fake = FakeServer::start().await;
  let client = Client::new(options()).unwrap();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;

  let (login, port) = server.read_login().await;
  assert_eq!(login.username, USERNAME);
  assert_eq!(login.password.expose(), PASSWORD);
  assert_eq!(login.major_version, PROTOCOL_MAJOR_VERSION);
  assert_eq!(login.minor_version, PROTOCOL_MINOR_VERSION);
  assert_eq!(login.hash.expose(), "bb6de2fa07892f34b0e1f62df88f3384");
  assert_eq!(port, 50_000);
  assert_eq!(client.state(), ClientState::LoggingIn);
  server.send(&crate::support::login_success()).await;

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
  assert_eq!(
    client.server_info().own_public_ip,
    Some(crate::support::PUBLIC_IP)
  );
}

async fn rejected_login(response: LoginResponse) -> ClientError {
  let fake = FakeServer::start().await;
  let client = Client::new(options()).unwrap();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&ServerResponse::Login(response)).await;
  connecting.await.unwrap().unwrap_err()
}

#[tokio::test]
async fn a_rejected_login_appends_the_detail() {
  let error = rejected_login(LoginResponse::Failure {
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
  let client = Client::new(ClientOptions {
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
async fn the_server_closing_during_login_is_a_connect_error() {
  let fake = FakeServer::start().await;
  let client = Client::new(options()).unwrap();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  drop(server);
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Connect { .. }), "{error:?}");
}

#[tokio::test]
async fn canceling_the_connect_while_logging_in() {
  let fake = FakeServer::start().await;
  let client = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let cancel = CancellationToken::new();
  let connecting = connect(&client, &fake, cancel.clone());
  let mut server = fake.accept().await;
  server.read_login().await;
  cancel.cancel();
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Canceled), "{error:?}");
  assert!(matches!(
    disconnected(&drain(&mut events)).1,
    Some(ClientError::Canceled)
  ));
  assert_eq!(client.state(), ClientState::Disconnected);
  server.closed().await;
}

#[tokio::test]
async fn disconnecting_cancels_a_connect_in_progress() {
  let fake = FakeServer::start().await;
  let client = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  client.disconnect(Some("user asked"), None).await;
  let error = connecting.await.unwrap().unwrap_err();
  assert!(matches!(error, ClientError::Canceled), "{error:?}");
  let (message, cause) = disconnected(&drain(&mut events));
  assert_eq!(message, "user asked");
  assert!(cause.is_none());
  assert_eq!(client.state(), ClientState::Disconnected);
  server.closed().await;
}

#[tokio::test]
async fn a_connect_queued_behind_another_login_is_refused_for_another_name() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  // both connects pass their first check while the disconnect stops the listener, so the second
  // waits for the first to log in
  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let serve = async {
    let mut server = fake.accept().await;
    server.read_login().await;
    server.send(&login_success()).await;
    server
  };
  let ((), same, other, _server) = tokio::join!(
    client.disconnect(None, None),
    client.connect(&host, address.port(), USERNAME, PASSWORD, &cancel),
    client.connect(&host, address.port(), "someone_else", PASSWORD, &cancel),
    serve,
  );
  same.unwrap();
  assert!(
    matches!(&other, Err(ClientError::InvalidOperation(message)) if message == "The client is already connected as another user"),
    "{other:?}"
  );
  assert_eq!(client.username().as_deref(), Some(USERNAME));
  drop(server);
}

#[tokio::test]
async fn a_connect_queued_behind_a_login_that_ends_waits_and_connects_as_itself() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  // both connects pass their first check while the disconnect stops the listener; the first
  // logs in, and its session ends before the second takes its turn
  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let serve = async {
    let mut joined = fake.accept().await;
    joined.read_login().await;
    joined.send(&login_success()).await;
    let mut queued = fake.accept().await;
    let (login, _) = queued.read_login().await;
    queued.send(&login_success()).await;
    (joined, queued, login.username)
  };
  let joined_then_ended = async {
    let joined = client
      .connect(&host, address.port(), USERNAME, PASSWORD, &cancel)
      .await;
    client.disconnect(None, None).await;
    joined
  };
  let ((), joined, queued, (_joined, _queued, name)) = tokio::join!(
    client.disconnect(None, None),
    joined_then_ended,
    client.connect(&host, address.port(), "someone_else", PASSWORD, &cancel),
    serve,
  );
  joined.unwrap();
  queued.unwrap();
  assert_eq!(name, "someone_else");
  assert_eq!(client.username().as_deref(), Some("someone_else"));
  drop(server);
}

#[tokio::test]
async fn a_connect_dropped_while_another_waits_leaves_a_session_the_other_can_take() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let serve = async {
    let mut hung = fake.accept().await;
    hung.read_login().await;
    // never answers; the queued connect's session is answered
    let mut second = fake.accept().await;
    let (login, _) = second.read_login().await;
    second.send(&login_success()).await;
    (hung, second, login.username)
  };
  let dropped = tokio::time::timeout(
    Duration::from_millis(300),
    client.connect(&host, address.port(), "dropped", PASSWORD, &cancel),
  );
  let ((), timed_out, queued, (_hung, _second, name)) = tokio::join!(
    client.disconnect(None, None),
    dropped,
    client.connect(&host, address.port(), "queued", PASSWORD, &cancel),
    serve,
  );
  assert!(timed_out.is_err());
  queued.unwrap();
  assert_eq!(name, "queued");
  drop(server);
}

#[tokio::test]
async fn the_server_closing_the_connection_ends_the_session() {
  let mut session = Session::start().await;
  drain(&mut session.events);
  let address = session.fake.address();
  drop(session.server.stream);
  let (message, cause) = next_event(&mut session.events, |event| match event {
    Event::Disconnected { message, error, .. } => Some((message, error)),
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
  assert_eq!(session.client.state(), ClientState::Disconnected);
  assert_eq!(session.client.username(), None);
  assert_eq!(session.client.address(), None);
  assert_eq!(session.client.ip_endpoint(), None);
}

/// The clock is paused only while nothing waits on the socket: a paused clock jumps to the next
/// timer whenever the runtime is idle, which can be before loopback data is seen.
#[tokio::test]
async fn the_server_is_pinged_each_minute() {
  let mut session = Session::start().await;
  tokio::time::pause();
  tokio::time::sleep(Duration::from_secs(59)).await;
  tokio::time::resume();
  let marker = ServerRequest::Ping;
  assert!(
    tokio::time::timeout(Duration::from_millis(100), session.server.frame())
      .await
      .is_err(),
    "nothing before the first minute"
  );
  tokio::time::pause();
  tokio::time::sleep(Duration::from_secs(2)).await;
  tokio::time::resume();
  let ping = session.server.frame().await;
  assert_eq!(ServerRequest::decode(&ping).unwrap(), marker);
}

#[tokio::test]
async fn a_busy_listen_port_fails_the_connect_before_the_server_is_contacted() {
  let fake = FakeServer::start().await;
  let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let client = Client::new(ClientOptions {
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
  assert_eq!(client.state(), ClientState::Disconnected);
  // nothing reached the server
  let accepted = tokio::time::timeout(Duration::from_millis(200), fake.accept()).await;
  assert!(accepted.is_err());
}

#[tokio::test]
async fn a_reconfigure_resends_the_configuration_and_the_status_only_when_it_changed() {
  let mut session = Session::start().await;
  let port = crate::support::free_port();
  session
    .client
    .reconfigure_options(ClientOptionsPatch {
      listen_port: Some(port),
      ..ClientOptionsPatch::default()
    })
    .await
    .unwrap();
  let configuration = |port: u16| {
    [
      ServerRequest::SetListenPort(SetListenPort {
        port: u32::from(port),
        obfuscation: None,
      }),
      ServerRequest::PrivateRoomToggle(PrivateRoomToggle {
        accept_invitations: false,
      }),
    ]
  };
  // the distributed status has not changed, so it is not sent again
  let sent = [
    session.server.request().await,
    session.server.request().await,
  ];
  assert_eq!(sent, configuration(port));

  session
    .client
    .reconfigure_options(ClientOptionsPatch {
      enable_distributed_network: Some(false),
      ..ClientOptionsPatch::default()
    })
    .await
    .unwrap();
  let mut sent = Vec::new();
  for _ in 0..6 {
    sent.push(session.server.request().await);
  }
  assert_eq!(sent[..2], configuration(port));
  assert_eq!(
    sent[2..],
    [
      ServerRequest::BranchLevel(BranchLevel { level: 0 }),
      ServerRequest::BranchRoot(BranchRoot {
        root: USERNAME.into()
      }),
      ServerRequest::AcceptChildren(AcceptChildren { accept: false }),
      ServerRequest::HaveNoParents(HaveNoParents {
        have_no_parents: false
      }),
    ]
  );
}
