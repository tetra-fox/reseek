//! The order the daemon's work starts in once it is built, and the order it stops in.
//!
//! Starting: the transfers the last run left unfinished are ended and the downloads among them
//! marked to resume after the login, and the searches it left running are ended, before
//! anything can start new ones, and the process's health is sampled into the state; then, in
//! the background, the client's events and the options are followed, whether other users can
//! reach us is judged and the router is asked to forward our peer port while we are logged in,
//! the shares are initialized, and the upkeep and the schedules start. Then the watchdog
//! connects, unless connecting is turned off or there are no credentials.
//!
//! Stopping, once the HTTP host has stopped: the running searches end as canceled, the
//! watchdog stops, the port mapping is removed, the client disconnects and is disposed, the
//! events this raised are handled, the session is closed, the statistics fetches and the transfer
//! tasks end, the transfers still unfinished are ended as shut down, the scripts' tasks
//! end with the runs the shutdown killed recorded, and last the event bus delivers what was
//! raised and stops.

use tracing::{Instrument, debug, info, info_span, warn};

use std::time::Duration;

use super::{AppState, figures};
use crate::jobs::schedules::{self, upkeep};
use crate::network;
use crate::transfers::cleanup;

/// How often the health of incoming searches and the process's memory are copied into the state
const HEALTH_EVERY: Duration = Duration::from_secs(30);

/// Ends what the last run left unfinished, then starts the daemon's work in order, in the
/// background.
pub(super) async fn start(app: &AppState) {
  // a transfer or a search started from here on is not one the last run left
  if let Err(e) = cleanup::end_unfinished(&app.transfers).await {
    warn!(error = %e, "transfers left unfinished at the last stop not ended");
  }
  app.searches.end_dangling().await;
  // the state is served complete from the first request, without waiting for the upkeep
  figures::sample_health(app).await;
  app.network.follow_events(app);
  app.network.follow_listening(app.shutdown.clone());
  tokio::spawn(network::follow_changes(app.clone()).instrument(info_span!("client_options")));
  tokio::spawn(network::follow_shared_counts(app.clone()).instrument(info_span!("shared_counts")));
  tokio::spawn(run(app.clone()).instrument(info_span!("startup")));
}

async fn run(app: AppState) {
  let app = &app;
  info!("Application started");
  let options = app.config.current().values.clone();
  // the flags take a restart, so they are read as the daemon started with
  let flags = &app.startup.values.flags;
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
  app.shares.initialized().await;

  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(60),
    "client figures",
    {
      let app = app.clone();
      move || {
        network::observe_client(&app);
        async {}
      }
    },
  );
  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(60),
    "enqueue turns",
    {
      let intake = app.uploads.clone();
      move || {
        intake.collect_idle_turns();
        async {}
      }
    },
  );
  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(5),
    "transfer figures",
    {
      let app = app.clone();
      move || {
        let app = app.clone();
        async move { network::observe_transfers(&app).await }
      }
    },
  );
  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(1),
    "transfer figures pushed",
    {
      let app = app.clone();
      move || {
        figures::push_transfers(&app);
        async {}
      }
    },
  );
  // the first sample was taken as the daemon started
  upkeep(app, HEALTH_EVERY, HEALTH_EVERY, "health sample", {
    let app = app.clone();
    move || {
      let app = app.clone();
      async move { figures::sample_health(&app).await }
    }
  });
  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(60),
    "session heartbeat",
    {
      let messaging = app.messaging.clone();
      move || {
        let messaging = messaging.clone();
        async move { messaging.heartbeat().await }
      }
    },
  );
  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(60),
    "expired blocks",
    {
      let users = app.users.clone();
      move || {
        let users = users.clone();
        async move { users.expire_blocks().await }
      }
    },
  );
  upkeep(
    app,
    Duration::ZERO,
    Duration::from_secs(3600),
    "lapsed sessions",
    {
      let app = app.clone();
      move || {
        let app = app.clone();
        async move { super::tasks::end_lapsed_sessions(&app).await }
      }
    },
  );
  tokio::spawn(schedules::run(app.clone()).instrument(info_span!("schedules")));

  if options.relay.enabled {
    info!("Running in Controller relay mode.  Listening for incoming Agent connections.");
  }
  if flags.no_connect {
    warn!("Not connecting to the Soulseek server; 'no-connect' option is enabled");
  } else if network::credentials(soulseek).is_none() {
    network::warn_no_credentials();
  } else {
    app.network.watchdog().start();
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
  // the runs the shutdown killed are recorded, then the bus delivers what was raised and stops
  app.scripts.stop().await;
  app.events.stop().await;
}
