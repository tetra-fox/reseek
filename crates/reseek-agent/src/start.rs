//! Running the agent: reading the command line and the configuration, logging to standard
//! output, the directories it keeps its files in, its device id, and serving until a signal
//! stops it.

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;

use cascade::{Layers, Schema, Watch, Watcher};
use directories::ProjectDirs;
use reseek_shares::config_files::ConfigFiles;
use reseek_shares::log_format::{Clock, Line};
use reseek_shares::{Setup, Shares};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::cli::{self, Command, Invocation};
use crate::client::AgentClient;
use crate::options::{Options, validate};

/// The agent program: reads its command line and configuration and serves until it is stopped.
/// Exits with 2 for a command line it cannot read and 1 for a configuration or a start that
/// fails.
pub fn main() -> ExitCode {
  let schema = Schema::of::<Options>();
  let args = std::env::args_os().skip(1).collect();
  let command = match cli::parse(args, |name| std::env::var_os(name), &schema) {
    Ok(command) => command,
    Err(e) => {
      eprintln!("Invalid command line input: {e}");
      return ExitCode::from(2);
    }
  };
  match command {
    Command::Help => {
      print!("{}", cli::help(&schema));
      ExitCode::SUCCESS
    }
    Command::Version => {
      println!("reseek-agent {}", crate::VERSION);
      ExitCode::SUCCESS
    }
    Command::Run(invocation) => run(invocation),
  }
}

/// Where the agent keeps its files, resolved.
struct Paths {
  /// database backups made before migrations
  backups: PathBuf,
  /// the share index and its backup
  cache: PathBuf,
  /// finished downloads copied from the controller
  downloads: PathBuf,
  /// the copies of the share index made to upload it
  uploads: PathBuf,
  /// the file holding the device id
  device: PathBuf,
}

/// The base directories before the options are read: under `app_dir` when given, else the
/// platform's places for `reseek-agent`, kept apart from the daemon's.
#[derive(Clone)]
struct Defaults {
  config: Option<PathBuf>,
  data: Option<PathBuf>,
  cache: Option<PathBuf>,
}

impl Defaults {
  fn new(app_dir: Option<&Path>) -> Self {
    if let Some(app) = app_dir {
      return Self {
        config: Some(app.to_path_buf()),
        data: Some(app.join("data")),
        cache: Some(app.join("cache")),
      };
    }
    let dirs = ProjectDirs::from("", "", "reseek-agent");
    Self {
      config: dirs.as_ref().map(|d| d.config_dir().to_path_buf()),
      data: dirs.as_ref().map(|d| d.data_local_dir().to_path_buf()),
      cache: dirs.as_ref().map(|d| d.cache_dir().to_path_buf()),
    }
  }

  /// The data directory `options` give, else the default.
  fn data(&self, options: &Options) -> Option<PathBuf> {
    options.paths.data.clone().or_else(|| self.data.clone())
  }

  /// Every path for `options`.
  fn resolve(&self, options: &Options) -> Result<Paths, String> {
    let no_home = |key: &str| {
      format!(
        "no home directory to keep {key} in; give --app-dir or RESEEK_APP_DIR, or set the \
         paths.* options"
      )
    };
    let data = self.data(options).ok_or_else(|| no_home("paths.data"))?;
    let cache = options
      .paths
      .cache
      .clone()
      .or_else(|| self.cache.clone())
      .ok_or_else(|| no_home("paths.cache"))?;
    Ok(Paths {
      backups: data.join("backups"),
      device: data.join(DEVICE_FILE),
      downloads: downloads(options, &data).0,
      uploads: cache.join("uploads"),
      cache,
    })
  }
}

/// The file in the data directory that holds the agent's device id.
const DEVICE_FILE: &str = "device-id";

/// The downloads directory `options` give, else the default, and the default: `downloads` in
/// the data directory `data`.
fn downloads(options: &Options, data: &Path) -> (PathBuf, PathBuf) {
  let default = data.join("downloads");
  let given = options.directories.downloads.clone();
  (given.unwrap_or_else(|| default.clone()), default)
}

