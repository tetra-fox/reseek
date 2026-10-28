//! Relay mode: an agent's controller and its own name, a controller's agents, and the timeouts
//! and limits it holds them to. A debug instance is both, so both halves are checked for it.

use std::collections::BTreeMap;
use std::time::Duration;

use super::Report;
use super::fields::{http_url, length, required};
use crate::config::cidr::parse_trimmed_list;
use crate::config::enums::RelayMode;
use crate::config::{Options, Relay};

/// The characters an instance name may not hold, besides control characters and anything
/// outside printable ASCII: a controller names a file after each agent, on any system.
const NOT_IN_A_NAME: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

pub(super) fn check(options: &Options, report: &mut Report) {
  let relay = &options.relay;
  let agent = matches!(relay.mode, RelayMode::Agent | RelayMode::Debug);
  let controller = matches!(relay.mode, RelayMode::Controller | RelayMode::Debug);
  if relay.mode == RelayMode::Agent && options.instance_name == "local" {
    report.add(
      "instance_name",
      "Instance name must be something other than 'local' when operating in Relay Agent mode",
    );
  }
  if agent {
    if let Err(message) = file_name("instance_name", &options.instance_name) {
      report.add("instance_name", message);
    }
    check_controller(relay, report);
  }
  if controller {
    check_agents(relay, report);
  }
  let timeouts = &relay.controller_timeouts;
  for (key, value) in [
    ("relay.controller_timeouts.file_info", timeouts.file_info),
    ("relay.controller_timeouts.file_ack", timeouts.file_ack),
    (
      "relay.controller_timeouts.file_stream",
      timeouts.file_stream,
    ),
    (
      "relay.controller_timeouts.sync_deferral",
      timeouts.sync_deferral,
    ),
  ] {
    if value < Duration::from_millis(1) {
      report.add(key, format!("The field {key} must be longer than 0."));
    }
  }
  let key = "relay.controller.downloads_filter";
  for pattern in &relay.controller.downloads_filter {
    if let Err(e) = glob::Pattern::new(pattern) {
      report.add(key, format!("Pattern {pattern} is invalid: {e}"));
    }
  }
}

/// Whether `name` can name a file on any system: printable ASCII without separators or the
/// characters Windows refuses, and not `.` or `..`.
fn file_name(key: &str, name: &str) -> Result<(), String> {
  let unfit = name
    .chars()
    .any(|c| !(' '..='~').contains(&c) || NOT_IN_A_NAME.contains(&c));
  if unfit || name == "." || name == ".." {
    return Err(format!(
      "The field {key} must be printable ASCII without / \\ : * ? \" < > |, and not . or .."
    ));
  }
  Ok(())
}

fn check_controller(relay: &Relay, report: &mut Report) {
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
}

fn check_agents(relay: &Relay, report: &mut Report) {
  let key = "relay.agents";
  let mut names = BTreeMap::new();
  for (name, agent) in &relay.agents {
    let field = |f: &str| format!("{key}.{name}.{f}");
    let instance = agent.instance_name.as_deref();
    report.check(key, required(&field("instance_name"), instance));
    report.check(key, length(&field("instance_name"), instance, 1, 255));
    if let Some(instance) = instance.filter(|i| !i.is_empty()) {
      report.check(key, file_name(&field("instance_name"), instance));
    }
    report.check(key, required(&field("secret"), agent.secret.as_deref()));
    report.check(
      key,
      length(&field("secret"), agent.secret.as_deref(), 16, 255),
    );
    report.check(
      key,
      parse_trimmed_list(&agent.cidr)
        .map(drop)
        .map_err(|(element, e)| format!("CIDR {element} is invalid: {e}")),
    );
    if let Some(instance) = instance {
      *names.entry(instance).or_insert(0) += 1;
    }
  }
  if names.values().any(|&count| count > 1) {
    report.add(
      key,
      "One or more Agent instance names are duplicated.  Ensure instance names are unique.",
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::RelayAgent;

  fn problems(options: &Options) -> Vec<(String, String)> {
    let mut report = Report::default();
    check(options, &mut report);
    report.0.into_iter().map(|i| (i.key, i.message)).collect()
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
  fn a_debug_instance_is_checked_as_both_halves() {
    let mut options = Options::default();
    options.relay.mode = RelayMode::Debug;
    options
      .relay
      .agents
      .insert("a".into(), RelayAgent::default());
    let keys: Vec<String> = problems(&options).into_iter().map(|(k, _)| k).collect();
    assert!(
      keys.contains(&"relay.controller.address".to_string()),
      "{keys:?}"
    );
    assert!(keys.contains(&"relay.agents".to_string()), "{keys:?}");
    options.relay.mode = RelayMode::Controller;
    let keys: Vec<String> = problems(&options).into_iter().map(|(k, _)| k).collect();
    assert!(!keys.contains(&"relay.controller.address".to_string()));
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
        cidr: "10.0.0.0/8, ::ffff:0:0/96".into(),
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
  fn an_instance_name_must_be_able_to_name_a_file() {
    for name in ["nas", "the agent", "a.b-c_d"] {
      assert!(file_name("n", name).is_ok(), "{name}");
    }
    for name in [
      ".",
      "..",
      "a/b",
      "a\\b",
      "c:",
      "nul\0",
      "tab\there",
      "naïve",
      "a|b",
    ] {
      assert!(file_name("n", name).is_err(), "{name:?}");
    }
  }
}
