//! The daemon's side of the Soulseek network: the client built from the options, the watchdog
//! that keeps it connected, the router forwarding our peer port and whether other users can
//! connect to us, what the daemon does with each client event, how it answers other users, and
//! the options it follows while running.
//!
//! [`Network`] owns the client. The engines reach the network through [`Network::client`]; the
//! server connection is started and ended through [`Network::connect`],
//! [`Network::disconnect`] and [`Network::stop`].

mod changes;
mod events;
mod mapping;
pub mod options;
mod reachability;
pub mod resolvers;
pub mod watchdog;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api_types::state::UserStatistics;
use cascade::Watcher;
use chrono::Utc;
use soulseek_client::{Client, ClientError, ClientState, Event};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::activity::Activity;
use crate::config::Options;
use crate::services::Connected;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::sync::lock;
use crate::telemetry::Telemetry;
use crate::users::Directory;
pub use changes::follow_changes;
pub use events::{follow_shared_counts, observe_client, observe_transfers};
use mapping::{NatPmp, PortMapper, Upnp};
use reachability::Reachability;
use resolvers::{Edges, IncomingSearches, resolvers};
use watchdog::Watchdog;

/// How long our own statistics and privileges are kept before a search response fetches them
/// again.
const STATISTICS_FOR: Duration = Duration::from_secs(4 * 3600);

/// The message the client is disconnected with at shutdown.
const SHUTTING_DOWN: &str = "Shutting down";

/// Why the daemon itself ended the server session, carried as the disconnect's error so that
/// what follows the disconnect knows not to reconnect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Intent {
  /// someone asked to disconnect
  #[error("disconnected by the user")]
  User,
  /// the daemon is stopping
  #[error("the application is shutting down")]
  Shutdown,
}

impl Intent {
  /// The error a disconnect for this reason carries.
  fn error(self) -> ClientError {
    ClientError::Client {
      message: self.to_string(),
      source: Some(Arc::new(self)),
    }
  }

  /// The reason a disconnect's `error` carries, when the daemon asked for it.
  pub fn of(error: &ClientError) -> Option<Self> {
    match error {
      ClientError::Client {
        source: Some(source),
        ..
      } => source.downcast_ref::<Self>().copied(),
      _ => None,
    }
  }
}

/// The username and password to log in with, when both are set and not empty.
pub fn credentials(soulseek: &crate::config::Soulseek) -> Option<(&str, &str)> {
  let username = soulseek.username.as_deref().filter(|u| !u.is_empty())?;
  let password = soulseek.password.as_deref().filter(|p| !p.is_empty())?;
  Some((username, password))
}

/// Says that the daemon does not connect for want of credentials.
pub fn warn_no_credentials() {
  warn!(
    "Not connecting to the Soulseek server; username and/or password invalid.  Specify valid \
     credentials and manually connect, or update config and restart."
  );
}

/// The client and what keeps it connected.
pub struct Network {
  client: Client,
  watchdog: Watchdog,
  state: StateOwner,
  /// the phrases the server asks our search responses to leave out, as it last sent them
  excluded: Mutex<Arc<[String]>>,
  /// when our own statistics were last fetched; held while they are fetched
  statistics: tokio::sync::Mutex<Option<Instant>>,
  /// the client's events, until the task that follows them takes them
  events: Mutex<Option<broadcast::Receiver<Event>>>,
  /// the task following the client's events
  following: Mutex<Option<JoinHandle<()>>>,
  /// tells that task to end once it has handled every event raised so far
  drained: CancellationToken,
  /// what other users are answered from
  edges: Arc<Edges>,
  /// the router forwarding our peer port
  mapper: PortMapper,
  /// whether other users can connect to us
  reachability: Arc<Reachability>,
}

