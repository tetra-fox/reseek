//! The daemon's life: reading the command line, loading the configuration, setting up the
//! directories, logging and the instance lock, opening the databases, serving HTTP until a
//! signal, and shutting down.

mod figures;
mod lifecycle;
mod restart;
mod service;
mod signals;
mod startup;
mod tasks;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cascade::{Loaded, Overrides, Schema, Watch, Watcher};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::activity::Activity;
use crate::auth::Auth;
use crate::cli::{self, Command, Invocation};
use crate::clock::Clock;
use crate::config::api_key::PrimaryApiKey;
use crate::config::load::{self, ConfigFiles, Sources};
use crate::config::{Options, redact};
use crate::databases::Databases;
use crate::db::Database;
use crate::events::{EventBus, EventLog};
use crate::files::Files;
use crate::http::{self, pipeline};
use crate::live::Live;
use crate::logging::{self, LogBuffer, Logging, Sinks};
use crate::messaging::Messaging;
use crate::network::Network;
use crate::paths::{Defaults, InstanceLock, Paths};
use crate::scripts::{self, Scripts};
use crate::searches::Searches;
use crate::services::Services;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::telemetry::Telemetry;
use crate::transfers::downloads::Downloads;
use crate::transfers::service::Transfers;
use crate::transfers::uploads::Uploads;
use crate::ui::{Bundle, Ui};
use crate::users::{Directory, Users};
use crate::version::{self, RELEASE_FEED, Releases};
use service::Application;
use signals::spawn_signals;
use startup::{config_error, log_startup, warn_about, write_template};

/// The command line could not be understood.
pub const EXIT_USAGE: u8 = 2;
/// The configuration is not valid.
pub const EXIT_CONFIG: u8 = 3;
/// A directory could not be made or written.
pub const EXIT_FILESYSTEM: u8 = 4;
/// Another instance holds the data directory.
pub const EXIT_LOCKED: u8 = 5;
/// Anything else that stops the start.
pub const EXIT_FATAL: u8 = 1;
pub use restart::EXIT_RESTART;

/// How often the configuration files are read when they are polled.
const POLL: Duration = Duration::from_secs(4);

/// Why the daemon's state could not be built.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
  /// an option the loader let through is not usable
  #[error("Invalid configuration: {0}")]
  Config(String),
  /// a database or an engine could not be opened
  #[error("{0}")]
  Open(String),
}

/// What every request handler shares.
#[derive(Clone)]
pub struct AppState {
  /// the configuration, kept current
  pub config: Watcher<Options>,
  /// the configuration the daemon started with
  pub startup: Arc<Loaded<Options>>,
  /// the single API key from the environment or the command line, read at startup
  pub primary_key: Option<Arc<PrimaryApiKey>>,
  /// the last log records
  pub logs: Arc<LogBuffer>,
  /// the URL base, with a leading `/` and no trailing one, empty for the root
  pub url_base: Arc<str>,
  /// accounts, sessions, tokens and the failure limits
  pub auth: Arc<Auth>,
  /// the event bus
  pub events: EventBus,
  /// the event stream's hub
  pub live: Live,
  /// what other users did with us
  pub activity: Arc<Activity>,
  /// the application state document
  pub state: StateOwner,
  /// cancelled when the daemon shuts down
  pub shutdown: CancellationToken,
  /// every place reseek reads or writes
  pub paths: Paths,
  /// `transfers.db`, the record of every download and upload
  pub transfers: Database,
  /// the metrics the engines update, and the reports
  pub telemetry: Arc<Telemetry>,
  /// every database opened, to list and to close
  pub databases: Arc<Databases>,
  /// the users directory, which the network engines touch and ask who is blocked
  pub users: Arc<Directory>,
  /// the files under the download and log directories, and which of them downloads write
  pub files: Arc<Files>,
  /// what we share, with the queries other users are answered from
  pub shares: Arc<Shares>,
  /// the Soulseek client and what keeps it connected
  pub network: Arc<Network>,
  /// the searches we start
  pub searches: Arc<Searches>,
  /// private conversations and the rooms we are in
  pub messaging: Arc<Messaging>,
  /// the download engine
  pub downloads: Arc<Downloads>,
  /// the upload engine
  pub uploads: Arc<Uploads>,
  /// the jobs that run at fixed intervals
  pub clock: Clock,
  /// the release feed the version is checked against
  pub releases: Arc<Releases>,
  /// the engine of every domain
  pub services: Services,
  /// held while our profile is changed, so one change reads the configuration file and the
  /// picture the one before it left
  pub profile_writes: Arc<tokio::sync::Mutex<()>>,
  /// set when the daemon is to start again once it has stopped
  restart: Arc<AtomicBool>,
}