/// Loads the configuration, starts logging and serves until a signal stops the agent.
fn run(invocation: Invocation) -> ExitCode {
  let defaults = Defaults::new(invocation.app_dir.as_deref());
  let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
  let configured = invocation
    .flags
    .iter()
    .rev()
    .find(|(flag, _)| flag == "--config-dir")
    .map(|(_, dir)| PathBuf::from(dir))
    .or_else(|| std::env::var_os("RESEEK_CONFIG_DIR").map(PathBuf::from));
  let Some(config_dir) = configured.or_else(|| defaults.config.clone()) else {
    eprintln!("Invalid configuration: no home directory to keep paths.config in");
    return ExitCode::FAILURE;
  };
  let files = match ConfigFiles::new(&invocation.config_files, &config_dir) {
    Ok(files) => files,
    Err(e) => {
      eprintln!("{e}");
      return ExitCode::FAILURE;
    }
  };
  // the first options validated are the ones the agent starts with
  let started = OnceLock::new();
  let checked = defaults.clone();
  let layers = Layers::<Options>::new()
    .env(env)
    .files(&files.dir, files.names)
    .cli(invocation.flags)
    .validate(move |options: &Options| {
      // without a home directory both downloads paths are the same, so the check of a
      // configured one is skipped here and `resolve` reports the missing directory instead
      let data = checked.data(options).unwrap_or_default();
      let (downloads, default_downloads) = downloads(options, &data);
      let case_sensitive = *started.get_or_init(|| options.flags.case_sensitive_regex);
      validate(options, &downloads, &default_downloads, case_sensitive)
    });
  let watcher = match Watcher::new(layers) {
    Ok(watcher) => watcher,
    Err(e @ cascade::Error::Invalid(_)) => {
      eprintln!("{e}");
      return ExitCode::FAILURE;
    }
    Err(e) => {
      eprintln!("Invalid configuration: {e}");
      return ExitCode::FAILURE;
    }
  };
  let loaded = watcher.current();
  let options = &loaded.values;
  let level = if options.debug {
    tracing::Level::DEBUG
  } else {
    tracing::Level::INFO
  };
  let color = std::io::stdout().is_terminal();
  tracing_subscriber::fmt()
    .with_max_level(level)
    .with_ansi(color)
    .event_format(Line {
      clock: Clock::Time,
      color,
    })
    .init();
  for warning in &loaded.warnings {
    warn!(warning, "configuration");
  }
  let paths = match defaults.resolve(options) {
    Ok(paths) => paths,
    Err(e) => {
      error!(error = e, "the agent's directories are not usable");
      return ExitCode::FAILURE;
    }
  };
  if let Err(e) = create(&paths, options) {
    error!(error = e, "the agent's directories are not usable");
    return ExitCode::FAILURE;
  }
  let device = match device_id(&paths.device) {
    Ok(device) => device,
    Err(e) => {
      error!(error = e, "the agent's device id is not usable");
      return ExitCode::FAILURE;
    }
  };
  let runtime = match tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
  {
    Ok(runtime) => runtime,
    Err(e) => {
      error!(error = %e, "the runtime did not start");
      return ExitCode::FAILURE;
    }
  };
  runtime.block_on(serve(watcher, paths, device))
}

/// Creates the directories the agent writes to; a configured downloads directory must exist.
fn create(paths: &Paths, options: &Options) -> Result<(), String> {
  let mut made = vec![&paths.backups, &paths.cache, &paths.uploads];
  if options.directories.downloads.is_none() {
    made.push(&paths.downloads);
  }
  for dir in made {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
  }
  Ok(())
}

/// The device id kept in `file`, made and written there on the first start. A login with it
/// replaces the agent's earlier session on the controller, and one under the agent's name with
/// another id is refused while this one is connected.
fn device_id(file: &Path) -> Result<Uuid, String> {
  match std::fs::read_to_string(file) {
    Ok(text) => Uuid::try_parse(text.trim()).map_err(|e| {
      format!(
        "{}: not a device id ({e}); remove the file to make a new one",
        file.display()
      )
    }),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
      let device = Uuid::new_v4();
      // written beside it, synced, renamed and the directory synced, so a stop or a power cut
      // partway leaves no file or a whole one
      let written = file.with_extension("tmp");
      let write = || -> std::io::Result<()> {
        let mut out = std::fs::File::create(&written)?;
        writeln!(out, "{device}")?;
        out.sync_all()?;
        std::fs::rename(&written, file)?;
        sync_directory(file)
      };
      write().map_err(|e| format!("{}: {e}", file.display()))?;
      info!(%device, path = %file.display(), "Made a device id for this agent");
      Ok(device)
    }
    Err(e) => Err(format!("{}: {e}", file.display())),
  }
}

