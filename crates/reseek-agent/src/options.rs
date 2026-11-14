//! The relay agent's options. Each is one of the daemon's, under the same key, environment
//! variable and flag, so a configuration written for an agent the daemon ran keeps working;
//! the daemon's other keys are reported as unknown.

use std::path::{Path, PathBuf};

use cascade::checks::{http_url, length, required};
use cascade::{Cascade, Invalid};
use reseek_relay_proto::check_instance_name;
use serde::{Deserialize, Serialize};

/// The whole configuration of the relay agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Options {
  /// Logs at debug level.
  #[cascade(env = "RESEEK_DEBUG", cli = "-d|--debug", apply = "restart")]
  pub debug: bool,
  /// Name of this agent in logs and to its controller, which lists it among its agents under
  /// this name.
  #[cascade(
    env = "RESEEK_INSTANCE_NAME",
    cli = "-i|--instance-name",
    apply = "restart"
  )]
  pub instance_name: String,
  /// Operational switches.
  #[cascade(apply = "restart")]
  pub flags: Flags,
  /// Directories for configuration, data and cache.
  #[cascade(apply = "restart")]
  pub paths: Paths,
  /// Download directories.
  #[cascade(apply = "restart")]
  pub directories: Directories,
  /// Shared directories.
  pub shares: reseek_shares::options::Shares,
  /// Relay mode.
  pub relay: Relay,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      debug: false,
      instance_name: "default".to_string(),
      flags: Flags::default(),
      paths: Paths::default(),
      directories: Directories::default(),
      shares: reseek_shares::options::Shares::default(),
      relay: Relay::default(),
    }
  }
}

/// Operational switches, all off by default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Flags {
  /// Skips the share scan at startup.
  #[cascade(env = "RESEEK_NO_SHARE_SCAN", cli = "--no-share-scan")]
  pub no_share_scan: bool,
  /// Runs a full share scan at startup, even when the share index is up to date.
  #[cascade(env = "RESEEK_FORCE_SHARE_SCAN", cli = "--force-share-scan")]
  pub force_share_scan: bool,
  /// Keeps the share index in memory only. Nothing is saved across restarts.
  #[cascade(env = "RESEEK_VOLATILE", cli = "--volatile")]
  pub volatile: bool,
  /// Makes the share filters case-sensitive.
  #[cascade(env = "RESEEK_CASE_SENSITIVE_REGEX", cli = "--case-sensitive-regex")]
  pub case_sensitive_regex: bool,
}

/// The base directories. Each defaults to the platform location for it, or to a directory
/// under the application directory when one is given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Paths {
  /// Configuration directory, which holds the configuration files. Set only through the
  /// environment or the command line.
  #[cascade(env = "RESEEK_CONFIG_DIR", cli = "--config-dir", path)]
  pub config: Option<PathBuf>,
  /// Data directory, which holds database backups and the default downloads directory. A relative
  /// path counts from the file that sets it.
  #[cascade(env = "RESEEK_DATA_DIR", cli = "--data-dir", path)]
  pub data: Option<PathBuf>,
  /// Cache directory, which holds the share index and the copies of it sent to the controller.
  /// A relative path counts from the file that sets it.
  #[cascade(env = "RESEEK_CACHE_DIR", cli = "--cache-dir", path)]
  pub cache: Option<PathBuf>,
}

/// Download directories.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Directories {
  /// Directory the controller's finished downloads are copied to, and the one
  /// `shares.share_downloads` shares. Defaults to `downloads` in the data directory. A
  /// directory set here must exist. A relative path counts from the file that sets it.
  #[cascade(env = "RESEEK_DOWNLOADS_DIR", cli = "-o|--downloads", path)]
  pub downloads: Option<PathBuf>,
}

/// Relay mode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Relay {
  /// Controller this agent connects to.
  pub controller: Controller,
}

/// Controller this agent connects to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Controller {
  /// Base URL of the controller, such as `https://host:5031`.
  #[cascade(env = "RESEEK_CONTROLLER_ADDRESS", cli = "--controller-address")]
  pub address: Option<String>,
  /// Accepts any TLS certificate from the controller. Otherwise its certificate must come from a
  /// certificate authority built in or one the system trusts.
  #[cascade(
    env = "RESEEK_CONTROLLER_IGNORE_CERTIFICATE_ERRORS",
    cli = "--controller-ignore-certificate-errors"
  )]
  pub ignore_certificate_errors: bool,
  /// API key of the controller, with the read-write or administrator role.
  #[cascade(
    env = "RESEEK_CONTROLLER_API_KEY",
    cli = "--controller-api-key",
    secret
  )]
  pub api_key: Option<String>,
  /// Secret the controller holds for this agent.
  #[cascade(env = "RESEEK_CONTROLLER_SECRET", cli = "--controller-secret", secret)]
  pub secret: Option<String>,
  /// Fetches finished downloads from the controller.
  #[cascade(env = "RESEEK_CONTROLLER_DOWNLOADS", cli = "--controller-downloads")]
  pub downloads: bool,
  /// Glob patterns matched against the path of a finished download under the downloads
  /// directory of the controller, with `/` as the separator. Only matching downloads are
  /// fetched. All downloads are fetched when empty.
  #[cascade(env = "RESEEK_CONTROLLER_DOWNLOADS_FILTER")]
  pub downloads_filter: Vec<String>,
}

