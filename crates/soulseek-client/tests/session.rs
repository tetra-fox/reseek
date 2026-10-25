//! Connecting, logging in and disconnecting against soulfind: the state transitions and their
//! events, a refused login, being kicked by a second login with our name, and a server
//! connection that stays up however long it is idle.

mod support;

use std::time::Duration;

use soulseek_client::{
  Client, ClientError, ClientOptions, ClientState, ConnectionErrorKind, Event, WaitKey,
};
use soulseek_proto::server::{GetPeerAddressRequest, GetPeerAddressResponse, GetStatusRequest};
use soulseek_proto::{PeerCode, ServerCode, ServerRequest};
use support::soulfind::{self, PASSWORD};
use support::{disconnected, drain, free_port, next_event, options, transitions, user_key};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn login_raises_its_transitions_before_anything_the_server_sends_after_it() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let username = soulfind::username("login");
  let client = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  server.connect(&client, &username, PASSWORD).await.unwrap();

  assert_eq!(client.state(), ClientState::READY);
  assert_eq!(client.username(), Some(username));
  assert_eq!(client.address(), Some(server.address().ip().to_string()));
  assert_eq!(client.ip_endpoint(), Some(server.address()));
  let info = client.server_info();
  assert_eq!(info.is_supporter, Some(false));
  assert!(info.own_public_ip.is_some());

  // soulfind sends the room list right after the login reply
  let mut seen = Vec::new();
  next_event(&mut events, |event| {
    let done = matches!(event, Event::RoomListReceived(_));
    seen.push(event);
    done.then_some(())
  })
  .await;
  let order: Vec<&str> = seen
    .iter()
    .map(|event| match event {
      Event::StateChanged { .. } => "StateChanged",
      Event::Connected => "Connected",
      Event::LoggedIn => "LoggedIn",
      Event::ServerInfoReceived(_) => "ServerInfoReceived",
      Event::RoomListReceived(_) => "RoomListReceived",
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
      "LoggedIn",
      "RoomListReceived"
    ]
  );
  assert_eq!(
    transitions(&seen),
    [
      (
        ClientState::DISCONNECTED,
        ClientState::CONNECTING,
        "Connecting".into()
      ),
      (
        ClientState::CONNECTING,
        ClientState::CONNECTED,
        format!("Connected to {}", server.address())
      ),
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
async fn the_server_learns_our_listen_port_even_when_we_do_not_listen() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let port = free_port();
  let alice = server
    .login(
      "port",
      ClientOptions {
        listen_port: port,
        ..options()
      },
    )
    .await;
  let bob = server.login("asker", options()).await;
  let address: GetPeerAddressResponse = bob
    .ask(
      user_key(ServerCode::GetPeerAddress, &alice.username),
      ServerRequest::GetPeerAddress(GetPeerAddressRequest {
        username: alice.username.clone(),
      }),
    )
    .await;
  assert_eq!(address.port, u32::from(port));
  assert_eq!(Some(address.ip), alice.client.server_info().own_public_ip);
}

#[tokio::test]
async fn connecting_twice_is_refused() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("twice", options()).await;
  let error = server
    .connect(&alice.client, &alice.username, PASSWORD)
    .await
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::InvalidOperation(message) if message == "The client is already connected")
  );
  assert_eq!(alice.client.state(), ClientState::READY);
}

#[tokio::test]
async fn a_wrong_password_is_rejected_with_the_server_reason() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let registered = server.login("password", options()).await;
  registered.client.disconnect(None, None).await;

  let client = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let error = server
    .connect(&client, &registered.username, "not the password")
    .await
    .unwrap_err();
  assert!(
    matches!(&error, ClientError::LoginRejected(message) if message == "The server rejected login attempt: INVALIDPASS"),
    "{error:?}"
  );
  let (message, cause) = disconnected(&drain(&mut events));
  assert_eq!(message, "The server rejected login attempt: INVALIDPASS");
  assert!(matches!(cause, Some(ClientError::LoginRejected(_))));
  assert_eq!(client.state(), ClientState::DISCONNECTED);
  assert_eq!(client.username(), None);
}

#[tokio::test]
async fn a_second_login_with_our_name_kicks_us() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut first = server.login("kicked", options()).await;
  let second = Client::new(options()).unwrap();
  server
    .connect(&second, &first.username, PASSWORD)
    .await
    .unwrap();
  first
    .event(|event| matches!(event, Event::KickedFromServer).then_some(()))
    .await;
  let (message, cause) = first
    .event(|event| match event {
      Event::Disconnected { message, error } => Some((message, error)),
      _ => None,
    })
    .await;
  assert_eq!(message, "Kicked from server");
  assert!(matches!(cause, Some(ClientError::KickedFromServer)));
  assert_eq!(first.client.state(), ClientState::DISCONNECTED);
  assert_eq!(second.state(), ClientState::READY);
}

#[tokio::test]
async fn disconnecting_fails_server_waits_and_keeps_peer_waits() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut alice = server.login("waits", options()).await;
  let waiter = alice.client.waiter().clone();
  let cancel = CancellationToken::new();
  let server_wait = waiter.wait::<()>(
    user_key(ServerCode::GetStatus, "nobody asked"),
    None,
    cancel.clone(),
  );
  let peer_key = WaitKey::Peer {
    code: PeerCode::InfoResponse,
    username: "bob".into(),
  };
  let peer_wait = waiter.wait::<u32>(peer_key.clone(), None, cancel.clone());

  alice.client.disconnect(Some("done"), None).await;

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
  assert_eq!(disconnected(&drain(&mut alice.events)).0, "done");
}

#[tokio::test]
async fn disconnecting_twice_raises_disconnected_once() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let mut alice = server.login("twice", options()).await;
  alice.client.disconnect(None, None).await;
  alice.client.disconnect(None, None).await;
  let events = drain(&mut alice.events);
  assert_eq!(disconnected(&events).0, "Client disconnected");
  assert_eq!(alice.client.username(), None);
  assert_eq!(alice.client.address(), None);
  assert_eq!(alice.client.ip_endpoint(), None);
}

#[tokio::test]
async fn a_disposed_client_refuses_to_connect() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("disposed", options()).await;
  alice.client.dispose().await;
  let error = server
    .connect(&alice.client, &alice.username, PASSWORD)
    .await
    .unwrap_err();
  assert!(matches!(error, ClientError::Disposed));
  assert!(alice.client.cancellation().is_cancelled());
}

#[tokio::test]
async fn a_refused_connection_is_a_connect_error() {
  let port = free_port();
  let client = Client::new(options()).unwrap();
  let mut events = client.subscribe();
  let error = client
    .connect(
      "127.0.0.1",
      port,
      "nobody",
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
      "Failed to connect: Failed to connect to 127.0.0.1:{port}: "
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

/// The clock is paused only while nothing waits on the socket: a paused clock jumps to the next
/// timer whenever the runtime is idle, which can be before loopback data is seen.
#[tokio::test]
async fn the_server_connection_never_times_out() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let alice = server.login("idle", options()).await;
  tokio::time::pause();
  tokio::time::sleep(Duration::from_secs(3600)).await;
  tokio::time::resume();
  assert_eq!(alice.client.state(), ClientState::READY);
  let status: soulseek_proto::server::UserStatus = alice
    .ask(
      user_key(ServerCode::GetStatus, &alice.username),
      ServerRequest::GetStatus(GetStatusRequest {
        username: alice.username.clone(),
      }),
    )
    .await;
  assert_eq!(status.username, alice.username);
}
