//! Following the options while running: a change to the search request filters compiles them
//! again, a change to the Soulseek, upload or download options or to whether we take private
//! room invitations patches the client, a change to the listener or to port mapping moves the
//! mapping on the router, new credentials log in again at once, another change that takes
//! effect only on a new session is flagged while connected, a change to how we reach the server
//! starts the watchdog over, and rooms added to the configuration are joined.

use std::sync::Arc;

use cascade::Loaded;
use soulseek_client::{Client, ClientError};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};

use super::options;
use crate::app::AppState;
use crate::config::Options;

/// Applies every change of the options to the client until the daemon stops. Each change is
/// compared with the options last applied, so a change that could not be applied, or one that
/// was missed, is applied with the next.
pub async fn follow_changes(app: AppState) {
  let mut changes = app.config.changes();
  // the client was built from the options the daemon started with; a change made before this
  // subscribed is applied at once
  let mut applied = app.startup.values.clone();
  let mut current = app.config.current();
  loop {
    if let Some(now) = apply(&app, &applied, &current).await {
      applied = now;
    }
    current = tokio::select! {
      () = app.shutdown.cancelled() => return,
      change = changes.recv() => match change {
        Ok(change) => change.current.clone(),
        Err(RecvError::Lagged(missed)) => {
          debug!(missed, "options changes missed; the client follows the current options");
          app.config.current()
        }
        Err(RecvError::Closed) => return,
      },
    };
  }
}

/// Applies `current` to the client, which runs with `applied`. Returns the options now in
/// effect: `current`, with the listener's address and port left as in `applied` when the
/// listener could not move, so the next change tries them again. `None` when the client could
/// not be patched at all.
async fn apply(
  app: &AppState,
  applied: &Options,
  current: &Arc<Loaded<Options>>,
) -> Option<Options> {
  let network = &app.network;
  let mut values = current.values.clone();
  if applied.filters.search.request != values.filters.search.request {
    network
      .edges
      .searches
      .refilter(&values.filters.search.request);
  }
  let mut reconnect_required = false;
  if applied.soulseek != values.soulseek
    || applied.transfers.upload != values.transfers.upload
    || applied.transfers.download != values.transfers.download
    || applied.chat.accept_private_room_invitations != values.chat.accept_private_room_invitations
  {
    let _reconfiguring = network.reconfiguring().lock().await;
    let alternative = app.state.current().alternative_speed_limits;
    let patch = match options::patch(applied, &values, alternative) {
      Ok(patch) => patch,
      Err(e) => {
        warn!(error = e, "Failed to apply option update");
        return None;
      }
    };
    match network.client().reconfigure_options(patch).await {
      Ok(required) => reconnect_required = required,
      // the client took the rest of the patch and kept its listener where it was
      Err(e @ ClientError::Listen { .. }) => {
        warn!(error = %e, "Failed to move the listener; the other option updates are applied");
        values.soulseek.listen_ip_address = applied.soulseek.listen_ip_address.clone();
        values.soulseek.listen_port = applied.soulseek.listen_port;
      }
      Err(e) => {
        warn!(error = %e, "Failed to apply option update");
        return None;
      }
    }
  }
  let relogin = new_credentials(network.client(), applied, &values);
  // the reconnect-class keys changed since the session began; while disconnected the next
  // connect takes them without a flag, as logging in again does
  let connected = network.client().state().is_connected();
  if !relogin && ((connected && !current.pending_reconnect.is_empty()) || reconnect_required) {
    let already = app.state.current().pending_reconnect;
    app.state.update(|state| state.pending_reconnect = true);
    if !already {
      info!(
        keys = current.pending_reconnect.join(", "),
        reconnect_required,
        "One or more updated Soulseek options requires the client to be disconnected, then \
         reconnected to the network to take effect."
      );
    }
  }
  let (was, now) = (&applied.soulseek, &values.soulseek);
  if was.listen_ip_address != now.listen_ip_address
    || was.listen_port != now.listen_port
    || was.port_mapping != now.port_mapping
  {
    network.listening_changed(now.port_mapping);
  }
  // stored in the poll that saw the client take the options, so before a session that began
  // with them is connected
  network.set_applied(current.clone());
  if relogin {
    if network.log_in_again().await {
      info!("Soulseek username or password changed; logging in again");
    } else {
      debug!("Soulseek username or password changed while disconnected by the user");
    }
  } else if network.watchdog().is_enabled() && reaches_differently(applied, &values) {
    info!("Options changed, restarting (re)connection process...");
    network.watchdog().restart();
  }
  if applied.rooms != values.rooms {
    app
      .messaging
      .configured_rooms_changed(&applied.rooms, &values.rooms);
  }
  Some(values)
}

/// Whether the username or password changed to ones that can log in, and the session is not
/// logged in with them already, as it is once the server changed our password.
fn new_credentials(client: &Client, previous: &Options, current: &Options) -> bool {
  let (was, now) = (&previous.soulseek, &current.soulseek);
  if was.username == now.username && was.password == now.password {
    return false;
  }
  super::credentials(now)
    .is_some_and(|(username, password)| !client.logged_in_as(username, password))
}

