//! The session against a scripted server, for what soulfind cannot show: the bytes the client
//! writes at login and after it, a login refused with a detail, a server that never answers the
//! login or closes the connection, canceling a login in progress, the write-liveness ping, and
//! the configuration re-sent when the options change.

use std::time::Duration;

use crate::support::{
  FakeServer, Logs, PASSWORD, Session, USERNAME, disconnected, drain, listening_options,
  login_success, next_event, options, poll_once, transitions,
};
use soulseek_client::{
  Client, ClientError, ClientOptions, ClientOptionsPatch, ClientState, ConnectionErrorKind, Event,
};
use soulseek_proto::server::{
  AcceptChildren, BranchLevel, BranchRoot, HaveNoParents, LoginResponse, NewPassword,
  PrivateRoomToggle, SetListenPort,
};
use soulseek_proto::{
  PROTOCOL_MAJOR_VERSION, PROTOCOL_MINOR_VERSION, Secret, ServerRequest, ServerResponse,
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
async fn a_connect_with_its_token_canceled_already_begins_nothing() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let mut events = client.subscribe();
  let address = fake.address();
  let cancel = CancellationToken::new();
  cancel.cancel();
  let connected = client
    .connect(
      &address.ip().to_string(),
      address.port(),
      USERNAME,
      PASSWORD,
      &cancel,
    )
    .await;
  assert!(
    matches!(connected, Err(ClientError::Canceled)),
    "{connected:?}"
  );
  let raised = drain(&mut events);
  assert!(raised.is_empty(), "{raised:?}");
  let reached = tokio::time::timeout(Duration::from_millis(200), fake.accept()).await;
  assert!(reached.is_err(), "the server took a connection");
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
async fn a_connect_queued_behind_a_login_to_another_server_is_refused_under_the_same_name() {
  let fake = FakeServer::start().await;
  let elsewhere = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  // both connects pass their first check while the disconnect stops the listener, so the second
  // waits for the first to log in
  let address = fake.address();
  let other = elsewhere.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let serve = async {
    let mut server = fake.accept().await;
    server.read_login().await;
    server.send(&login_success()).await;
    server
  };
  let ((), here, there, _server) = tokio::join!(
    client.disconnect(None, None),
    client.connect(&host, address.port(), USERNAME, PASSWORD, &cancel),
    client.connect(&host, other.port(), USERNAME, PASSWORD, &cancel),
    serve,
  );
  here.unwrap();
  assert_eq!(client.ip_endpoint(), Some(address));
  // the session is with the first server, which is not the one the second connect asked for
  assert!(there.is_err(), "{there:?}");
  drop(server);
}

#[tokio::test]
async fn a_disconnect_while_a_connect_waits_for_the_last_session_to_end_cancels_that_connect() {
  let session = Session::with_options(listening_options()).await;
  let client = &session.client;
  let mut events = client.subscribe();
  let address = session.fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  // the connect starts while the first disconnect stops the listener and waits for it to end;
  // the second disconnect is asked for after the connect
  let connecting = tokio::time::timeout(
    Duration::from_secs(2),
    client.connect(&host, address.port(), USERNAME, PASSWORD, &cancel),
  );
  let ((), connected, ()) = tokio::join!(
    client.disconnect(None, None),
    connecting,
    client.disconnect(None, None),
  );
  let states: Vec<ClientState> = transitions(&drain(&mut events))
    .into_iter()
    .map(|(_, state, _)| state)
    .collect();
  assert!(
    matches!(connected, Ok(Err(ClientError::Canceled))),
    "{connected:?}, through {states:?}"
  );
  assert_eq!(client.state(), ClientState::Disconnected);
}

#[tokio::test]
async fn a_second_disconnect_returns_once_the_one_under_way_has_ended() {
  let session = Session::with_options(listening_options()).await;
  let client = &session.client;
  let second = async {
    client.disconnect(None, None).await;
    client.state()
  };
  // the first is polled first and waits for the listener to stop
  let ((), after_second) = tokio::join!(client.disconnect(None, None), second);
  assert_eq!(after_second, ClientState::Disconnected);
}

#[tokio::test]
async fn a_connect_queued_behind_a_login_that_fails_waits_and_connects_as_itself() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  // both connects pass their first check while the disconnect stops the listener; the first's
  // login is refused, and its session has ended by the time the second takes its turn
  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let serve = async {
    let mut refused = fake.accept().await;
    refused.read_login().await;
    refused
      .send(&ServerResponse::Login(LoginResponse::Failure {
        reason: "INVALIDPASS".into(),
        detail: None,
      }))
      .await;
    let mut queued = fake.accept().await;
    let (login, _) = queued.read_login().await;
    queued.send(&login_success()).await;
    (refused, queued, login.username)
  };
  let ((), refused, queued, (_refused, _queued, name)) = tokio::join!(
    client.disconnect(None, None),
    client.connect(&host, address.port(), USERNAME, PASSWORD, &cancel),
    client.connect(&host, address.port(), "someone_else", PASSWORD, &cancel),
    serve,
  );
  assert!(
    matches!(refused, Err(ClientError::LoginRejected(_))),
    "{refused:?}"
  );
  queued.unwrap();
  assert_eq!(name, "someone_else");
  assert_eq!(client.username().as_deref(), Some("someone_else"));
  drop(server);
}