impl AppState {
  /// The state for a configuration that just loaded, an opened authentication database and
  /// the paths it resolved to: the other databases are opened and every engine is built.
  /// Raised events are pushed to the event stream from here on; [`AppState::spawn_tasks`]
  /// starts the rest of the background work.
  pub async fn new(
    config: Watcher<Options>,
    logs: Arc<LogBuffer>,
    auth: Auth,
    paths: Paths,
    shutdown: CancellationToken,
  ) -> Result<Self, StartError> {
    let startup = config.current();
    let primary_key = match &startup.values.web.authentication.api_key {
      Some(text) => Some(Arc::new(PrimaryApiKey::parse(text).map_err(|e| {
        StartError::Config(format!("web.authentication.api_key: {e}"))
      })?)),
      None => None,
    };
    let url_base = pipeline::url_base(&startup.values.web.url_base).into();
    let live = Live::new(shutdown.clone());
    let activity = Activity::new(live.clone());
    let state = StateOwner::new(&startup, &paths, live.clone());
    let events = EventBus::new(shutdown.clone());
    let pushed = live.clone();
    events.subscribe("live", None, move |record| {
      pushed.publish(api_types::live::LiveFrame::EventsRaised(Box::new(
        api_types::events::EventRecord::clone(&record),
      )));
      async { Ok(()) }
    });
    Scripts::start(
      config.clone(),
      &events,
      paths.scripts.clone(),
      scripts::shell(std::env::var_os("SHELL")),
      shutdown.clone(),
    );
    let location = paths.databases(&startup.values);
    let transfers = crate::transfers::open(&location)
      .await
      .map_err(|e| StartError::Open(format!("transfers.db: {e}")))?;
    let auth = Arc::new(auth);
    let files = Files::start(
      config.clone(),
      transfers.clone(),
      &paths,
      auth.clone(),
      shutdown.clone(),
    );
    let event_log = EventLog::open(&location, &events, config.clone(), shutdown.clone())
      .await
      .map_err(|e| StartError::Open(format!("events.db: {e}")))?;
    let messaging_db = crate::messaging::open(&location)
      .await
      .map_err(|e| StartError::Open(format!("messaging.db: {e}")))?;
    let users = Directory::open(
      &location,
      transfers.clone(),
      messaging_db.clone(),
      config.clone(),
      live.clone(),
      events.clone(),
      shutdown.clone(),
    )
    .await
    .map_err(StartError::Open)?;
    let event_log_db = event_log.database();
    let telemetry = Telemetry::new(transfers.clone(), shutdown.clone());
    let shares = Shares::open(config.clone(), &paths, state.clone(), shutdown.clone())
      .await
      .map_err(|e| StartError::Open(format!("shares.local.db: {e}")))?;
    let network = Network::new(
      &config,
      users.clone(),
      activity.clone(),
      shares.clone(),
      telemetry.clone(),
      state.clone(),
      shutdown.clone(),
    )
    .map_err(StartError::Config)?;
    users.attach_client(network.client().clone());
    let messaging = Messaging::open(
      messaging_db.clone(),
      network.clone(),
      users.clone(),
      live.clone(),
      events.clone(),
      config.clone(),
      state.clone(),
      shutdown.clone(),
    )
    .await
    .map_err(|e| StartError::Open(format!("messaging.db: {e}")))?;
    let searches = Searches::open(
      &location,
      network.clone(),
      users.clone(),
      live.clone(),
      config.clone(),
    )
    .await
    .map_err(|e| StartError::Open(format!("search.db: {e}")))?;
    let releases = Arc::new(
      Releases::new(RELEASE_FEED)
        .map_err(|e| StartError::Open(format!("release feed HTTP client: {e}")))?,
    );
    let downloads = Downloads::new(
      transfers.clone(),
      config.clone(),
      network.clone(),
      users.clone(),
      files.clone(),
      searches.clone(),
      events.clone(),
      live.clone(),
      telemetry.clone(),
      &paths,
      shutdown.clone(),
    );
    let uploads = Uploads::new(
      transfers.clone(),
      config.clone(),
      &network,
      users.clone(),
      shares.clone(),
      events.clone(),
      live.clone(),
      telemetry.clone(),
      shutdown.clone(),
    );
    network.attach_uploads(uploads.clone());
    let clock = Clock::new(shutdown.clone());
    let mut services = Services::unavailable();
    services.transfers = Arc::new(Transfers {
      downloads: downloads.clone(),
      uploads: uploads.clone(),
    });
    services.shares = shares.clone();
    services.events = event_log;
    services.files = files.clone();
    services.reports = telemetry.clone();
    services.searches = searches.clone();
    services.messaging = messaging.clone();
    services.users = Arc::new(Users::new(
      users.clone(),
      network.clone(),
      messaging.clone(),
      config.clone(),
      live.clone(),
      crate::api::agent::is_agent(&startup.values),
    ));
    services.blocking = users.clone();
    let databases = Arc::new(Databases::new(vec![
      auth.db.clone(),
      event_log_db,
      transfers.clone(),
      users.database().clone(),
      shares.database(),
      searches.database().clone(),
      messaging_db,
    ]));
    services.application = Arc::new(Application {
      network: network.clone(),
      releases: releases.clone(),
      state: state.clone(),
      databases: databases.clone(),
      config: config.clone(),
    });
    Ok(Self {
      config,
      startup,
      primary_key,
      logs,
      url_base,
      auth,
      events,
      live,
      activity,
      state,
      shutdown,
      paths,
      transfers,
      telemetry,
      databases,
      users,
      files,
      shares,
      network,
      searches,
      messaging,
      downloads,
      uploads,
      clock,
      releases,
      services,
      profile_writes: Arc::new(tokio::sync::Mutex::new(())),
      restart: Arc::new(AtomicBool::new(false)),
    })
  }

