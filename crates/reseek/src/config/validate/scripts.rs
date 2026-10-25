//! The user's scripts: the events they run on and how each is started.

use api_types::events::EventType;

use super::Report;
use crate::config::{Integrations, Script};

const KEY: &str = "integrations.scripts";

/// Names in `on` that match every event.
const WILDCARDS: &[&str] = &["Any", "All", "*"];

pub(super) fn check(integrations: &Integrations, report: &mut Report) {
  for (name, script) in &integrations.scripts {
    check_script(name, script, report);
  }
}

fn check_script(name: &str, script: &Script, report: &mut Report) {
  let on = format!("{KEY}.{name}.on");
  if script.on.iter().any(|event| event.trim().is_empty()) {
    report.add(
      KEY,
      format!("The field {on} contains one or more null or empty values"),
    );
  }
  let names: Vec<&str> = WILDCARDS
    .iter()
    .copied()
    .chain(EventType::ALL.iter().map(|kind| kind.name()))
    .collect();
  let known = |event: &str| names.iter().any(|name| name.eq_ignore_ascii_case(event));
  if script
    .on
    .iter()
    .any(|event| !event.trim().is_empty() && !known(event))
  {
    report.add(
      KEY,
      format!(
        "The elements in the field {on} must all be one of: {}. Case insensitive.",
        names.join(", ")
      ),
    );
  }
  let set = |value: &Option<String>| value.as_deref().is_some_and(|v| !v.trim().is_empty());
  let run = &script.run;
  if set(&run.command) == set(&run.executable) {
    report.add(
      KEY,
      format!(
        "Script '{name}': One and only one of the fields Command or Executable may be specified \
         for a single script. If you intend to use the system shell, omit 'executable'. If you \
         intend to use an executable other than the system shell, omit 'command' and specify \
         either 'args' or 'arglist'."
      ),
    );
  }
  if set(&run.args) && run.arglist.is_some() {
    report.add(
      KEY,
      format!(
        "Script '{name}': Only one of the fields Args or Arglist may be specified for a single \
         script. Specify 'args' if you intend to construct a single quoted string yourself, and \
         specify 'arglist' if you'd like reseek to handle quoting for you."
      ),
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::ScriptRun;

  fn problems(script: Script) -> Vec<String> {
    let mut integrations = Integrations::default();
    integrations.scripts.insert("notify".into(), script);
    let mut report = Report::default();
    check(&integrations, &mut report);
    report.messages()
  }

  fn command(on: &[&str]) -> Script {
    Script {
      on: on.iter().map(|e| e.to_string()).collect(),
      run: ScriptRun {
        command: Some("echo hi".into()),
        ..ScriptRun::default()
      },
      ..Script::default()
    }
  }

  #[test]
  fn events_are_known_names_or_wildcards_in_any_case() {
    assert!(problems(command(&["downloadfilecomplete", "All", "any", "*"])).is_empty());
    assert_eq!(problems(command(&["**"])).len(), 1);
    assert!(problems(command(&[])).is_empty(), "never runs");
    let found = problems(command(&["DownloadFinished", ""]));
    assert_eq!(found.len(), 2);
    assert!(found[0].ends_with("contains one or more null or empty values"));
    assert!(found[1].contains("must all be one of: Any, All, *, DownloadFileComplete"));
    assert_eq!(problems(command(&["None"])).len(), 1, "None never matches");
  }

  #[test]
  fn a_script_runs_one_command_or_one_program() {
    let neither = Script::default();
    assert_eq!(problems(neither).len(), 1);
    let mut both = command(&[]);
    both.run.executable = Some("/bin/sh".into());
    assert_eq!(problems(both).len(), 1);
    let mut program = Script::default();
    program.run.executable = Some("/usr/bin/notify".into());
    program.run.args = Some("--quiet".into());
    program.run.arglist = Some(Vec::new());
    let found = problems(program);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("Only one of the fields Args or Arglist"));
  }
}