#[tokio::test]
async fn a_disconnect_cancels_a_connect_still_waiting_for_the_lock() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  // both connects pass their first check while the disconnect stops the listener; the first
  // logs in and is disconnected while the second still waits for the lock it holds
  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let serve = async {
    let mut joined = fake.accept().await;
    joined.read_login().await;
    joined.send(&login_success()).await;
    joined
  };
  let joined_then_ended = async {
    let joined = client
      .connect(&host, address.port(), USERNAME, PASSWORD, &cancel)
      .await;
    client.disconnect(None, None).await;
    joined
  };
  let ((), joined, queued, _joined) = tokio::join!(
    client.disconnect(None, None),
    joined_then_ended,
    client.connect(&host, address.port(), "someone_else", PASSWORD, &cancel),
    serve,
  );
  joined.unwrap();
  assert!(matches!(queued, Err(ClientError::Canceled)), "{queued:?}");
  assert_eq!(client.state(), ClientState::Disconnected);
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
async fn a_disconnect_dropped_while_the_listener_stops_ends_disconnected_and_can_connect_again() {
  let session = Session::with_options(listening_options()).await;
  let client = session.client.clone();
  let mut disconnecting = Box::pin(client.disconnect(None, None));
  // polled once, it waits for the listener's task to end
  assert!(poll_once(&mut disconnecting).await.is_pending());
  drop(disconnecting);
  let ended = tokio::time::timeout(Duration::from_secs(2), async {
    while client.state() != ClientState::Disconnected {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await;
  assert!(ended.is_ok(), "the client stayed {:?}", client.state());

  let fake = FakeServer::start().await;
  let connecting = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.accept_login().await;
  server.read_configuration().await;
  connecting.await.unwrap().unwrap();
}

/// What a dropped connect does today once it has logged in: the session it made is ended as a
/// canceled attempt. Whether a logged in session should end with the call that made it is not
/// settled; this holds the behavior still until it is.
#[tokio::test]
async fn a_connect_dropped_while_sending_the_configuration_ends_the_logged_in_session_as_canceled()
{
  let fake = FakeServer::start().await;
  let client = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let mut connecting = Box::pin(client.connect(&host, address.port(), USERNAME, PASSWORD, &cancel));
  let mut server = tokio::select! {
    server = fake.accept() => server,
    connected = &mut connecting => panic!("connected without a server: {connected:?}"),
  };
  tokio::select! {
    _ = server.read_login() => {}
    connected = &mut connecting => panic!("connected without an answer: {connected:?}"),
  }
  server.send(&login_success()).await;
  // polled until it has logged in, which is when it writes the configuration
  while client.state() != ClientState::LoggedIn {
    assert!(poll_once(&mut connecting).await.is_pending());
    tokio::time::sleep(Duration::from_millis(1)).await;
  }
  drop(connecting);
  let (message, error) = next_event(&mut events, |event| match event {
    Event::Disconnected { message, error, .. } => Some((message, error)),
    _ => None,
  })
  .await;
  assert_eq!(message, "connect dropped");
  assert!(matches!(error, Some(ClientError::Canceled)), "{error:?}");
  server.closed().await;
}

#[test]
fn a_connect_dropped_outside_the_runtime_does_not_make_a_queued_connect_panic() {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  let session = runtime.block_on(Session::with_options(listening_options()));
  let client = session.client.clone();
  let address = session.fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let mut dropped = Box::pin(client.connect(&host, address.port(), "dropped", PASSWORD, &cancel));
  let (queued, _hung) = runtime.block_on(async {
    // both connects start while the disconnect stops the listener: the first takes the lock,
    // the second waits for it
    let mut disconnecting = Box::pin(client.disconnect(None, None));
    assert!(poll_once(&mut disconnecting).await.is_pending());
    assert!(poll_once(&mut dropped).await.is_pending());
    let mut queued = Box::pin({
      let (client, host, cancel) = (client.clone(), host.clone(), cancel.clone());
      async move {
        client
          .connect(&host, address.port(), "queued", PASSWORD, &cancel)
          .await
      }
    });
    assert!(poll_once(&mut queued).await.is_pending());
    let queued = tokio::spawn(queued);
    disconnecting.await;
    // the server takes the login and never answers it
    let hang = async {
      let mut hung = session.fake.accept().await;
      hung.read_login().await;
      hung
    };
    let (timed_out, hung) = tokio::join!(
      tokio::time::timeout(Duration::from_millis(300), &mut dropped),
      hang
    );
    assert!(timed_out.is_err());
    (queued, hung)
  });
  drop(dropped);
  let joined = runtime.block_on(async {
    let serve = async {
      let mut second = session.fake.accept().await;
      second.accept_login().await;
      second.discard().await;
    };
    tokio::select! {
      joined = queued => joined,
      () = serve => unreachable!("the server reads until the test ends"),
    }
  });
  assert!(joined.is_ok(), "the queued connect panicked: {joined:?}");
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

/// The error an application gives with the disconnect it asks for is its reason, as when it
/// stops; only a session lost on its own is a warning
#[tokio::test]
async fn a_disconnect_asked_for_is_no_warning_and_a_lost_session_is() {
  let logs = Logs::capture();
  let asked = Session::start().await;
  let reason = ClientError::client("the application is shutting down", None);
  asked
    .client
    .disconnect(Some("Shutting down"), Some(reason))
    .await;
  let mut lost = Session::start().await;
  drain(&mut lost.events);
  drop(lost.server.stream);
  next_event(&mut lost.events, |event| {
    matches!(event, Event::Disconnected { .. }).then_some(())
  })
  .await;
  let ended = logs.lines_with(&["disconnected from server"]);
  assert_eq!(ended.len(), 2, "{}", logs.text());
  assert!(
    ended[0].contains("INFO") && ended[0].contains("Shutting down"),
    "{}",
    ended[0]
  );
  assert!(
    ended[1].contains("WARN") && ended[1].contains("Read error"),
    "{}",
    ended[1]
  );
}

/// A connect that fails is tried again by the application, so each way it fails is warned about
/// once, and again only once a login came between
#[tokio::test]
async fn a_server_out_of_reach_is_warned_about_once_until_a_login() {
  let logs = Logs::capture();
  let session = Session::start().await;
  let client = &session.client;
  let closed = std::net::TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap();
  let fail = async || {
    client
      .connect(
        "127.0.0.1",
        closed.port(),
        USERNAME,
        PASSWORD,
        &CancellationToken::new(),
      )
      .await
      .unwrap_err();
  };
  let warned = || logs.lines_with(&["WARN", "disconnected from server"]).len();
  client.disconnect(None, None).await;
  for _ in 0..3 {
    fail().await;
  }
  assert_eq!(warned(), 1, "{}", logs.text());
  // a server that closes during the login fails another way
  for _ in 0..2 {
    let cut = connect(client, &session.fake, CancellationToken::new());
    drop(session.fake.accept().await);
    cut.await.unwrap().unwrap_err();
  }
  assert_eq!(warned(), 2, "{}", logs.text());
  let logging_in = connect(client, &session.fake, CancellationToken::new());
  let mut server = session.fake.accept().await;
  server.accept_login().await;
  server.read_configuration().await;
  logging_in.await.unwrap().unwrap();
  client.disconnect(None, None).await;
  // the way it failed last, which the login made news again
  let cut = connect(client, &session.fake, CancellationToken::new());
  drop(session.fake.accept().await);
  cut.await.unwrap().unwrap_err();
  assert_eq!(warned(), 3, "{}", logs.text());
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

#[tokio::test]
async fn a_connect_queued_behind_a_login_is_compared_with_the_password_changed_since() {
  let fake = FakeServer::start().await;
  let client = Client::new(listening_options()).unwrap();
  let first = connect(&client, &fake, CancellationToken::new());
  let mut server = fake.accept().await;
  server.read_login().await;
  server.send(&login_success()).await;
  first.await.unwrap().unwrap();

  // the first connect logs in again with the old password while the second, made with the new
  // one, waits for the lock the first holds
  let address = fake.address();
  let host = address.ip().to_string();
  let cancel = CancellationToken::new();
  let changed = "n3w password";
  let mut disconnecting = Box::pin(client.disconnect(None, None));
  let mut relogging = Box::pin(client.connect(&host, address.port(), USERNAME, PASSWORD, &cancel));
  let mut queued = Box::pin(client.connect(&host, address.port(), USERNAME, changed, &cancel));
  assert!(poll_once(&mut disconnecting).await.is_pending());
  assert!(poll_once(&mut relogging).await.is_pending());
  assert!(poll_once(&mut queued).await.is_pending());
  let serve = async {
    let mut relogged = fake.accept().await;
    relogged.read_login().await;
    relogged.send(&login_success()).await;
    relogged
  };
  let ((), relogged, mut relogged_server) = tokio::join!(disconnecting, relogging, serve);
  relogged.unwrap();
  let echo = async {
    relogged_server
      .request_matching(|request| matches!(request, ServerRequest::NewPassword(_)).then_some(()))
      .await;
    relogged_server
      .send(&ServerResponse::NewPassword(NewPassword {
        new_password: Secret::new(changed),
      }))
      .await;
  };
  let (changing, ()) = tokio::join!(client.change_password(changed), echo);
  changing.unwrap();
  let joined = tokio::time::timeout(Duration::from_secs(2), queued)
    .await
    .expect("the queued connect ends");
  assert!(joined.is_ok(), "{joined:?}");
  assert_eq!(client.state(), ClientState::LoggedIn);
  drop(server);
}