  /// Stops the daemon so that it starts again.
  pub fn request_restart(&self) {
    self.restart.store(true, Ordering::Relaxed);
    self.shutdown.cancel();
  }

  /// Whether the daemon is to start again once it has stopped.
  pub fn restart_requested(&self) -> bool {
    self.restart.load(Ordering::Relaxed)
  }

  /// Starts the background work every request relies on: the state and the stream follow the
  /// configuration and the log, and ended sessions are pruned.
  pub fn spawn_tasks(&self) {
    tasks::spawn(self);
  }

  /// Ends the transfers and searches the last run left unfinished, then starts the daemon's
  /// own work in the background: following the client and the options, the startup tasks, the
  /// clock, and connecting to the Soulseek server.
  pub async fn start(&self) {
    lifecycle::start(self).await;
  }

  /// Stops the daemon's own work once the HTTP host has stopped: the client disconnects and
  /// what that raised is handled.
  pub async fn stop(&self) {
    lifecycle::stop(self).await;
  }
}

/// Runs the program with the UI `bundle` carries and returns its exit code.
pub fn main(bundle: Bundle) -> ExitCode {
  let schema = Schema::of::<Options>();
  let command = match cli::parse(
    std::env::args_os().skip(1),
    |n| std::env::var_os(n),
    &schema,
  ) {
    Ok(command) => command,
    Err(e) => {
      eprintln!("{e}");
      return ExitCode::from(EXIT_USAGE);
    }
  };
  match command {
    Command::Version => println!("{}", version::info().full),
    Command::Help {
      flags,
      envars,
      no_logo,
    } => {
      if !no_logo {
        print!("{}", cli::banner());
      }
      if flags {
        print!("{}", cli::help(&schema));
      }
      if envars {
        print!("{}", cli::envars(&schema));
      }
    }
    Command::GenerateCert { path } => match cli::generate_cert(path.as_deref()) {
      Ok(text) => println!("{text}"),
      Err(e) => {
        eprintln!("{e}");
        return ExitCode::from(EXIT_FATAL);
      }
    },
    Command::GenerateSecret { length } => println!("{}", cli::generate_secret(length)),
    Command::HashPassword => match cli::hash_password() {
      Ok(hash) => println!("{hash}"),
      Err(e) => {
        eprintln!("{e}");
        return ExitCode::from(EXIT_FATAL);
      }
    },
    Command::Run(invocation) => {
      let supervised = invocation
        .supervised
        .unwrap_or_else(|| restart::supervised_by_default(|n| std::env::var_os(n)));
      let code = run(invocation, bundle);
      if code == EXIT_RESTART && !supervised {
        return ExitCode::from(restart::start_again());
      }
      return ExitCode::from(code);
    }
  }
  ExitCode::SUCCESS
}