/// Syncs the directory holding `file`, so a rename into it is on disk.
#[cfg(unix)]
fn sync_directory(file: &Path) -> std::io::Result<()> {
  let dir = file.parent().expect("the file is named in a directory");
  std::fs::File::open(dir)?.sync_all()
}

/// On Windows the directory is not synced; the rename is left to the file system.
#[cfg(not(unix))]
fn sync_directory(_: &Path) -> std::io::Result<()> {
  Ok(())
}

/// Opens the share index and keeps the connection to the controller, as the device `device`,
/// until a signal stops the agent.
async fn serve(config: Watcher<Options>, paths: Paths, device: Uuid) -> ExitCode {
  let shutdown = CancellationToken::new();
  stop_on_signals(shutdown.clone());
  let options = config.current().values.clone();
  info!(
    version = crate::VERSION,
    instance = options.instance_name,
    %device,
    "reseek-agent starting"
  );
  let setup = Setup {
    cache: paths.cache,
    backups: paths.backups,
    downloads: paths.downloads.clone(),
    scan_at_startup: !options.flags.no_share_scan,
    force_scan: options.flags.force_share_scan,
    volatile: options.flags.volatile,
    case_sensitive: options.flags.case_sensitive_regex,
    rescan_on_schedule: true,
    // the controller answers browses from the index the agent uploads
    browse_caches: false,
    // nothing but the engine's own log follows the scans
    scans: Box::new(|_| Box::pin(async {})),
    // nothing shows the share state but the log the engine writes
    publish: Box::new(|_| {}),
  };
  let shares = match Shares::open(&config, |o: &Options| &o.shares, setup, shutdown.clone()).await {
    Ok(shares) => shares,
    Err(e) => {
      error!(error = %e, "the share index did not open");
      return ExitCode::FAILURE;
    }
  };
  let agent = AgentClient::new(
    config.clone(),
    shares,
    paths.downloads,
    paths.uploads,
    device,
    shutdown.clone(),
  );
  // the options change only from here, once the shares and the connection are built from the
  // ones loaded; an edit made since they loaded is read now
  config.start(Watch::Events, shutdown).await;
  agent.run().await;
  info!("reseek-agent stopped");
  ExitCode::SUCCESS
}

/// Cancels `shutdown` at the first interrupt or termination signal, and exits at once at a
/// second one.
fn stop_on_signals(shutdown: CancellationToken) {
  tokio::spawn(async move {
    #[cfg(unix)]
    {
      use tokio::signal::unix::{SignalKind, signal};
      let (Ok(mut interrupt), Ok(mut terminate)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
      ) else {
        error!("shutdown signals not handled");
        return;
      };
      tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
      }
      info!("Received signal, shutting down");
      shutdown.cancel();
      tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
      }
      warn!("Received a second signal, exiting at once");
      std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
      if let Err(e) = tokio::signal::ctrl_c().await {
        error!(error = %e, "shutdown signals not handled");
        return;
      }
      info!("Received signal, shutting down");
      shutdown.cancel();
      // a second signal ends the process, which the first no longer handles
      drop(tokio::signal::ctrl_c().await);
      warn!("Received a second signal, exiting at once");
      std::process::exit(1);
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_device_id_is_made_on_the_first_start_and_kept_after() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(DEVICE_FILE);
    let made = device_id(&file).unwrap();
    assert_eq!(
      std::fs::read_to_string(&file).unwrap().trim(),
      made.to_string()
    );
    assert_eq!(
      device_id(&file).unwrap(),
      made,
      "read again at the next start"
    );
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(left.len(), 1, "nothing but the file");
    std::fs::write(&file, "nas\n").unwrap();
    let damaged = device_id(&file).unwrap_err();
    assert!(damaged.contains("not a device id"), "{damaged}");
  }

  #[test]
  fn a_relative_directory_in_a_file_counts_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
      dir.path().join("config.toml"),
      "[directories]\ndownloads = \"dl\"\n[paths]\ndata = \"../data\"\n",
    )
    .unwrap();
    let loaded = Layers::<Options>::new()
      .files(dir.path(), ["config.toml"])
      .load()
      .unwrap();
    let options = loaded.values;
    assert_eq!(options.directories.downloads, Some(dir.path().join("dl")));
    assert_eq!(
      options.paths.data,
      Some(dir.path().parent().unwrap().join("data")),
      "its `..` resolved by name"
    );
  }
}
