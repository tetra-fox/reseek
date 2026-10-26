//! Where reseek keeps its files: the four base directories, what lives in each, creating them,
//! and the lock that keeps a second instance off the same data.
//!
//! The defaults come from the platform's conventional places (the `directories` crate), or from
//! one tree under the application directory when `--app-dir` is given. A `paths.*` or
//! `directories.*` option overrides either.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::config::Options;
use crate::db::Location;

/// The four base directories before the options are read.
#[derive(Clone, Debug)]
pub struct Defaults {
  config: Option<PathBuf>,
  data: Option<PathBuf>,
  cache: Option<PathBuf>,
  logs: Option<PathBuf>,
}

/// Every place reseek reads or writes, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
  /// the configuration directory
  pub config: PathBuf,
  /// the data directory
  pub data: PathBuf,
  /// the cache directory
  pub cache: PathBuf,
  /// the log directory
  pub logs: PathBuf,
  /// where scripts run
  pub scripts: PathBuf,
  /// database backups made before migrations
  pub backups: PathBuf,
  /// finished downloads
  pub downloads: PathBuf,
  /// downloads in progress
  pub incomplete: PathBuf,
  /// whether `downloads` is the default, which is created here rather than required to exist
  pub downloads_is_default: bool,
  /// whether `incomplete` is the default
  pub incomplete_is_default: bool,
}

/// A directory that could not be used, with the message the operator sees.
#[derive(Debug, thiserror::Error)]
#[error("Filesystem exception: {0}")]
pub struct PathError(pub String);

impl Defaults {
  /// The defaults: under `app_dir` when given, else the platform's places for `reseek`.
  pub fn new(app_dir: Option<&Path>) -> Self {
    if let Some(app) = app_dir {
      return Self {
        config: Some(app.to_path_buf()),
        data: Some(app.join("data")),
        cache: Some(app.join("cache")),
        logs: Some(app.join("logs")),
      };
    }
    match ProjectDirs::from("", "", "reseek") {
      Some(dirs) => Self {
        config: Some(dirs.config_dir().to_path_buf()),
        data: Some(dirs.data_local_dir().to_path_buf()),
        cache: Some(dirs.cache_dir().to_path_buf()),
        // only some platforms have a state directory; the others keep logs with the data
        logs: Some(
          dirs
            .state_dir()
            .unwrap_or_else(|| dirs.data_local_dir())
            .join("logs"),
        ),
      },
      None => Self {
        config: None,
        data: None,
        cache: None,
        logs: None,
      },
    }
  }

  /// The default data directory, `None` when there is no home directory to put it in.
  pub fn data(&self) -> Option<&Path> {
    self.data.as_deref()
  }

  /// The configuration directory: `configured` (from `RESEEK_CONFIG_DIR` or `--config-dir`)
  /// when given, else the default.
  pub fn config_dir(&self, configured: Option<PathBuf>) -> Result<PathBuf, PathError> {
    configured
      .or_else(|| self.config.clone())
      .ok_or_else(|| no_home("paths.config"))
  }

  /// Every path for `options`.
  pub fn resolve(&self, options: &Options) -> Result<Paths, PathError> {
    let pick = |set: &Option<PathBuf>, default: &Option<PathBuf>, key: &str| {
      set
        .clone()
        .or_else(|| default.clone())
        .ok_or_else(|| no_home(key))
    };
    let config = pick(&options.paths.config, &self.config, "paths.config")?;
    let data = pick(&options.paths.data, &self.data, "paths.data")?;
    let cache = pick(&options.paths.cache, &self.cache, "paths.cache")?;
    let logs = pick(&options.paths.logs, &self.logs, "paths.logs")?;
    let (downloads, incomplete) = default_downloads(&data);
    let dirs = &options.directories;
    Ok(Paths {
      scripts: config.join("scripts"),
      backups: data.join("backups"),
      downloads_is_default: dirs.downloads.is_none(),
      incomplete_is_default: dirs.incomplete.is_none(),
      downloads: dirs.downloads.clone().unwrap_or(downloads),
      incomplete: dirs.incomplete.clone().unwrap_or(incomplete),
      config,
      data,
      cache,
      logs,
    })
  }
}

/// The default downloads and incomplete directories under the data directory `data`.
pub fn default_downloads(data: &Path) -> (PathBuf, PathBuf) {
  (data.join("downloads"), data.join("incomplete"))
}

fn no_home(key: &str) -> PathError {
  PathError(format!(
    "no home directory to keep {key} in; give --app-dir or RESEEK_APP_DIR, or set the paths.* \
     options"
  ))
}