/// Starts the daemon and serves until it is told to stop.
fn run(invocation: Invocation, bundle: Bundle) -> u8 {
  let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
  let defaults = Defaults::new(invocation.app_dir.as_deref());
  let mut sources = Sources {
    env,
    files: ConfigFiles::new(&[], &PathBuf::new()),
    cli: invocation.flags,
  };
  let config_dir = match defaults.config_dir(sources.config_dir()) {
    Ok(dir) => dir,
    Err(e) => {
      eprintln!("{e}");
      return EXIT_FILESYSTEM;
    }
  };
  sources.files = ConfigFiles::new(&invocation.config_files, &config_dir);

  // a first load, before the data directory that holds the overrides is known
  let options = match load::layers(&sources, &defaults, None).load() {
    Ok(loaded) => {
      if matches!(
        loaded.sources.get("paths.config"),
        Some(cascade::Source::File { .. })
      ) {
        eprintln!(
          "Invalid configuration: paths.config is set by RESEEK_CONFIG_DIR or --config-dir \
           only, since the configuration files are in it"
        );
        return EXIT_CONFIG;
      }
      loaded.values
    }
    Err(e) => {
      eprintln!("{}", config_error(&e));
      return EXIT_CONFIG;
    }
  };
  let paths = match defaults
    .resolve(&options)
    .and_then(|p| p.create().map(|()| p))
  {
    Ok(paths) => paths,
    Err(e) => {
      eprintln!("{e}");
      return EXIT_FILESYSTEM;
    }
  };
  let no_color_env = sources
    .env
    .iter()
    .any(|(name, value)| name == "NO_COLOR" && !value.is_empty());
  let logging = match logging::init(
    &options,
    Sinks {
      files: (!options.logger.no_disk).then(|| paths.logs.clone()),
      no_color_env,
      loki: options.logger.loki.clone(),
    },
  ) {
    Ok(logging) => logging,
    Err(e) => {
      eprintln!("logging could not be set up: {e}");
      return EXIT_FATAL;
    }
  };
  if !options.flags.no_logo {
    print!("{}", cli::banner());
  }
  let invocation_id = uuid::Uuid::new_v4();
  // every event of the process is inside it, so its fields reach any sink that reads spans
  let process = info_span!(
    logging::PROCESS_SPAN,
    instance_name = options.instance_name,
    invocation_id = %invocation_id,
    pid = std::process::id()
  );
  let _process = process.enter();
  log_startup(
    &options,
    &paths,
    invocation.app_dir.as_deref(),
    &sources.files,
    invocation_id,
  );
  let _lock = match InstanceLock::acquire(&paths.lock()) {
    Ok(lock) => lock,
    Err(e) => {
      error!(error = %e, "instance lock not taken");
      return EXIT_LOCKED;
    }
  };
  let runtime = match tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
  {
    Ok(runtime) => runtime,
    Err(e) => {
      error!(error = %e, "the async runtime could not start");
      return EXIT_FATAL;
    }
  };
  runtime.block_on(serve(sources, defaults, paths, logging, bundle).instrument(process.clone()))
}

