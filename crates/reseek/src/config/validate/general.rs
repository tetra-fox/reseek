//! The options outside the larger sections: the instance name, relay mode, the base and
//! download directories, the blocklist, search filters, logging, metrics, throttling,
//! retention and blocking.

use std::collections::BTreeMap;
use std::time::Duration;

use super::blacklist::{UNDETECTED, detect_file};
use super::fields::{
  cidr_list, duration_at_least, duration_off_or_at_least, http_url, is_regex, length,
  off_or_at_least, range, required, url_path,
};
use super::paths::{absolute, download_directory, readable_file};
use super::{Context, Report};
use crate::config::enums::RelayMode;
use crate::config::{Blacklist, Logging, Options, Relay, Retention, TransferRetention};

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(24 * 3600);

/// The levels `logging.level` and `logging.targets` may name.
const LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

pub(super) fn check(options: &Options, context: &Context, report: &mut Report) {
  if options.relay.mode == RelayMode::Agent && options.instance_name == "local" {
    report.add(
      "instance_name",
      "Instance name must be something other than 'local' when operating in Relay Agent mode",
    );
  }
  check_relay(&options.relay, report);
  let paths = [
    ("paths.config", &options.paths.config),
    ("paths.data", &options.paths.data),
    ("paths.cache", &options.paths.cache),
    ("paths.logs", &options.paths.logs),
  ];
  for (key, path) in paths {
    if let Some(path) = path {
      report.check(key, absolute(key, path));
    }
  }
  let directories = [
    (
      "directories.incomplete",
      &options.directories.incomplete,
      &context.default_incomplete,
    ),
    (
      "directories.downloads",
      &options.directories.downloads,
      &context.default_downloads,
    ),
  ];
  for (key, path, default) in directories {
    if let Some(path) = path {
      report.check(key, download_directory(key, path, default));
    }
  }
  check_blacklist(&options.blacklist, report);
  for filter in &options.filters.search.request {
    if !is_regex(filter, context.started_case_sensitive) {
      report.add(
        "filters.search.request",
        format!("Search request filter '{filter}' is not a valid regular expression"),
      );
    }
  }
  if let Some(loki) = &options.logger.loki {
    report.check("logger.loki", http_url("logger.loki", loki));
  }
  check_logging(&options.logging, report);
  let metrics = &options.metrics;
  report.check("metrics.url", url_path("metrics.url", &metrics.url));
  report.check("metrics.url", metrics_url(&metrics.url));
  for (key, value) in [
    (
      "metrics.authentication.username",
      &metrics.authentication.username,
    ),
    (
      "metrics.authentication.password",
      &metrics.authentication.password,
    ),
  ] {
    report.check(key, length(key, Some(value), 1, 255));
  }
  let incoming = &options.throttling.search.incoming;
  for (key, value, min, max) in [
    (
      "throttling.search.incoming.concurrency",
      incoming.concurrency,
      1,
      100,
    ),
    (
      "throttling.search.incoming.circuit_breaker",
      incoming.circuit_breaker,
      100,
      10000,
    ),
    (
      "throttling.search.incoming.response_file_limit",
      incoming.response_file_limit,
      100,
      5000,
    ),
  ] {
    report.check(key, range(key, value, min, max));
  }
  check_retention(&options.retention, report);
  for scope in &options.blocking.default_scopes {
    if let Err(message) = scope.parse::<api_types::blocking::BlockScope>() {
      report.add("blocking.default_scopes", message);
    }
  }
  let refresh = "transfers.ui.queue_refresh";
  report.check(
    refresh,
    duration_at_least(
      refresh,
      options.transfers.ui.queue_refresh,
      Duration::from_secs(30),
    ),
  );
}

/// The controller is checked only in agent mode, the agents only when not.
fn check_relay(relay: &Relay, report: &mut Report) {
  if relay.mode == RelayMode::Agent {
    let controller = &relay.controller;
    let key = "relay.controller.address";
    match controller.address.as_deref() {
      Some(address) if !address.trim().is_empty() => report.check(key, http_url(key, address)),
      address => report.check(key, required(key, address)),
    }
    for (key, value) in [
      ("relay.controller.api_key", &controller.api_key),
      ("relay.controller.secret", &controller.secret),
    ] {
      report.check(key, required(key, value.as_deref()));
      report.check(key, length(key, value.as_deref(), 16, 255));
    }
    return;
  }
  let key = "relay.agents";
  let mut names = BTreeMap::new();
  for (name, agent) in &relay.agents {
    let field = |f: &str| format!("{key}.{name}.{f}");
    report.check(
      key,
      required(&field("instance_name"), agent.instance_name.as_deref()),
    );
    report.check(
      key,
      length(
        &field("instance_name"),
        agent.instance_name.as_deref(),
        1,
        255,
      ),
    );
    report.check(key, required(&field("secret"), agent.secret.as_deref()));
    report.check(
      key,
      length(&field("secret"), agent.secret.as_deref(), 16, 255),
    );
    report.check(key, cidr_list(&agent.cidr));
    if let Some(instance) = &agent.instance_name {
      *names.entry(instance.as_str()).or_insert(0) += 1;
    }
  }
  if names.values().any(|&count| count > 1) {
    report.add(
      key,
      "One or more Agent instance names are duplicated.  Ensure instance names are unique.",
    );
  }
}