/// Every problem with `options`, all of them rather than the first. `downloads` is the
/// downloads directory they give, `default_downloads` the one used when they give none, which
/// is created at startup rather than required to exist; the share filters are checked
/// matching case when `case_sensitive`.
pub fn validate(
  options: &Options,
  downloads: &Path,
  default_downloads: &Path,
  case_sensitive: bool,
) -> Result<(), Vec<Invalid>> {
  let mut report = Vec::new();
  let mut check = |key: &str, result: Result<(), String>| {
    if let Err(message) = result {
      report.push(Invalid::new(key, message));
    }
  };
  check(
    "instance_name",
    check_instance_name("instance_name", &options.instance_name),
  );
  let controller = &options.relay.controller;
  let key = "relay.controller.address";
  match controller.address.as_deref() {
    Some(address) if !address.trim().is_empty() => {
      check(key, http_url(key, address));
      check(key, no_userinfo(key, address));
    }
    address => check(key, required(key, address)),
  }
  for (key, value) in [
    ("relay.controller.api_key", &controller.api_key),
    ("relay.controller.secret", &controller.secret),
  ] {
    check(key, required(key, value.as_deref()));
    check(key, length(key, value.as_deref(), 16, 255));
  }
  let key = "relay.controller.downloads_filter";
  for pattern in &controller.downloads_filter {
    if let Err(e) = glob::Pattern::new(pattern) {
      check(key, Err(format!("The pattern {pattern} is invalid: {e}.")));
    }
  }
  if downloads != default_downloads && !downloads.is_dir() {
    check(
      "directories.downloads",
      Err(format!(
        "The field directories.downloads names {}, which is not a directory.",
        downloads.display()
      )),
    );
  }
  report.extend(reseek_shares::validate::check(
    &options.shares,
    downloads,
    case_sensitive,
  ));
  if report.is_empty() {
    Ok(())
  } else {
    Err(report)
  }
}

/// The controller address `value`, the option `key`, without a user name or password: the
/// relay socket cannot send them, so a controller behind Basic auth would refuse the socket
/// while it took the agent's other calls.
fn no_userinfo(key: &str, value: &str) -> Result<(), String> {
  match url::Url::parse(value) {
    Ok(url) if !url.username().is_empty() || url.password().is_some() => Err(format!(
      "The field {key} must not contain a user name or password, which the relay socket cannot \
       send."
    )),
    _ => Ok(()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn problems(options: &Options) -> Vec<String> {
    let downloads = Path::new("/nowhere/downloads");
    match validate(options, downloads, downloads, false) {
      Ok(()) => Vec::new(),
      Err(invalid) => invalid.into_iter().map(|i| i.key).collect(),
    }
  }

  #[test]
  fn an_agent_needs_its_controller_and_a_real_name() {
    let mut options = Options {
      instance_name: "local".into(),
      ..Options::default()
    };
    options.relay.controller.address = Some("ftp://controller".into());
    options.relay.controller.secret = Some("short".into());
    options.relay.controller.downloads_filter = vec!["[".into()];
    assert_eq!(
      problems(&options),
      [
        "instance_name",
        "relay.controller.address",
        "relay.controller.api_key",
        "relay.controller.secret",
        "relay.controller.downloads_filter",
      ]
    );
    options.instance_name = "nas".into();
    options.relay.controller = Controller {
      address: Some("https://controller.lan:5031".into()),
      api_key: Some("agent-key-0123456789".into()),
      secret: Some("agent-secret-0123456789".into()),
      ..Controller::default()
    };
    assert!(problems(&options).is_empty());
  }

  #[test]
  fn a_controller_address_with_a_user_or_password_is_refused() {
    let mut options = Options {
      instance_name: "nas".into(),
      ..Options::default()
    };
    options.relay.controller = Controller {
      api_key: Some("agent-key-0123456789".into()),
      secret: Some("agent-secret-0123456789".into()),
      ..Controller::default()
    };
    let downloads = Path::new("/nowhere/downloads");
    for address in [
      "https://nas:hunter2@controller.lan:5031",
      "https://nas@controller.lan:5031",
      "https://:hunter2@controller.lan:5031",
    ] {
      options.relay.controller.address = Some(address.into());
      let invalid = validate(&options, downloads, downloads, false).unwrap_err();
      assert_eq!(invalid.len(), 1, "{address}");
      assert_eq!(invalid[0].key, "relay.controller.address");
      assert!(
        invalid[0].message.contains("user name or password"),
        "{}",
        invalid[0].message
      );
    }
  }
}
