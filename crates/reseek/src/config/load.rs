//! Building the configuration's layers from the process's environment, its files and its
//! command line.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cascade::{Layers, Overrides, Schema};

use super::Options;
use super::validate::{Context, validate};
use crate::paths::{Defaults, default_downloads};

/// The variable that marks the configuration files as managed by a tool, such as `nixos`.
pub const MANAGED_BY: &str = "RESEEK_CONFIG_MANAGED_BY";

/// Variables starting with the prefix that are read outside the options.
const NOT_OPTIONS: &[&str] = &[
  "RESEEK_APP_DIR",
  "RESEEK_CONFIG",
  MANAGED_BY,
  "RESEEK_UMASK",
];

/// Variables starting with this are build information nothing reads.
const BUILD_INFO_PREFIX: &str = "RESEEK_DOCKER_";

/// Where the configuration files are: a directory and the files in it, in the order they
/// apply.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigFiles {
  /// the directory the names are relative to; the `config.d` drop-ins are in it
  pub dir: PathBuf,
  /// the files, then the drop-in pattern
  pub names: Vec<String>,
}

impl ConfigFiles {
  /// `config.toml` in `config_dir`, or the files given on the command line, followed by every
  /// `config.d/*.toml` next to the first of them.
  pub fn new(given: &[PathBuf], config_dir: &Path) -> Self {
    let Some((first, rest)) = given.split_first() else {
      return Self {
        dir: config_dir.to_path_buf(),
        names: vec!["config.toml".to_string(), "config.d/*.toml".to_string()],
      };
    };
    let first = std::path::absolute(first).unwrap_or_else(|_| first.clone());
    let dir = first
      .parent()
      .map_or_else(|| config_dir.to_path_buf(), Path::to_path_buf);
    let name = |path: &Path| match path.strip_prefix(&dir) {
      Ok(relative) => relative.display().to_string(),
      Err(_) => path.display().to_string(),
    };
    let mut names = vec![name(&first)];
    names.extend(rest.iter().map(|path| {
      let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
      name(&path)
    }));
    names.push("config.d/*.toml".to_string());
    Self { dir, names }
  }

  /// The first file, which is where a new configuration is written.
  pub fn first(&self) -> PathBuf {
    self.dir.join(&self.names[0])
  }
}

/// Everything the layers are read from.
pub struct Sources {
  /// the process's environment
  pub env: Vec<(OsString, OsString)>,
  /// the configuration files
  pub files: ConfigFiles,
  /// the command line's option flags as `(long flag, value)`
  pub cli: Vec<(String, String)>,
}

impl Sources {
  /// The tool `RESEEK_CONFIG_MANAGED_BY` names, when set.
  pub fn managed_by(&self) -> Option<String> {
    self
      .env
      .iter()
      .find(|(name, _)| name == MANAGED_BY)
      .and_then(|(_, value)| value.to_str().map(str::to_string))
      .filter(|value| !value.is_empty())
  }

  /// The configuration directory the environment or the command line names.
  pub fn config_dir(&self) -> Option<PathBuf> {
    let flag = self
      .cli
      .iter()
      .rev()
      .find(|(flag, _)| flag == "--config-dir")
      .map(|(_, value)| PathBuf::from(value));
    flag.or_else(|| {
      self
        .env
        .iter()
        .find(|(name, _)| name == "RESEEK_CONFIG_DIR")
        .map(|(_, value)| PathBuf::from(value))
    })
  }
}

/// The layers of `sources`, validated, with `overrides` on top when given.
pub fn layers(
  sources: &Sources,
  defaults: &Defaults,
  overrides: Option<Overrides>,
) -> Layers<Options> {
  let defaults = defaults.clone();
  let program_dir = program_dir();
  // the first options validated are the ones the daemon starts with
  let started = Arc::new(OnceLock::new());
  let mut layers = Layers::<Options>::new()
    .env(sources.env.clone())
    .files(&sources.files.dir, sources.files.names.clone())
    .cli(sources.cli.clone())
    .managed_by(sources.managed_by())
    .validate(move |options: &Options| {
      let data = options
        .paths
        .data
        .clone()
        .or_else(|| defaults.data().map(Path::to_path_buf))
        .unwrap_or_default();
      let (default_downloads, default_incomplete) = default_downloads(&data);
      let started_case_sensitive = *started.get_or_init(|| options.flags.case_sensitive_regex);
      validate(
        options,
        &Context {
          default_downloads,
          default_incomplete,
          program_dir: program_dir.clone(),
          started_case_sensitive,
        },
      )
    });
  if let Some(overrides) = overrides {
    layers = layers.overrides(overrides);
  }
  layers
}

