//! An example configuration that uses every attribute, shared by the tests.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{Cascade, Change, Invalid, Key, Kind, Layers, Overrides, Schema, Shape, Watcher};

/// The root of the example.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[cascade(env_alias_prefix(from = "APP_", to = "NEW_"))]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Example {
  /// Debug mode.
  #[cascade(env = "APP_DEBUG", cli = "-d|--debug", apply = "restart")]
  pub(crate) debug: bool,
  /// Names this instance in logs.
  #[cascade(
    env = "APP_INSTANCE_NAME",
    cli = "-i|--instance-name",
    apply = "restart"
  )]
  pub(crate) instance_name: String,
  #[cascade(apply = "reconnect")]
  pub(crate) server: Server,
  pub(crate) shares: Shares,
  /// Keys that may call the API, by name.
  #[cascade(user_keyed)]
  pub(crate) api_keys: BTreeMap<String, ApiKey>,
  /// Where events are sent.
  pub(crate) webhooks: Vec<Webhook>,
  #[cascade(apply = "restart")]
  pub(crate) flags: Flags,
}

impl Default for Example {
  fn default() -> Self {
    Self {
      debug: false,
      instance_name: "default".into(),
      server: Server::default(),
      shares: Shares::default(),
      api_keys: BTreeMap::new(),
      webhooks: Vec::new(),
      flags: Flags::default(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Server {
  /// The server's host name.
  #[cascade(env = "APP_SERVER_ADDRESS", cli = "--server-address")]
  pub(crate) address: String,
  /// The server's port.
  #[cascade(env = "APP_SERVER_PORT", env_alias = "NEW_PORT", cli = "--server-port")]
  pub(crate) port: u16,
  /// The user to log in as.
  #[cascade(env = "APP_USERNAME", cli = "--username")]
  pub(crate) username: String,
  /// The password to log in with.
  #[cascade(env = "APP_SERVER_PASSWORD", cli = "--password", secret)]
  pub(crate) password: Option<String>,
  /// How long to wait for the server.
  #[cascade(env = "APP_SERVER_TIMEOUT", apply = "live")]
  #[serde(with = "humantime_serde")]
  pub(crate) timeout: Duration,
  /// The read buffer.
  #[cascade(env = "APP_SERVER_BUFFER", kind = "size")]
  pub(crate) buffer: bytesize::ByteSize,
}

impl Default for Server {
  fn default() -> Self {
    Self {
      address: "localhost".into(),
      port: 2242,
      username: "reseek".into(),
      password: None,
      timeout: Duration::from_secs(5),
      buffer: bytesize::ByteSize::kib(16),
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Shares {
  /// Shared directories.
  #[cascade(env = "APP_SHARED_DIR", cli = "-s|--shared")]
  pub(crate) directories: Vec<String>,
  /// Per-share limits.
  #[cascade(env = "APP_SHARE_LIMITS", cli = "--share-limit")]
  pub(crate) limits: Vec<u32>,
  pub(crate) cache: Cache,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Cache {
  /// Scan workers.
  #[cascade(env = "APP_CACHE_WORKERS", apply = "restart")]
  pub(crate) workers: u32,
  /// Where the cache lives.
  #[cascade(env = "APP_CACHE_MODE")]
  pub(crate) mode: Mode,
}

impl Default for Cache {
  fn default() -> Self {
    Self {
      workers: 4,
      mode: Mode::Memory,
    }
  }
}

/// A leaf enum that describes itself.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
  Memory,
  Disk,
}

impl Cascade for Mode {
  fn shape() -> Shape {
    Shape::leaf(Kind::String)
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApiKey {
  #[cascade(secret)]
  pub(crate) key: String,
  #[serde(default)]
  pub(crate) role: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(deny_unknown_fields)]
pub(crate) struct Webhook {
  pub(crate) url: String,
  #[cascade(secret)]
  pub(crate) token: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Flags {
  /// Do not connect at startup.
  #[cascade(env = "APP_NO_CONNECT", cli = "--no-connect")]
  pub(crate) no_connect: bool,
  /// Keep nothing on disk.
  #[cascade(env = "APP_VOLATILE", cli = "--volatile")]
  pub(crate) volatile: bool,
}

/// The schema of [`Example`].
pub(crate) fn example_schema() -> Schema {
  Schema::of::<Example>()
}

/// Whether a 0o555 directory refuses writes here; root and some filesystems ignore the mode,
/// and then a test of a read-only directory proves nothing.
#[cfg(unix)]
pub(crate) fn read_only_dirs_hold(dir: &Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
  let probe = dir.join(".probe");
  let refused = std::fs::File::create(&probe).is_err();
  // only exists when the mode was ignored, and then the test returns early anyway
  drop(std::fs::remove_file(&probe));
  std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
  refused
}

/// Layers over `config.toml` and `config.d/*.toml` in `dir`.
pub(crate) fn example_layers(dir: &Path) -> Layers<Example> {
  Layers::new().files(dir, ["config.toml", "config.d/*.toml"])
}

/// Writes `text` to `name` under `dir`, making the directories on the way.
pub(crate) fn write(dir: &Path, name: &str, text: &str) {
  let path = dir.join(name);
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, text).unwrap();
}

/// Owned name and value pairs, as env vars or flags are given.
pub(crate) fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
  pairs
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Layers over `config.toml` and `config.d/*.toml` in `dir`, with `APP_USERNAME` set, overrides
/// kept in `dir`, and a check that refuses ports below 1024.
pub(crate) fn watched_layers(dir: &Path) -> Layers<Example> {
  Layers::new()
    .files(dir, ["config.toml", "config.d/*.toml"])
    .env([("APP_USERNAME".to_string(), "from-env".to_string())])
    .overrides(Overrides::load(dir.join("overrides.toml")).unwrap())
    .validate(|o: &Example| {
      if o.server.port < 1024 {
        Err(vec![Invalid::new("server.port", "must be 1024 or more")])
      } else {
        Ok(())
      }
    })
}

/// A watcher of [`watched_layers`].
pub(crate) fn watched(dir: &Path) -> Watcher<Example> {
  Watcher::new(watched_layers(dir)).unwrap()
}

/// Keys and their values.
pub(crate) fn values(pairs: &[(&str, Value)]) -> BTreeMap<Key, Value> {
  pairs
    .iter()
    .map(|(k, v)| (k.to_string(), v.clone()))
    .collect()
}

/// The next change, failing the test when none comes within five seconds.
pub(crate) async fn next_change(
  changes: &mut broadcast::Receiver<Arc<Change<Example>>>,
) -> Arc<Change<Example>> {
  tokio::time::timeout(Duration::from_secs(5), changes.recv())
    .await
    .expect("a change in time")
    .unwrap()
}
