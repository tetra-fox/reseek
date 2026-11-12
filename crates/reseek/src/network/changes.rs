//! Following the options while running: a change to the search request filters compiles them
//! again, a change to the Soulseek, upload or download options or to whether we take private
//! room invitations patches the client, a change to the listener or to port mapping moves the
//! mapping on the router, a change that takes effect only on a new session is flagged while
//! connected, a change to how we reach the server starts the watchdog over, and rooms added to
//! the configuration are joined.

use std::sync::Arc;

use cascade::Loaded;
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
  let mut applied = app.config.current();
  loop {
    let current = tokio::select! {
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
    if apply(&app, &applied.values, &current).await {
      applied = current;
    }
  }
}

/// Applies `current` to the client, which runs with `applied`; false when the client could
/// not be patched.
async fn apply(app: &AppState, applied: &Options, current: &Arc<Loaded<Options>>) -> bool {
  let network = &app.network;
  let values = &current.values;
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
    let patch = match options::patch(applied, values, alternative) {
      Ok(patch) => patch,
      Err(e) => {
        warn!(error = e, "Failed to apply option update");
        return false;
      }
    };
    match network.client().reconfigure_options(patch).await {
      Ok(required) => reconnect_required = required,
      Err(e) => {
        warn!(error = %e, "Failed to apply option update");
        return false;
      }
    }
  }
  // the reconnect-class keys changed since the session began; while disconnected the next
  // connect takes them without a flag
  let connected = network.client().state().is_connected();
  if (connected && !current.pending_reconnect.is_empty()) || reconnect_required {
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
  if network.watchdog().is_enabled() && reaches_differently(applied, values) {
    info!("Options changed, restarting (re)connection process...");
    network.watchdog().restart();
  }
  if applied.rooms != values.rooms {
    app
      .messaging
      .configured_rooms_changed(&applied.rooms, &values.rooms);
  }
  true
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
    let free = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let listen = free.local_addr().unwrap().port();
    drop(free);
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
}
