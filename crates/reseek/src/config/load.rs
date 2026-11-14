//! Building the configuration's layers from the process's environment, its files and its
//! command line.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cascade::{Layers, Overrides, Schema};

use super::Options;
use super::validate::{Context, validate};
use crate::paths::Defaults;
pub use reseek_shares::config_files::ConfigFiles;

/// The variable that marks the configuration files as managed by a tool, such as `Nix`.
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
}

/// The configuration directory the command line's option flags `cli` or the environment `env`
/// name.
pub fn given_config_dir(env: &[(OsString, OsString)], cli: &[(String, String)]) -> Option<PathBuf> {
  let flag = cli
    .iter()
    .rev()
    .find(|(flag, _)| flag == "--config-dir")
    .map(|(_, value)| PathBuf::from(value));
  flag.or_else(|| {
    env
      .iter()
      .find(|(name, _)| name == "RESEEK_CONFIG_DIR")
      .map(|(_, value)| PathBuf::from(value))
  })
}

/// The layers of `sources`, validated, with `overrides` on top when given.
pub fn layers(
  sources: &Sources,
  defaults: &Defaults,
  overrides: Option<Overrides>,
) -> Layers<Options> {
  let defaults = defaults.clone();
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
      let (default_downloads, default_incomplete) = defaults.downloads(&data);
      let started_case_sensitive = *started.get_or_init(|| options.flags.case_sensitive_regex);
      validate(
        options,
        &Context {
          default_downloads,
          default_incomplete,
          started_case_sensitive,
        },
      )
    });
  if let Some(overrides) = overrides {
    layers = layers.overrides(overrides);
  }
  layers
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

  /// A watcher over `config.toml` in `dir` holding `text`.
  fn watching(dir: &Path, text: &str) -> cascade::Watcher<Options> {
    std::fs::write(dir.join("config.toml"), text).unwrap();
    let sources = Sources {
      env: Vec::new(),
      files: ConfigFiles::new(&[], dir).unwrap(),
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
  fn a_relative_directory_in_a_file_counts_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("dl")).unwrap();
    std::fs::create_dir(dir.path().join("ui")).unwrap();
    std::fs::write(
      dir.path().join("config.toml"),
      "[directories]\ndownloads = \"dl\"\n[web]\ncontent_path = \"ui\"\n",
    )
    .unwrap();
    let sources = Sources {
      env: Vec::new(),
      files: ConfigFiles::new(&[], dir.path()).unwrap(),
      cli: vec![("--config-dir".into(), "conf".into())],
    };
    let defaults = Defaults::new(Some(dir.path()));
    let watcher = cascade::Watcher::new(layers(&sources, &defaults, None)).unwrap();
    let options = &watcher.current().values;
    let paths = defaults.resolve(options).unwrap();
    assert_eq!(paths.downloads, dir.path().join("dl"));
    assert_eq!(options.web.content_path, Some(dir.path().join("ui")));
    assert_eq!(
      paths.config,
      std::env::current_dir().unwrap().join("conf"),
      "one given on the command line counts from the working directory"
    );
  }

  /// The command line names the configuration through `..`, as `--config-file ../conf/x.toml`
  /// from another directory does
  #[cfg(unix)]
  #[test]
  fn a_config_file_named_through_dot_dot_counts_its_paths_from_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    let conf = dir.path().join("conf");
    std::fs::create_dir_all(conf.join("dl")).unwrap();
    std::fs::write(
      conf.join("reseek.toml"),
      "[directories]\ndownloads = \"dl\"\n",
    )
    .unwrap();
    let here = std::env::current_dir().unwrap();
    let up = "../".repeat(here.components().count() - 1);
    let through = |path: &Path| format!("{up}{}", path.strip_prefix("/").unwrap().display());
    let given = PathBuf::from(through(&conf.join("reseek.toml")));
    let sources = Sources {
      env: Vec::new(),
      files: ConfigFiles::new(&[given], dir.path()).unwrap(),
      cli: vec![("--config-dir".into(), through(&conf))],
    };
    let defaults = Defaults::new(Some(dir.path()));
    let watcher = cascade::Watcher::new(layers(&sources, &defaults, None)).unwrap();
    let paths = defaults.resolve(&watcher.current().values).unwrap();
    assert_eq!(paths.downloads, conf.join("dl"));
    assert_eq!(paths.config, conf);
  }

  #[test]
  fn the_relay_agent_mode_is_refused_with_a_pointer_to_the_agent_program() {
    let dir = tempfile::tempdir().unwrap();
    for mode in ["agent", "Debug"] {
      std::fs::write(
        dir.path().join("config.toml"),
        format!("[relay]\nenabled = true\nmode = \"{mode}\"\n"),
      )
      .unwrap();
      let sources = Sources {
        env: Vec::new(),
        files: ConfigFiles::new(&[], dir.path()).unwrap(),
        cli: Vec::new(),
      };
      let defaults = Defaults::new(Some(dir.path()));
      let Err(refused) = cascade::Watcher::new(layers(&sources, &defaults, None)) else {
        panic!("relay.mode = {mode} loaded");
      };
      let refused = refused.to_string();
      assert!(
        refused.contains("relay.mode") && refused.contains("reseek-agent"),
        "{refused}"
      );
    }
    let sources = Sources {
      env: vec![("RESEEK_RELAY_MODE".into(), "agent".into())],
      files: ConfigFiles::new(&[], dir.path()).unwrap(),
      cli: Vec::new(),
    };
    std::fs::write(dir.path().join("config.toml"), "").unwrap();
    let defaults = Defaults::new(Some(dir.path()));
    let refused = cascade::Watcher::new(layers(&sources, &defaults, None)).err();
    assert!(
      refused.is_some_and(|e| e.to_string().contains("reseek-agent")),
      "set by the environment"
    );
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