fn check_blacklist(blacklist: &Blacklist, report: &mut Report) {
  let key = "blacklist.file";
  match &blacklist.file {
    Some(file) => match readable_file(key, file) {
      Err(message) => report.add(key, message),
      Ok(()) if blacklist.enabled => {
        // a file that can not be read was refused by the readable check above
        if detect_file(file).ok().flatten().is_none() {
          report.add(key, UNDETECTED);
        }
      }
      Ok(()) => {}
    },
    None if blacklist.enabled => report.add(
      key,
      "The field blacklist.enabled is true, but no blacklist.file has been specified.",
    ),
    None => {}
  }
}

fn check_logging(logging: &Logging, report: &mut Report) {
  if !LEVELS.contains(&logging.level.to_ascii_lowercase().as_str()) {
    report.add(
      "logging.level",
      format!(
        "The field logging.level must be one of: {}.",
        LEVELS.join(", ")
      ),
    );
  }
  for directive in &logging.targets {
    let valid = directive.split_once('=').is_some_and(|(target, level)| {
      !target.is_empty()
        && !target.contains(char::is_whitespace)
        && LEVELS.contains(&level.to_ascii_lowercase().as_str())
    });
    if !valid {
      report.add(
        "logging.targets",
        format!(
          "Log target '{directive}' must be a module and a level, such as reseek::http=debug"
        ),
      );
    }
  }
}

/// Refuses a metrics path that is the web UI's root or lies under a path the daemon serves
/// itself.
fn metrics_url(url: &str) -> Result<(), String> {
  let path = format!("/{}", url.trim_start_matches('/'));
  let path = path.trim_end_matches('/');
  if path.is_empty() {
    return Err("The field metrics.url must not be the root of the web UI.".to_string());
  }
  for taken in ["/api", "/hub", "/health", "/swagger"] {
    if path == taken || path.starts_with(&format!("{taken}/")) {
      return Err(format!("The field metrics.url must not be under {taken}."));
    }
  }
  Ok(())
}

fn check_retention(retention: &Retention, report: &mut Report) {
  if let Some(search) = retention.search {
    report.check(
      "retention.search",
      duration_at_least("retention.search", search, 5 * MINUTE),
    );
  }
  for (direction, transfers) in [
    ("upload", &retention.transfers.upload),
    ("download", &retention.transfers.download),
  ] {
    check_transfer_retention(direction, transfers, report);
  }
  for (key, value) in [
    ("retention.files.complete", retention.files.complete),
    ("retention.files.incomplete", retention.files.incomplete),
  ] {
    if let Some(value) = value {
      report.check(key, duration_at_least(key, value, 30 * MINUTE));
    }
  }
  report.check(
    "retention.logs",
    duration_at_least("retention.logs", retention.logs, 2 * DAY),
  );
  let messages = &retention.messages;
  report.check(
    "retention.messages.rooms",
    duration_off_or_at_least("retention.messages.rooms", messages.rooms, HOUR),
  );
  for (key, value) in [
    (
      "retention.messages.max_per_conversation",
      messages.max_per_conversation,
    ),
    ("retention.messages.max_per_room", messages.max_per_room),
  ] {
    report.check(key, off_or_at_least(key, value, 250));
  }
}

