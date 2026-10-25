//! The daemon's life: reading the command line, loading the configuration, setting up the
//! directories, logging and the instance lock, opening the databases, serving HTTP until a
//! signal, and shutting down.

mod tasks;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use cascade::{Loaded, Overrides, Schema, Watch, Watcher};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::auth::Auth;
use crate::cli::{self, Command, Invocation};
use crate::config::api_key::PrimaryApiKey;
use crate::config::load::{self, ConfigFiles, Sources};
use crate::config::{Options, redact, template};
use crate::db::Location;
use crate::events::EventBus;
use crate::http::{self, pipeline};
use crate::live::Live;
use crate::logging::{self, LogBuffer, Logging, Sinks};
use crate::paths::{Defaults, InstanceLock, Paths};
use crate::state::StateOwner;
use crate::ui::{Bundle, Ui};
use crate::version;

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

/// How often the configuration files are read when they are polled.
const POLL: Duration = Duration::from_secs(4);

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
}

impl AppState {
  /// The state for a configuration that just loaded and an opened authentication database.
  /// Raised events are pushed to the event stream from here on; [`AppState::spawn_tasks`]
  /// starts the rest of the background work.
  pub fn new(
    config: Watcher<Options>,
    logs: Arc<LogBuffer>,
    auth: Auth,
    shutdown: CancellationToken,
  ) -> Result<Self, String> {
    let startup = config.current();
    let primary_key = match &startup.values.web.authentication.api_key {
      Some(text) => Some(Arc::new(
        PrimaryApiKey::parse(text).map_err(|e| format!("web.authentication.api_key: {e}"))?,
      )),
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
    Ok(Self {
      config,
      startup,
      primary_key,
      logs,
      url_base,
      auth: Arc::new(auth),
      events,
      live,
      state,
      shutdown,
    })
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
    Command::Run(invocation) => return ExitCode::from(run(invocation, bundle)),
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
  let location = Location {
    data: paths.data.clone(),
    backups: paths.backups.clone(),
    volatile: options.flags.volatile,
    log_sql: options.flags.log_sql,
  };
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
  let state = match AppState::new(watcher, logging.buffer().clone(), auth, cancel.clone()) {
    Ok(state) => state,
    Err(e) => {
      error!(error = e, "Invalid configuration");
      return EXIT_CONFIG;
    }
  };
  state.spawn_tasks();
  info!(base = %state.url_base, "Using base url");
  info!("reseek started");
  http::serve(listeners, pipeline::app(state.clone(), ui), cancel, grace).await;
  state.auth.db.close().await;
  info!("reseek stopped");
  0
}

/// The lines every start logs: the version, the system, this process, and where things are.
fn log_startup(
  options: &Options,
  paths: &Paths,
  app_dir: Option<&std::path::Path>,
  files: &ConfigFiles,
  invocation_id: uuid::Uuid,
) {
  let info = version::info();
  info!(version = info.full, "Version");
  if info.is_development {
    warn!("This is a Development build; YMMV");
  }
  let processors = std::thread::available_parallelism().map_or(1, |n| n.get());
  info!(
    os = std::env::consts::OS,
    arch = std::env::consts::ARCH,
    processors,
    "System"
  );
  info!(pid = std::process::id(), "Process ID");
  info!(id = %invocation_id, "Invocation ID");
  info!(name = options.instance_name, "Instance Name");
  if let Some(dir) = app_dir {
    info!(path = %dir.display(), "Using application directory");
  }
  info!(path = %files.first().display(), "Using configuration file");
  info!(path = %paths.data.display(), "Storing application data");
  info!(path = %paths.cache.display(), "Storing rebuildable data");
  if !options.logger.no_disk {
    info!(path = %paths.logs.display(), "Saving application logs");
  }
  info!(path = %paths.downloads.display(), "Saving downloads");
  info!(path = %paths.incomplete.display(), "Saving downloads in progress");
}

/// The warnings the options call for at startup.
fn warn_about(options: &Options, logging: &Logging) {
  if logging.from_env() {
    info!("RUST_LOG sets the log filter; logging.level and logging.targets are not applied");
  }
  if options.web.authentication.disabled {
    warn!("Authentication of web requests is DISABLED");
  }
  if options.web.https.force && !options.web.https.disabled {
    info!(
      port = options.web.https.port,
      "Forcing HTTP requests to HTTPS"
    );
  }
  if options.flags.log_sql {
    warn!("SQL logging is ENABLED; log files may contain sensitive data");
  }
  if options.flags.force_migrations {
    warn!("force_migrations has no effect: migrations apply by version and none runs twice");
  }
  let authentication = &options.web.authentication;
  if authentication.local.enabled
    && authentication.password_hash.is_none()
    && authentication.password == crate::config::Authentication::default().password
  {
    warn!(
      username = authentication.username,
      "the built-in account still has the default password; set web.authentication.password_hash"
    );
  }
  for (name, user) in &authentication.local.users {
    if user.password_hash.is_none() && user.password.is_some() {
      warn!(
        username = name,
        "local account has its password in plain text; set password_hash from `reseek hash-password`"
      );
    }
  }
  if options.logger.loki.is_some() {
    warn!("forwarding logs to Grafana Loki is not available in this build; logger.loki ignored");
  }
}

/// Writes the commented-out template to the first configuration file when it does not exist.
fn write_template(files: &ConfigFiles, managed_by: Option<&str>) {
  let path = files.first();
  if path.exists() || managed_by.is_some() {
    return;
  }
  warn!(path = %path.display(), "Configuration file does not exist; creating from example");
  let text = template::render(&Schema::of::<Options>());
  if let Err(e) = std::fs::write(&path, text) {
    error!(path = %path.display(), error = %e, "Failed to create configuration file");
  }
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

/// Shuts down on the first SIGINT or SIGTERM and exits at once on a second; reloads the
/// configuration on SIGHUP.
fn spawn_signals(watcher: Watcher<Options>, cancel: CancellationToken) {
  #[cfg(unix)]
  {
    use tokio::signal::unix::{SignalKind, signal};
    let reload = watcher;
    tokio::spawn(
      async move {
        let Ok(mut hangup) = signal(SignalKind::hangup()) else {
          warn!("SIGHUP not handled; reload the configuration through the API instead");
          return;
        };
        while hangup.recv().await.is_some() {
          info!("configuration reload asked for by SIGHUP");
          // a failed reload is logged and kept in the configuration's state where it happens
          drop(reload.reload().await);
        }
      }
      .in_current_span(),
    );
    tokio::spawn(
      async move {
        let (Ok(mut interrupt), Ok(mut terminate)) = (
          signal(SignalKind::interrupt()),
          signal(SignalKind::terminate()),
        ) else {
          error!("shutdown signals not handled");
          return;
        };
        let code = |name: &str| if name == "SIGINT" { 130 } else { 143 };
        let first = tokio::select! {
          _ = interrupt.recv() => "SIGINT",
          _ = terminate.recv() => "SIGTERM",
        };
        info!(signal = first, "Received signal, shutting down");
        cancel.cancel();
        let second = tokio::select! {
          _ = interrupt.recv() => "SIGINT",
          _ = terminate.recv() => "SIGTERM",
        };
        warn!(signal = second, "Received a second signal, exiting at once");
        std::process::exit(code(second));
      }
      .in_current_span(),
    );
  }
  #[cfg(not(unix))]
  {
    drop(watcher);
    tokio::spawn(
      async move {
        if tokio::signal::ctrl_c().await.is_ok() {
          info!("Received Ctrl-C, shutting down");
          cancel.cancel();
        }
        if tokio::signal::ctrl_c().await.is_ok() {
          std::process::exit(130);
        }
      }
      .in_current_span(),
    );
  }
}

/// A configuration error as printed: the problems one per line when there are several.
fn config_error(error: &cascade::Error) -> String {
  match error {
    cascade::Error::Invalid(invalid) => {
      let mut text = String::from("Invalid configuration:");
      for problem in invalid {
        text.push_str(&format!("\n  {}: {}", problem.key, problem.message));
      }
      text
    }
    other => format!("Invalid configuration: {other}"),
  }
}
