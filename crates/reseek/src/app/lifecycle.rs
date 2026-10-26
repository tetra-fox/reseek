//! The order the daemon's work starts in once it is built, and the order it stops in.
//!
//! Starting: the client's events and the options are followed, the transfers the last run left
//! unfinished are ended, the release feed is read, the shares are initialized, the clock
//! starts, and then the watchdog connects, unless this is a relay agent, connecting is turned
//! off or there are no credentials. Stopping, once the HTTP host has stopped: the watchdog
//! stops, the client disconnects and is disposed, and the events this raised are handled.

use tracing::{Instrument, info, info_span, warn};

use super::AppState;
use crate::clock::Every;
use crate::config::enums::RelayMode;
use crate::network;
use crate::transfers::cleanup;
use crate::version::RECHECK;

/// Starts the daemon's work in order, in the background.
pub(super) fn start(app: &AppState) {
  app.network.follow_events(app);
  tokio::spawn(network::follow_changes(app.clone()).instrument(info_span!("client_options")));
  tokio::spawn(network::follow_shared_counts(app.clone()).instrument(info_span!("shared_counts")));
  tokio::spawn(run(app.clone()).instrument(info_span!("startup")));
}

async fn run(app: AppState) {
  info!("Application started");
  match cleanup::end_unfinished(&app.transfers).await {
    Ok(downloads) if !downloads.is_empty() => {
      info!(
        downloads = downloads.len(),
        "downloads active at the last stop are not resumed yet"
      );
    }
    Ok(_) => {}
    Err(e) => warn!(error = %e, "transfers left unfinished at the last stop not ended"),
  }
  let options = app.config.current().values.clone();
  let soulseek = &options.soulseek;
  info!(
    address = soulseek.listen_ip_address,
    port = soulseek.listen_port,
    "Listening for incoming connections"
  );
  let proxy = &soulseek.connection.proxy;
  if proxy.enabled {
    info!(address = ?proxy.address, port = ?proxy.port, "Using Proxy");
  }
  if !options.flags.no_version_check {
    tokio::spawn(check_versions(app.clone()).in_current_span());
  }
  app.shares.initialized().await;

  let observed = app.clone();
  app.clock.every(Every::Minute, "client figures", move |_| {
    network::observe_client(&observed);
    async {}
  });
  app.clock.start();

  let relay = &options.relay;
  if relay.enabled && relay.mode == RelayMode::Agent {
    info!("Running in Agent relay mode; not connecting to the Soulseek server.");
    return;
  }
  if relay.enabled {
    info!("Running in Controller relay mode.  Listening for incoming Agent connections.");
    if relay.mode == RelayMode::Debug {
      warn!("Running in Debug relay mode; connecting to controller");
    }
  }
  if options.flags.no_connect {
    warn!("Not connecting to the Soulseek server; 'no-connect' option is enabled");
  } else if network::credentials(soulseek).is_none() {
    network::warn_no_credentials();
  } else {
    app.network.watchdog().start();
  }
}

/// Reads the release feed now and once a day until the daemon stops.
async fn check_versions(app: AppState) {
  let mut every = tokio::time::interval(RECHECK);
  loop {
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      _ = every.tick() => {}
    }
    if let Err(e) = app.releases.update(&app.state).await {
      warn!(error = e, "Failed to check version");
    }
  }
}

/// Stops the daemon's work in order.
pub(super) async fn stop(app: &AppState) {
  info!("Application is shutting down");
  app.network.stop().await;
}