impl Network {
  /// A disconnected client with the options `config` gives, answering other users through
  /// `users` and `shares` and counting the answers in `telemetry`, and a watchdog that is off.
  /// Fails only for options that did not pass validation.
  pub fn new(
    config: &Watcher<Options>,
    users: Arc<Directory>,
    activity: Arc<Activity>,
    shares: Arc<Shares>,
    telemetry: Arc<Telemetry>,
    state: StateOwner,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, String> {
    let startup = config.current();
    let mut client_options = options::client_options(&startup.values)?;
    client_options.validate().map_err(|e| e.to_string())?;
    let searches = IncomingSearches::new(&startup.values, shutdown.clone());
    Ok(Arc::new_cyclic(|this| {
      let edges = Arc::new(Edges::new(
        users,
        activity,
        shares.clone(),
        config.clone(),
        telemetry,
        this.clone(),
        searches,
      ));
      client_options.resolvers = resolvers(edges.clone());
      // the options were validated above, which is all a new client can fail on
      let listen = (client_options.listen_ip_address, client_options.listen_port);
      let client = Client::new(client_options).expect("the client options were validated");
      let events = client.subscribe();
      let reachability = Arc::new(Reachability::new(
        state.clone(),
        client.inbound_connections(),
      ));
      let mapper = PortMapper::new(
        vec![Box::new(Upnp::default()), Box::new(NatPmp::default())],
        startup.values.soulseek.port_mapping,
        listen.0,
        listen.1,
      );
      let watchdog = Watchdog::new(
        client.clone(),
        config.clone(),
        state.clone(),
        shares,
        shutdown,
      );
      Self {
        client,
        watchdog,
        state,
        excluded: Mutex::new(Arc::from(Vec::new())),
        statistics: tokio::sync::Mutex::new(None),
        events: Mutex::new(Some(events)),
        following: Mutex::new(None),
        drained: CancellationToken::new(),
        edges,
        mapper,
        reachability,
      }
    }))
  }

  /// Has the upload engine answer peers' upload requests and their questions about the
  /// upload queue; done once, while the daemon is built.
  pub fn attach_uploads(&self, uploads: Arc<crate::transfers::uploads::Uploads>) {
    self.edges.attach(uploads);
  }

  /// The Soulseek client, for the operations the engines start.
  pub fn client(&self) -> &Client {
    &self.client
  }

  /// The loop that keeps the server connection up.
  pub fn watchdog(&self) -> &Watchdog {
    &self.watchdog
  }

  /// The phrases the server asks our search responses to leave out.
  pub fn excluded_phrases(&self) -> Arc<[String]> {
    lock(&self.excluded).clone()
  }

  /// How many of other users' searches wait for a worker now.
  pub fn searches_waiting(&self) -> usize {
    self.edges.searches.waiting()
  }

  /// Starts following the client's events for `app`; called once, before anything connects.
  pub fn follow_events(&self, app: &crate::app::AppState) {
    let Some(events) = lock(&self.events).take() else {
      return warn!("the client's events are followed already");
    };
    let task = tokio::spawn(
      events::follow(app.clone(), events, self.drained.clone())
        .instrument(info_span!("client_events")),
    );
    *lock(&self.following) = Some(task);
  }

  /// Starts asking the router to forward our peer port and judging whether other users can
  /// connect to us, until `shutdown`; called once, before anything connects.
  pub fn follow_listening(&self, shutdown: CancellationToken) {
    self
      .mapper
      .start(self.state.clone(), self.reachability.clone());
    tokio::spawn(
      self
        .reachability
        .clone()
        .follow(shutdown)
        .instrument(info_span!("reachability")),
    );
  }

  /// The listener's options may have changed: the client's own, which a listener that could
  /// not move keeps as they were, are mapped on the router when `mapping` is on, and a new
  /// port starts the judgment of who can reach us over.
  fn listening_changed(&self, mapping: bool) {
    let options = self.client.options();
    let (ip, port) = (options.listen_ip_address, options.listen_port);
    self.mapper.options(mapping, ip, port);
    if self.state.current().server.listen.port != port {
      self.state.update(|state| state.server.listen.port = port);
      self.reachability.start_over("the listening port changed");
    }
  }

  /// Connects and keeps reconnecting: starts the watchdog, or, when it was trying already,
  /// starts it over at once instead of waiting out its back-off.
  pub fn connect(&self) -> Connected {
    if self.watchdog.restart() {
      Connected::Restarted
    } else {
      Connected::Started
    }
  }

  /// Disconnects with `reason` and stops reconnecting until asked to connect again.
  pub async fn disconnect(&self, reason: Option<String>) {
    self.watchdog.stop(true);
    if self.client.state() != ClientState::DISCONNECTED {
      let message = reason.unwrap_or_else(|| Intent::User.to_string());
      self
        .client
        .disconnect(Some(&message), Some(Intent::User.error()))
        .await;
    }
  }

  /// Stops for good at shutdown: the watchdog is canceled, the port mapping removed, the
  /// client disconnected and disposed, and the events this raised are handled before this
  /// returns.
  pub async fn stop(&self) {
    self.watchdog.stop(true);
    self.mapper.stop().await;
    self
      .client
      .disconnect(Some(SHUTTING_DOWN), Some(Intent::Shutdown.error()))
      .await;
    self.client.dispose().await;
    self.drained.cancel();
    let following = lock(&self.following).take();
    if let Some(task) = following
      && let Err(e) = task.await
    {
      warn!(error = %e, "the client's events were not all handled");
    }
    info!("Client stopped");
  }

  /// Fetches our statistics and privileges from the server into `State.user`: always with
  /// `force`, else only when they were last fetched 4 hours ago or more. Nothing while not
  /// logged in.
  pub async fn refresh_user_statistics(&self, force: bool) -> Result<(), ClientError> {
    let mut fetched = self.statistics.lock().await;
    if !force && fetched.is_some_and(|at| at.elapsed() < STATISTICS_FOR) {
      return Ok(());
    }
    let Some(username) = self.client.username() else {
      return Ok(());
    };
    let statistics = self.client.get_user_statistics(&username).await?;
    let privileges = self.client.get_privileges().await?;
    let stats = statistics.stats;
    // the server counts the seconds in 32 bits
    let seconds = u32::try_from(privileges.as_secs()).unwrap_or(u32::MAX);
    self.state.update(|state| {
      let user = &mut state.user;
      user.statistics = UserStatistics {
        average_speed: stats.average_speed,
        directory_count: stats.directory_count,
        file_count: stats.file_count,
        upload_count: stats.upload_count.into(),
      };
      user.privileges.is_privileged = seconds > 0;
      user.privileges.privileges_remaining = seconds;
      user.privileges.privileged_until =
        (seconds > 0).then(|| Utc::now() + chrono::Duration::seconds(seconds.into()));
    });
    *fetched = Some(Instant::now());
    debug!(username, "own statistics refreshed");
    Ok(())
  }

  /// Takes the phrases the server asks our search responses to leave out.
  fn exclude(&self, phrases: Vec<String>) {
    *lock(&self.excluded) = phrases.into();
  }

  /// Clears `State.pendingReconnect`: the session an option change waited for has ended, and
  /// the next one starts with the options in effect.
  fn forget_reconnect(&self) {
    self.state.update(|state| state.pending_reconnect = false);
  }
}
