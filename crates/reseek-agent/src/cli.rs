//! The agent's command line: its own flags (help, version, the application directory and the
//! configuration files) and the flags of its options, each as the daemon spells it.

use std::ffi::OsString;
use std::path::PathBuf;

use cascade::{Field, Kind, Schema};

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
  /// print the flags and exit
  Help,
  /// print the version and exit
  Version,
  /// run the agent
  Run(Invocation),
}

/// What the agent is started with.
#[derive(Debug, Default, PartialEq)]
pub struct Invocation {
  /// the application directory, from `--app-dir` or `RESEEK_APP_DIR`
  pub app_dir: Option<PathBuf>,
  /// the configuration files, from `--config` or `RESEEK_CONFIG`, in the order they apply
  pub config_files: Vec<PathBuf>,
  /// the options' flags as `(long flag, value)`, in the order given
  pub flags: Vec<(String, String)>,
}

/// The agent's own flags with the value each takes and its help.
const OWN: &[(&str, &str, Option<&str>, &str)] = &[
  (
    "-h",
    "--help",
    None,
    "Prints the command-line flags and exits.",
  ),
  ("-v", "--version", None, "Prints the version and exits."),
  (
    "-a",
    "--app-dir",
    Some("path"),
    "Directory that holds the configuration, data and cache.",
  ),
  (
    "-c",
    "--config",
    Some("path"),
    "Configuration file to use instead of config.toml in the configuration directory. Repeat \
     for more files. Later files take precedence.",
  ),
];

/// Reads the command line, without the program name, and the variables `env` gives.
pub fn parse(
  args: Vec<OsString>,
  env: impl Fn(&str) -> Option<OsString>,
  schema: &Schema,
) -> Result<Command, String> {
  let args = args
    .into_iter()
    .map(|arg| {
      arg
        .into_string()
        .map_err(|arg| format!("`{}` is not UTF-8", arg.to_string_lossy()))
    })
    .collect::<Result<Vec<_>, _>>()?;
  let mut invocation = Invocation::default();
  let mut args = args.into_iter();
  while let Some(arg) = args.next() {
    let (flag, attached) = match arg.split_once('=') {
      Some((flag, value)) if flag.starts_with("--") => (flag.to_string(), Some(value.to_string())),
      _ => (arg.clone(), None),
    };
    let mut value = |takes: bool| -> Result<String, String> {
      match (attached.clone(), takes) {
        (Some(value), _) => Ok(value),
        (None, false) => Ok("true".to_string()),
        (None, true) => args.next().ok_or_else(|| format!("{flag} needs a value")),
      }
    };
    match flag.as_str() {
      "-h" | "--help" => return Ok(Command::Help),
      "-v" | "--version" => return Ok(Command::Version),
      "-a" | "--app-dir" => invocation.app_dir = Some(PathBuf::from(value(true)?)),
      "-c" | "--config" => invocation.config_files.push(PathBuf::from(value(true)?)),
      _ => {
        let field = option(schema, &flag).ok_or_else(|| format!("unknown flag `{flag}`"))?;
        let long = field
          .cli
          .clone()
          .expect("a field found by its flag has one");
        let given = value(field.kind != Kind::Bool)?;
        invocation.flags.push((long, given));
      }
    }
  }
  if invocation.app_dir.is_none() {
    invocation.app_dir = env("RESEEK_APP_DIR").map(PathBuf::from);
  }
  if invocation.config_files.is_empty() {
    invocation.config_files = env("RESEEK_CONFIG")
      .map(PathBuf::from)
      .into_iter()
      .collect();
  }
  Ok(Command::Run(invocation))
}

/// The option the flag `flag`, long or short, sets.
fn option<'a>(schema: &'a Schema, flag: &str) -> Option<&'a Field> {
  let short = flag
    .strip_prefix('-')
    .filter(|rest| !rest.starts_with('-'))
    .and_then(|rest| {
      let mut chars = rest.chars();
      chars.next().filter(|_| chars.next().is_none())
    });
  schema.fields().iter().find(|field| match short {
    Some(c) => field.short == Some(c),
    None => field.cli.as_deref() == Some(flag),
  })
}

/// The help: the agent's own flags, then each option's flag with its environment variable.
pub fn help(schema: &Schema) -> String {
  let mut text = format!(
    "reseek-agent {}: shares this machine's files through a reseek daemon, its relay \
     controller.\n\nUsage: reseek-agent [flags]\n\n",
    crate::VERSION
  );
  for (short, long, takes, doc) in OWN {
    let value = takes.map(|v| format!(" <{v}>")).unwrap_or_default();
    text.push_str(&format!("  {short}, {long}{value}\n      {doc}\n"));
  }
  for field in schema.fields() {
    let Some(long) = &field.cli else {
      continue;
    };
    let short = field.short.map(|c| format!("-{c}, ")).unwrap_or_default();
    let value = if field.kind == Kind::Bool {
      String::new()
    } else {
      format!(" <{}>", field.key)
    };
    let env = field
      .env
      .as_ref()
      .map(|env| format!(" [{env}]"))
      .unwrap_or_default();
    let doc = field.doc.replace('\n', " ");
    text.push_str(&format!("  {short}{long}{value}{env}\n      {doc}\n"));
  }
  text
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::options::Options;

  fn parsed(args: &[&str]) -> Result<Command, String> {
    let env = |name: &str| (name == "RESEEK_CONFIG").then(|| OsString::from("/env.toml"));
    parse(
      args.iter().map(OsString::from).collect(),
      env,
      &Schema::of::<Options>(),
    )
  }

  #[test]
  fn flags_are_read_long_short_and_attached() {
    let Command::Run(invocation) = parsed(&[
      "-a",
      "/app",
      "-d",
      "--instance-name=nas",
      "-s",
      "/music",
      "--controller-address",
      "https://c:5031",
    ])
    .unwrap() else {
      panic!("not a run");
    };
    assert_eq!(invocation.app_dir, Some(PathBuf::from("/app")));
    assert_eq!(invocation.config_files, [PathBuf::from("/env.toml")]);
    assert_eq!(
      invocation.flags,
      [
        ("--debug".to_string(), "true".to_string()),
        ("--instance-name".to_string(), "nas".to_string()),
        ("--shared".to_string(), "/music".to_string()),
        (
          "--controller-address".to_string(),
          "https://c:5031".to_string()
        ),
      ]
    );
    assert_eq!(parsed(&["-h"]).unwrap(), Command::Help);
    assert!(
      parsed(&["--http-port", "1"])
        .unwrap_err()
        .contains("--http-port")
    );
    assert!(parsed(&["--instance-name"]).is_err(), "a value is missing");
  }
}