fn check_transfer_retention(direction: &str, retention: &TransferRetention, report: &mut Report) {
  for (outcome, value) in [
    ("succeeded", retention.succeeded),
    ("errored", retention.errored),
    ("cancelled", retention.cancelled),
    ("failed", retention.failed),
  ] {
    if let Some(value) = value {
      let key = format!("retention.transfers.{direction}.{outcome}");
      report.check(&key, duration_at_least(&key, value, 5 * MINUTE));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::RelayAgent;

  fn problems(options: &Options) -> Vec<(String, String)> {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
      default_downloads: dir.path().join("downloads"),
      default_incomplete: dir.path().join("incomplete"),
      program_dir: dir.path().to_path_buf(),
      started_case_sensitive: false,
    };
    let mut report = Report::default();
    check(options, &context, &mut report);
    report.0.into_iter().map(|i| (i.key, i.message)).collect()
  }

  #[test]
  fn the_metrics_path_stays_clear_of_the_daemons_own() {
    assert!(metrics_url("/metrics").is_ok());
    assert!(metrics_url("stats/prometheus").is_ok());
    assert!(metrics_url("/apis").is_ok());
    for taken in [
      "/",
      "",
      "/api",
      "api/metrics",
      "/health",
      "/hub/x",
      "/swagger/",
    ] {
      assert!(metrics_url(taken).is_err(), "{taken:?}");
    }
  }

  #[test]
  fn an_agent_needs_its_controller_and_a_real_name() {
    let mut options = Options::default();
    options.relay.mode = RelayMode::Agent;
    options.instance_name = "local".into();
    options.relay.controller.address = Some("ftp://controller".into());
    options.relay.controller.secret = Some("short".into());
    let keys: Vec<String> = problems(&options).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
      keys,
      [
        "instance_name",
        "relay.controller.address",
        "relay.controller.api_key",
        "relay.controller.secret",
      ]
    );
  }

  #[test]
  fn agents_need_unique_names_and_secrets() {
    let mut options = Options::default();
    let agent = |name: &str| RelayAgent {
      instance_name: Some(name.into()),
      secret: Some("0123456789abcdef".into()),
      ..RelayAgent::default()
    };
    options.relay.agents.insert("a".into(), agent("same"));
    options.relay.agents.insert("b".into(), agent("same"));
    options.relay.agents.insert(
      "c".into(),
      RelayAgent {
        cidr: "10.0.0.0/8,::ffff:0:0/96".into(),
        ..RelayAgent::default()
      },
    );
    let messages: Vec<String> = problems(&options).into_iter().map(|(_, m)| m).collect();
    assert_eq!(
      messages,
      [
        "The relay.agents.c.instance_name field must contain a value",
        "The relay.agents.c.secret field must contain a value",
        "CIDR ::ffff:0:0/96 is invalid: IPv4 mapped IPv6 addresses are not allowed",
        "One or more Agent instance names are duplicated.  Ensure instance names are unique.",
      ]
    );
  }

  #[test]
  fn an_enabled_blacklist_needs_a_detectable_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = Options::default();
    options.blacklist.enabled = true;
    assert_eq!(
      problems(&options)[0].1,
      "The field blacklist.enabled is true, but no blacklist.file has been specified."
    );
    let file = dir.path().join("list.txt");
    std::fs::write(&file, "# nothing but words\nhello\n").unwrap();
    options.blacklist.file = Some(file.clone());
    assert_eq!(problems(&options)[0].1, UNDETECTED);
    std::fs::write(&file, "1.2.4.0/24\n").unwrap();
    assert!(problems(&options).is_empty());
  }

  #[test]
  fn retention_limits_may_be_off_or_above_their_minimum() {
    let mut options = Options::default();
    options.retention.messages.rooms = Duration::ZERO;
    options.retention.messages.max_per_room = 0;
    assert!(problems(&options).is_empty());
    options.retention.messages.rooms = 10 * MINUTE;
    options.retention.messages.max_per_room = 10;
    options.retention.transfers.download.failed = Some(MINUTE);
    let keys: Vec<String> = problems(&options).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
      keys,
      [
        "retention.transfers.download.failed",
        "retention.messages.rooms",
        "retention.messages.max_per_room",
      ]
    );
  }

  #[test]
  fn log_levels_and_targets_are_checked() {
    let mut options = Options::default();
    options.logging.level = "DEBUG".into();
    options.logging.targets = vec!["soulseek_client::distributed=trace".into()];
    assert!(problems(&options).is_empty());
    options.logging.level = "verbose".into();
    options.logging.targets = vec!["reseek".into(), "=debug".into()];
    assert_eq!(problems(&options).len(), 3);
  }

  #[test]
  fn a_search_request_filter_must_compile_within_its_size() {
    let mut options = Options::default();
    options.filters.search.request = vec!["^.{1,2}$".into()];
    assert!(problems(&options).is_empty());
    // a pattern this large would be matched against every search that reaches us
    let huge = "\\w{1000}".to_string();
    options.filters.search.request = vec!["(".into(), huge.clone()];
    assert_eq!(
      problems(&options),
      [
        (
          "filters.search.request".to_string(),
          "Search request filter '(' is not a valid regular expression".to_string()
        ),
        (
          "filters.search.request".to_string(),
          format!("Search request filter '{huge}' is not a valid regular expression")
        ),
      ]
    );
  }

  #[test]
  fn unknown_block_scopes_are_refused() {
    let mut options = Options::default();
    options.blocking.default_scopes.push("downloads".into());
    assert!(
      problems(&options).is_empty(),
      "every scope may be a default"
    );
    options.blocking.default_scopes.push("everything".into());
    assert_eq!(
      problems(&options),
      [(
        "blocking.default_scopes".to_string(),
        "Block scope 'everything' must be one of: uploads, searches, browse, messages, rooms, \
         downloads, distributed"
          .to_string()
      )]
    );
  }
}
