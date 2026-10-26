//! The daemon's life: reading the command line, loading the configuration, setting up the
//! directories, logging and the instance lock, opening the databases, serving HTTP until a
//! signal, and shutting down.

mod restart;
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

use crate::auth::Auth;
use crate::cli::{self, Command, Invocation};
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
use crate::paths::{Defaults, InstanceLock, Paths};
use crate::services::Services;
use crate::shares::Shares;
use crate::state::StateOwner;
use crate::telemetry::Telemetry;
use crate::ui::{Bundle, Ui};
use crate::users::Directory;
use crate::version;
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
  /// the engine of every domain
  pub services: Services,
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
    let state = StateOwner::new(&startup, live.clone());
    let events = EventBus::new(shutdown.clone());
    let pushed = live.clone();
    events.subscribe("live", None, move |record| {
      pushed.publish(api_types::live::LiveFrame::EventsRaised(Box::new(
        api_types::events::EventRecord::clone(&record),
      )));
      async { Ok(()) }
    });
    let location = paths.databases(&startup.values);
    let transfers = crate::transfers::open(&location)
      .await
      .map_err(|e| StartError::Open(format!("transfers.db: {e}")))?;
    let auth = Arc::new(auth);
    let files = Files::start(config.clone(), &paths, auth.clone(), shutdown.clone());
    let event_log = EventLog::open(&location, &events, config.clone(), shutdown.clone())
      .await
      .map_err(|e| StartError::Open(format!("events.db: {e}")))?;
    let users = Directory::open(
      &location,
      transfers.clone(),
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
    let mut services = Services::unavailable();
    services.shares = shares.clone();
    services.events = event_log;
    services.files = files.clone();
    services.reports = telemetry.clone();
    services.users = users.clone();
    services.blocking = users.clone();
    let databases = Arc::new(Databases::new(vec![
      auth.db.clone(),
      event_log_db,
      transfers.clone(),
      users.database().clone(),
      shares.database(),
    ]));
    services.application = databases.clone();
    Ok(Self {
      config,
      startup,
      primary_key,
      logs,
      url_base,
      auth,
      events,
      live,
      state,
      shutdown,
      paths,
      transfers,
      telemetry,
      databases,
      users,
      files,
      shares,
      services,
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

/// Loads the configuration for good, binds the listeners and serves until a signal.
async fn serve(
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
  write_template(&sources.files, current.managed_by.as_deref());
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
  tokio::spawn(follow_changes(watcher.clone(), logging.clone(), cancel.clone()).in_current_span());
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
  state.spawn_tasks();
  info!(base = %state.url_base, "Using base url");
  info!("reseek started");
  http::serve(listeners, pipeline::app(state.clone(), ui), cancel, grace).await;
  state.databases.close().await;
  if state.restart_requested() {
    info!("reseek stopped to start again");
    return EXIT_RESTART;
  }
  info!("reseek stopped");
  0
}

/// Applies the live parts of each configuration change that are not read per request.
async fn follow_changes(
  watcher: Watcher<Options>,
  logging: Arc<Logging>,
  cancel: CancellationToken,
) {
  let mut changes = watcher.changes();
  loop {
    let change = tokio::select! {
      () = cancel.cancelled() => return,
      change = changes.recv() => change,
    };
    match change {
      Ok(change) => {
        if change.changed.iter().any(|k| k.starts_with("logging.")) {
          logging.apply(&change.current.values);
        }
        let limit = change.current.values.web.files.serve_speed_limit;
        if limit != change.previous.values.web.files.serve_speed_limit {
          startup::warn_about_serve_speed_limit(limit);
        }
        if !change.by_apply.reconnect.is_empty() {
          info!(
            keys = change.by_apply.reconnect.join(", "),
            "One or more updated Soulseek options requires the client to be disconnected, then reconnected to the network to take effect."
          );
        }
        if !change.by_apply.restart.is_empty() {
          info!(
            keys = change.by_apply.restart.join(", "),
            "One or more updated options requires an application restart to take effect."
          );
        }
      }
      Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
        warn!(
          missed,
          "configuration changes missed; applying the current values"
        );
        logging.apply(&watcher.current().values);
      }
      Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
    }
  }
}