/// Forwards the logs to Grafana Loki when it is configured, and serves until a signal; the last
/// lines are pushed to Loki before it returns.
async fn serve(
  sources: Sources,
  defaults: Defaults,
  paths: Paths,
  logging: Logging,
  bundle: Bundle,
) -> u8 {
  let loki = logging.start_loki();
  let code = host(sources, defaults, paths, logging, bundle).await;
  if let Some(loki) = loki {
    loki.finish().await;
  }
  code
}

/// Loads the configuration for good, binds the listeners and serves until a signal.
async fn host(
  sources: Sources,
  defaults: Defaults,
  paths: Paths,
  logging: Logging,
  bundle: Bundle,
) -> u8 {
  let overrides = match Overrides::load(paths.overrides()) {
    Ok(overrides) => overrides,
    Err(e) => {
      error!(error = %e, "runtime overrides not loaded");
      return EXIT_CONFIG;
    }
  };
  // before the files are read, so the configuration knows the template's revision and the
  // first save to it is not taken for a change made on disk
  write_template(&sources.files, sources.managed_by().as_deref());
  let watcher = match Watcher::new(load::layers(&sources, &defaults, Some(overrides))) {
    Ok(watcher) => watcher,
    Err(e) => {
      error!(error = %config_error(&e), "Invalid configuration");
      return EXIT_CONFIG;
    }
  };
  let current = watcher.current();
  let options = &current.values;
  logging.apply(options);
  for name in load::unknown_variables(&Schema::of::<Options>(), &sources.env) {
    warn!(
      variable = name,
      "environment variable is no option of reseek (ignored)"
    );
  }
  warn_about(options, &logging);
  debug!(options = %redact::values(&current), "effective options");

  let cancel = CancellationToken::new();
  let watch = if options.flags.no_config_watch {
    warn!(
      "Configuration watch DISABLED; all configuration changes will require a restart to take effect"
    );
    Watch::Off
  } else if options.flags.config_watch_poll {
    Watch::Poll(POLL)
  } else {
    Watch::Events
  };
  watcher.start(watch, cancel.clone());
  let logging = Arc::new(logging);
  tokio::spawn(
    tasks::follow_changes(watcher.clone(), logging.clone(), cancel.clone()).in_current_span(),
  );
  spawn_signals(watcher.clone(), cancel.clone());

  let grace = options.shutdown_timeout;
  let location = paths.databases(options);
  let auth = match Auth::open(&location).await {
    Ok(auth) => auth,
    Err(e) => {
      error!(error = %e, "the authentication database could not be opened");
      return EXIT_FATAL;
    }
  };
  let ui = Ui::new(bundle, options, &load::program_dir());
  let listeners = match http::bind(&options.web).await {
    Ok(listeners) => listeners,
    Err(e) => {
      error!(error = e, "HTTP host not started");
      return EXIT_FATAL;
    }
  };
  if options.flags.no_start {
    info!("Quitting because 'no-start' option is enabled");
    cancel.cancel();
    return 0;
  }
  let state = match AppState::new(
    watcher,
    logging.buffer().clone(),
    auth,
    paths,
    cancel.clone(),
  )
  .await
  {
    Ok(state) => state,
    Err(e @ StartError::Config(_)) => {
      error!(error = %e, "reseek could not start");
      return EXIT_CONFIG;
    }
    Err(e @ StartError::Open(_)) => {
      error!(error = %e, "reseek could not start");
      return EXIT_FATAL;
    }
  };
  let urls = listeners.urls();
  state.state.update(|current| current.listeners = urls);
  state.spawn_tasks();
  state.start().await;
  info!(base = %state.url_base, "Using base url");
  info!("reseek started");
  http::serve(listeners, pipeline::app(state.clone(), ui), cancel, grace).await;
  state.stop().await;
  state.databases.close().await;
  if state.restart_requested() {
    info!("reseek stopped to start again");
    return EXIT_RESTART;
  }
  info!("reseek stopped");
  0
}
