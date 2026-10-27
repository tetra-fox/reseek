//! The order the daemon's work starts in once it is built, and the order it stops in.
//!
//! Starting: the transfers the last run left unfinished are ended and the downloads among them
//! marked to resume after the login, and the searches it left running are ended, before
//! anything can start new ones; then, in the background, the client's events and the options
//! are followed, whether other users can reach us is judged and the router is asked to forward
//! our peer port while we are logged in, the release feed is read, the shares are initialized,
//! the clock starts, and the watchdog connects, unless this is a relay agent, connecting is
//! turned off or there are no credentials. Stopping, once the HTTP host has stopped: the
//! running searches end as cancelled, the watchdog stops, the port mapping is removed, the
//! client disconnects and is disposed, the events this raised are handled, the session is
//! closed, the statistics fetched again and the transfer tasks end, and the transfers still
//! unfinished are ended as shut down.

use tracing::{Instrument, debug, info, info_span, warn};

use super::AppState;
use crate::clock::Every;
use crate::config::enums::RelayMode;
use crate::network;
use crate::transfers::{cleanup, prune};
use crate::version::RECHECK;

/// Ends what the last run left unfinished, then starts the daemon's work in order, in the
/// background.
pub(super) async fn start(app: &AppState) {
  // a transfer or a search started from here on is not one the last run left
  if let Err(e) = cleanup::end_unfinished(&app.transfers).await {
    warn!(error = %e, "transfers left unfinished at the last stop not ended");
  }
  app.searches.end_dangling().await;
  app.network.follow_events(app);
  app.network.follow_listening(app.shutdown.clone());
  tokio::spawn(network::follow_changes(app.clone()).instrument(info_span!("client_options")));
  tokio::spawn(network::follow_shared_counts(app.clone()).instrument(info_span!("shared_counts")));
  tokio::spawn(run(app.clone()).instrument(info_span!("startup")));
}

async fn run(app: AppState) {
  info!("Application started");
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
  let intake = app.uploads.clone();
  app.clock.every(Every::Minute, "enqueue turns", move |_| {
    intake.collect_idle_turns();
    async {}
  });
  let counted = app.clone();
  app
    .clock
    .every(Every::FiveSeconds, "transfer figures", move |_| {
      network::observe_transfers(&counted);
      async {}
    });
  let pruned = app.clone();
  app
    .clock
    .every(Every::FiveMinutes, "prune transfers", move |_| {
      let app = pruned.clone();
      async move {
        let retention = app.config.current().values.retention.transfers.clone();
        prune::prune(&app.transfers, &app.live, &retention).await;
      }
    });
  let searches = app.searches.clone();
  app
    .clock
    .every(Every::FiveMinutes, "prune searches", move |_| {
      let searches = searches.clone();
      async move { searches.prune().await }
    });
  let messaging = app.messaging.clone();
  app
    .clock
    .every(Every::Minute, "session heartbeat", move |_| {
      let messaging = messaging.clone();
      async move { messaging.heartbeat().await }
    });
  let messaging = app.messaging.clone();
  app
    .clock
    .every(Every::FiveMinutes, "prune private messages", move |_| {
      let messaging = messaging.clone();
      async move { messaging.prune_private().await }
    });
  let messaging = app.messaging.clone();
  app
    .clock
    .every(Every::Hour, "prune room messages", move |_| {
      let messaging = messaging.clone();
      async move { messaging.prune_rooms().await }
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

/// Stops the daemon's work in order: once the client is stopped and the transfer tasks have
/// ended, the transfers left unfinished are ended as shut down, so the next start resumes the
/// downloads among them.
pub(super) async fn stop(app: &AppState) {
  info!("Application is shutting down");
  app.searches.stop().await;
  app.network.stop().await;
  app.messaging.stop().await;
  app.users.stop().await;
  app.downloads.stop().await;
  app.uploads.stop().await;
  match cleanup::end_unfinished(&app.transfers).await {
    Ok(ended) => debug!(ended, "unfinished transfers ended at shutdown"),
    Err(e) => warn!(error = %e, "unfinished transfers not ended at shutdown"),
  }
}
