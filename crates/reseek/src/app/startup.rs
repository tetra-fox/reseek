//! What a start prints and writes before serving: the startup log lines, the warnings the
//! options call for, the configuration template, and configuration errors as they are printed.

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
  let metrics = &options.metrics;
  if metrics.enabled {
    let credentials = &metrics.authentication;
    let defaults = crate::config::MetricsAuthentication::default();
    if credentials.disabled {
      warn!("Authentication for the metrics endpoint is DISABLED");
    } else if credentials.username == defaults.username && credentials.password == defaults.password
    {
      warn!(
        "the metrics endpoint still has the default credentials; set \
         metrics.authentication.username and metrics.authentication.password"
      );
    }
  }
  if options.logger.loki.is_some() {
    warn!("forwarding logs to Grafana Loki is not available in this build; logger.loki ignored");
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