/// Whether the server's address, our credentials or the connection options differ.
fn reaches_differently(previous: &Options, current: &Options) -> bool {
  let (previous, current) = (&previous.soulseek, &current.soulseek);
  previous.address != current.address
    || previous.port != current.port
    || previous.username != current.username
    || previous.password != current.password
    || previous.connection != current.connection
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use api_types::state::Reachable;
  use serde_json::json;
  use soulseek_proto::ServerRequest;

  use crate::testing::soulseek::Server;
  use crate::testing::{app, request};

  #[tokio::test]
  async fn a_live_change_reaches_the_client_without_a_reconnect() {
    let app = app(
      "remote_configuration = true\n[web.authentication]\n\
       trusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let change = json!({ "set": {
      "transfers.upload.speed_limit": 64,
      "transfers.upload.slots": 3,
      "chat.accept_private_room_invitations": false,
    } });
    let answer = app
      .send(request("PATCH", "/api/v0/options", Some(change)))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    let client = app.state.network.client();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while client.options().maximum_upload_speed != 64 {
      assert!(tokio::time::Instant::now() < deadline, "never applied");
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(client.options().maximum_concurrent_uploads, 3);
    assert!(!client.options().accept_private_room_invitations);
    assert!(!app.state.state.current().pending_reconnect);
  }

  #[tokio::test]
  async fn a_new_listening_port_is_shown_and_judged_anew() {
    let app = app(
      "remote_configuration = true\n[web.authentication]\n\
       trusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let network = &app.state.network;
    let state = &app.state.state;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let waiting = |what: &str, ready: &dyn Fn(&api_types::state::ListenState) -> bool| {
      assert!(tokio::time::Instant::now() < deadline, "never {what}");
      !ready(&state.current().server.listen)
    };
    network.reachability.logged_in();
    for token in 0..5 {
      network.reachability.search_answered(token);
    }
    while waiting("unreachable", &|listen| listen.reachable == Reachable::No) {
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let change = json!({ "set": { "soulseek.listen_port": 50311 } });
    let answer = app
      .send(request("PATCH", "/api/v0/options", Some(change)))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    while waiting("judged anew on the new port", &|listen| {
      listen.port == 50311 && listen.reachable == Reachable::Unknown
    }) {
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  }

  #[tokio::test]
  async fn a_listener_that_cannot_move_does_not_keep_a_room_added_later_from_being_joined() {
    let listen = crate::testing::ports::free_port();
    let app = app(
      &format!(
        "remote_configuration = true\n[web.authentication]\n\
         trusted_networks = [\"127.0.0.0/8\"]\n\
         [soulseek]\nlisten_port = {listen}\nport_mapping = false\n"
      ),
      &[],
    )
    .await;
    let mut server = Server::silent().await;
    server.login(&app).await;
    let taken = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let changes = [
      json!({ "set": { "soulseek.listen_port": taken.local_addr().unwrap().port() } }),
      json!({ "set": { "rooms": ["lobby"] } }),
    ];
    for change in changes {
      let answer = app
        .send(request("PATCH", "/api/v0/options", Some(change)))
        .await;
      assert_eq!(answer.status, 200, "{:?}", answer.body);
    }
    let joined = server
      .receives(
        Duration::from_secs(5),
        |request| matches!(request, ServerRequest::JoinRoom(join) if join.room == "lobby"),
      )
      .await;
    assert!(joined, "the room was not joined");
  }

  #[tokio::test]
  async fn new_credentials_log_in_again_at_once_unless_a_disconnect_was_asked_for() {
    use soulseek_proto::ServerResponse;
    use soulseek_proto::server::NewPassword;

    let mut server = Server::start(|request| match request {
      ServerRequest::NewPassword(asked) => vec![ServerResponse::NewPassword(NewPassword {
        new_password: asked.new_password.clone(),
      })],
      _ => Vec::new(),
    })
    .await;
    // no password, so nothing connects at the start
    let app = crate::testing::soulseek::app(&format!(
      "remote_configuration = true\n[web.authentication]\n\
       trusted_networks = [\"127.0.0.0/8\"]\n\
       [soulseek]\naddress = \"127.0.0.1\"\nport = {}\nusername = \"me\"\n",
      server.port
    ))
    .await;
    let send = async |method: &str, uri: &str, body: serde_json::Value| {
      let answer = app.send(request(method, uri, Some(body))).await;
      assert!(answer.status.is_success(), "{uri}: {:?}", answer.body);
    };
    let set = async |key: &str, value: &str| {
      send("PATCH", "/api/v0/options", json!({ "set": { key: value } })).await;
    };
    let client = app.state.network.client();
    let logged_in = async || {
      let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
      while !client.state().is_logged_in() {
        assert!(tokio::time::Instant::now() < deadline, "never logged in");
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    };
    let login = |request: &ServerRequest| match request {
      ServerRequest::Login(login) => Some(login.username.clone()),
      _ => None,
    };
    let another_login = |request: &ServerRequest| matches!(request, ServerRequest::Login(_));

    set("soulseek.password", "secret").await;
    assert_eq!(server.received(login).await, "me");
    logged_in().await;
    set("soulseek.username", "other").await;
    assert_eq!(server.received(login).await, "other");
    logged_in().await;
    assert!(!app.state.state.current().pending_reconnect);

    // the server changed the password of the session, which goes on
    let change = json!({ "password": "n3w-password" });
    send("PUT", "/api/v0/application/password", change).await;
    let wait = Duration::from_secs(1);
    assert!(!server.receives(wait, another_login).await);

    app.state.network.disconnect(None).await;
    set("soulseek.username", "third").await;
    assert!(
      !server.receives(wait, another_login).await,
      "the disconnect asked for stays"
    );
  }
}
