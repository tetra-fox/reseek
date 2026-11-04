//! What a start prints and writes before serving: the startup log lines, the warnings the
//! options call for, the refusal of a default login others can reach, the configuration
//! template, and configuration errors as they are printed.

use cascade::Schema;
use tracing::{error, info, warn};

use crate::config::load::ConfigFiles;
use crate::config::{Options, template};
use crate::logging::Logging;
use crate::paths::Paths;
use crate::version;

/// The lines every start logs: the version, the system, this process, and where things are.
pub(super) fn log_startup(
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
pub(super) fn warn_about(options: &Options, logging: &Logging) {
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
  for (name, user) in &authentication.local.users {
    if user.password_hash.is_none() && user.password.is_some() {
      warn!(
        username = name,
        "local account has its password in plain text; set password_hash from `reseek hash-password`"
      );
    }
  }
  warn_about_metrics(&options.metrics);
}

/// Refuses a start whose built-in account still has the default password while other machines
/// can reach the web server; on this machine alone it is warned about. The error says how to
/// set a password.
pub(super) fn check_default_login(options: &Options) -> Result<(), String> {
  let auth = &options.web.authentication;
  let default_login = !auth.disabled
    && auth.local.enabled
    && auth.password_hash.is_none()
    && auth.password == crate::config::Authentication::default().password;
  if !default_login {
    return Ok(());
  }
  let Some(reached) = reached_from_elsewhere(&options.web) else {
    warn!(
      username = auth.username,
      "the built-in account still has the default password; set web.authentication.password_hash"
    );
    return Ok(());
  };
  Err(format!(
    "The built-in account {} still has the default password, and {reached}. Set a password \
     with RESEEK_PASSWORD or web.authentication.password, or web.authentication.password_hash \
     from `reseek hash-password`; or keep the web server on this machine with \
     web.ip_address = \"127.0.0.1\" and no reverse proxy.",
    auth.username
  ))
}

/// How other machines reach the web server, if they do: an HTTP or HTTPS address that is not
/// this machine's loopback, or a reverse proxy that forwards their requests
fn reached_from_elsewhere(web: &crate::config::Web) -> Option<String> {
  let https = (!web.https.disabled).then_some(&web.https.ip_address);
  let address = [Some(&web.ip_address), https]
    .into_iter()
    .flatten()
    // the addresses were checked with the rest of the configuration
    .flat_map(|list| crate::config::addresses::list(list.as_deref()).unwrap_or_default())
    .find(|ip| !ip.to_canonical().is_loopback());
  if let Some(ip) = address {
    return Some(format!(
      "the web server listens on {ip}, which other machines can reach"
    ));
  }
  if let Some(url) = &web.external_url {
    return Some(format!("the web server is served at {url}"));
  }
  (!web.trusted_proxies.is_empty()).then(|| {
    "web.trusted_proxies names a reverse proxy that forwards other machines' requests".into()
  })
}

/// The warnings an enabled metrics endpoint calls for: no authentication, or the default
/// credentials.
fn warn_about_metrics(metrics: &crate::config::Metrics) {
  if !metrics.enabled {
    return;
  }
  let credentials = &metrics.authentication;
  let defaults = crate::config::MetricsAuthentication::default();
  if credentials.disabled {
    warn!("Authentication for the metrics endpoint is DISABLED");
  } else if credentials.username == defaults.username && credentials.password == defaults.password {
    warn!(
      "the metrics endpoint still has the default credentials; set \
       metrics.authentication.username and metrics.authentication.password"
    );
  }
}

/// Writes the commented-out template to the first configuration file when it does not exist.
pub(super) fn write_template(files: &ConfigFiles, managed_by: Option<&str>) {
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

/// A configuration error as printed: the problems one per line when there are several.
pub(super) fn config_error(error: &cascade::Error) -> String {
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::Metrics;
  use crate::testing::logged;

  #[test]
  fn a_default_login_is_refused_only_where_other_machines_reach_it() {
    let mut options = Options::default();
    let refused = check_default_login(&options).unwrap_err();
    assert!(refused.contains("listens on ::"), "{refused}");
    options.web.ip_address = Some("127.0.0.1, ::1".into());
    assert!(
      check_default_login(&options).is_err(),
      "HTTPS listens everywhere"
    );
    options.web.https.ip_address = Some("[::ffff:127.0.0.1]".into());
    let ((), text) = logged(|| check_default_login(&options).unwrap());
    assert!(text.contains("default password"), "{text}");
    // a reverse proxy on this machine forwards requests from anywhere
    options.web.trusted_proxies = vec!["127.0.0.1/32".into()];
    assert!(check_default_login(&options).is_err());
    options.web.trusted_proxies.clear();
    options.web.external_url = Some("https://reseek.example.net".into());
    assert!(check_default_login(&options).is_err());
    options.web.ip_address = Some("192.168.1.2".into());
    options.web.authentication.password = "something else".into();
    assert_eq!(check_default_login(&options), Ok(()));
    options.web.authentication.password = "reseek".into();
    options.web.authentication.local.enabled = false;
    assert_eq!(check_default_login(&options), Ok(()));
  }

  #[test]
  fn an_open_or_default_metrics_endpoint_is_warned_about() {
    let mut metrics = Metrics {
      enabled: true,
      ..Metrics::default()
    };
    let ((), text) = logged(|| warn_about_metrics(&metrics));
    assert!(
      text.contains("WARN") && text.contains("default credentials"),
      "{text}"
    );
    metrics.authentication.disabled = true;
    let ((), text) = logged(|| warn_about_metrics(&metrics));
    assert!(
      text.contains("Authentication for the metrics endpoint is DISABLED"),
      "{text}"
    );
    metrics.authentication.disabled = false;
    metrics.authentication.password = "something else".into();
    let ((), text) = logged(|| warn_about_metrics(&metrics));
    assert!(text.is_empty(), "{text}");
    metrics.enabled = false;
    metrics.authentication.disabled = true;
    assert!(logged(|| warn_about_metrics(&metrics)).1.is_empty());
  }
}
