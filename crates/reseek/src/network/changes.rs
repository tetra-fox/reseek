//! Following the options while running: a change to the search request filters compiles them
//! again, a change to the Soulseek, upload or download options patches the client, a change
//! that takes effect only on a new session is flagged while connected, and a change to how we
//! reach the server starts the watchdog over.

use std::sync::Arc;

use cascade::Loaded;
use soulseek_client::ClientState;
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
  {
    let patch = match options::patch(applied, values) {
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
  let connected = network.client().state().contains(ClientState::CONNECTED);
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
  if network.watchdog().is_enabled() && reaches_differently(applied, values) {
    info!("Options changed, restarting (re)connection process...");
    network.watchdog().restart();
  }
  if applied.rooms != values.rooms {
    debug!("room list changed; the rooms added are joined at the next login");
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

  use serde_json::json;

  use crate::testing::{app, request};

  #[tokio::test]
  async fn a_live_change_reaches_the_client_without_a_reconnect() {
    let app = app(
      "remote_configuration = true\n[web.authentication]\n\
       trusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let change =
      json!({ "set": { "transfers.upload.speed_limit": 64, "transfers.upload.slots": 3 } });
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
    assert!(!app.state.state.current().pending_reconnect);
  }
}