impl Paths {
  /// Where the databases are opened: in the data directory, or in memory with
  /// `flags.volatile`.
  pub fn databases(&self, options: &Options) -> Location {
    Location {
      data: self.data.clone(),
      backups: self.backups.clone(),
      volatile: options.flags.volatile,
      log_sql: options.flags.log_sql,
    }
  }

  /// The file the runtime overrides are kept in.
  pub fn overrides(&self) -> PathBuf {
    self.data.join("overrides.toml")
  }

  /// The instance lock.
  pub fn lock(&self) -> PathBuf {
    self.data.join("reseek.lock")
  }

  /// Creates every directory reseek owns and checks it can write where it must. The download
  /// directories are only created when they are the defaults; a configured one must exist.
  pub fn create(&self) -> Result<(), PathError> {
    // the configuration directory may be read-only, such as a Nix store path
    create(&self.config)?;
    create(&self.scripts)?;
    for dir in [&self.data, &self.backups, &self.cache, &self.logs] {
      create(dir)?;
      writable(dir)?;
    }
    for (dir, default) in [
      (&self.downloads, self.downloads_is_default),
      (&self.incomplete, self.incomplete_is_default),
    ] {
      if default {
        create(dir)?;
        writable(dir)?;
      }
    }
    Ok(())
  }
}

fn create(dir: &Path) -> Result<(), PathError> {
  std::fs::create_dir_all(dir).map_err(|e| {
    PathError(format!(
      "Directory {} does not exist, and could not be created: {e}",
      dir.display()
    ))
  })
}

/// Checks `dir` is writable by creating and removing a file with a fresh name.
pub fn probe(dir: &Path) -> std::io::Result<()> {
  let probe = dir.join(uuid::Uuid::new_v4().to_string());
  File::create(&probe)?;
  std::fs::remove_file(&probe)
}

/// [`probe`] with the message of a directory reseek owns.
fn writable(dir: &Path) -> Result<(), PathError> {
  probe(dir).map_err(|e| PathError(format!("Directory {} is not writeable: {e}", dir.display())))
}

/// The lock that keeps a second instance off the same data directory; released when dropped
/// or when the process ends.
#[derive(Debug)]
pub struct InstanceLock {
  _file: File,
}

/// Why the instance lock was not taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
  /// another process holds it
  #[error("An instance of reseek is already running (lock held on {})", .0.display())]
  Held(PathBuf),
  /// the lock file could not be opened or locked
  #[error("could not take the instance lock {}: {source}", path.display())]
  Io {
    /// the lock file
    path: PathBuf,
    /// why
    source: std::io::Error,
  },
}

impl InstanceLock {
  /// Takes the lock at `path`, without waiting.
  pub fn acquire(path: &Path) -> Result<Self, LockError> {
    let io = |source| LockError::Io {
      path: path.to_path_buf(),
      source,
    };
    let file = File::options()
      .create(true)
      .truncate(false)
      .write(true)
      .open(path)
      .map_err(io)?;
    match file.try_lock() {
      Ok(()) => Ok(Self { _file: file }),
      Err(TryLockError::WouldBlock) => Err(LockError::Held(path.to_path_buf())),
      Err(TryLockError::Error(e)) => Err(io(e)),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_app_dir_puts_everything_under_one_root_and_options_override() {
    let defaults = Defaults::new(Some(Path::new("/app")));
    let mut options = Options::default();
    let paths = defaults.resolve(&options).unwrap();
    assert_eq!(paths.config, Path::new("/app"));
    assert_eq!(paths.data, Path::new("/app/data"));
    assert_eq!(paths.cache, Path::new("/app/cache"));
    assert_eq!(paths.logs, Path::new("/app/logs"));
    assert_eq!(paths.downloads, Path::new("/app/data/downloads"));
    assert_eq!(paths.scripts, Path::new("/app/scripts"));
    assert!(paths.downloads_is_default);

    options.paths.data = Some("/var/lib/reseek".into());
    options.directories.incomplete = Some("/scratch".into());
    let paths = defaults.resolve(&options).unwrap();
    assert_eq!(paths.downloads, Path::new("/var/lib/reseek/downloads"));
    assert_eq!(paths.incomplete, Path::new("/scratch"));
    assert!(!paths.incomplete_is_default);
    assert_eq!(paths.lock(), Path::new("/var/lib/reseek/reseek.lock"));
  }

  #[test]
  fn a_second_lock_on_the_same_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reseek.lock");
    let first = InstanceLock::acquire(&path).unwrap();
    assert!(matches!(
      InstanceLock::acquire(&path),
      Err(LockError::Held(_))
    ));
    drop(first);
    InstanceLock::acquire(&path).unwrap();
  }
}