/// The directory of the running program, which `web.content_path` is relative to.
pub fn program_dir() -> PathBuf {
  std::env::current_exe()
    .ok()
    .and_then(|exe| exe.parent().map(Path::to_path_buf))
    .unwrap_or_default()
}

/// The set variables that start with `RESEEK_` but no option reads and nothing else reads
/// either, in name order.
pub fn unknown_variables(schema: &Schema, env: &[(OsString, OsString)]) -> Vec<String> {
  let mut unknown: Vec<String> = env
    .iter()
    .filter_map(|(name, _)| name.to_str())
    .filter(|name| name.starts_with("RESEEK_"))
    .filter(|name| !NOT_OPTIONS.contains(name) && !name.starts_with(BUILD_INFO_PREFIX))
    .filter(|name| {
      !schema
        .fields()
        .iter()
        .any(|field| field.env_names().any(|n| n == *name))
    })
    .map(str::to_string)
    .collect();
  unknown.sort();
  unknown
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn given_files_come_first_then_the_drop_ins_next_to_the_first() {
    let files = ConfigFiles::new(&[], Path::new("/home/u/.config/reseek"));
    assert_eq!(files.names, ["config.toml", "config.d/*.toml"]);
    assert_eq!(
      files.first(),
      Path::new("/home/u/.config/reseek/config.toml")
    );
    let files = ConfigFiles::new(
      &[
        "/etc/reseek/config.toml".into(),
        "/run/secrets/reseek.toml".into(),
      ],
      Path::new("/var/lib/reseek"),
    );
    assert_eq!(files.dir, Path::new("/etc/reseek"));
    assert_eq!(
      files.names,
      ["config.toml", "/run/secrets/reseek.toml", "config.d/*.toml"]
    );
  }

  /// A watcher over `config.toml` in `dir` holding `text`.
  fn watching(dir: &Path, text: &str) -> cascade::Watcher<Options> {
    std::fs::write(dir.join("config.toml"), text).unwrap();
    let sources = Sources {
      env: Vec::new(),
      files: ConfigFiles::new(&[], dir),
      cli: Vec::new(),
    };
    cascade::Watcher::new(layers(&sources, &Defaults::new(Some(dir)), None)).unwrap()
  }

  #[tokio::test]
  async fn search_filters_are_checked_with_the_flag_the_daemon_started_with() {
    // compiles matching case, and is too large to compile ignoring it
    let filter = "[a-z]{50000}";
    let reloaded = |sensitive: bool| {
      format!(
        "[flags]\ncase_sensitive_regex = {sensitive}\n[filters.search]\nrequest = ['{filter}']\n"
      )
    };
    let dir = tempfile::tempdir().unwrap();
    let watcher = watching(dir.path(), "");
    std::fs::write(dir.path().join("config.toml"), reloaded(true)).unwrap();
    // started ignoring case, the filter would not be used
    let refused = watcher.reload().await.unwrap_err().to_string();
    assert!(refused.contains("filters.search.request"), "{refused}");

    let dir = tempfile::tempdir().unwrap();
    let watcher = watching(dir.path(), "[flags]\ncase_sensitive_regex = true\n");
    std::fs::write(dir.path().join("config.toml"), reloaded(false)).unwrap();
    watcher.reload().await.unwrap();
    assert_eq!(watcher.current().values.filters.search.request, [filter]);
  }

  #[test]
  fn unknown_variables_skip_the_ones_read_elsewhere() {
    let env: Vec<(OsString, OsString)> = [
      "RESEEK_HTTP_PORT",
      "RESEEK_HTTP_PROT",
      "RESEEK_APP_DIR",
      "RESEEK_CONFIG_MANAGED_BY",
      "RESEEK_DOCKER_TAG",
      "RESEEK_UMASK",
      "HOME",
    ]
    .into_iter()
    .map(|n| (n.into(), "x".into()))
    .collect();
    assert_eq!(
      unknown_variables(&Schema::of::<Options>(), &env),
      ["RESEEK_HTTP_PROT"]
    );
  }
}
