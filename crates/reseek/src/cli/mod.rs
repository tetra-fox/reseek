//! The command line: the commands that run and exit, the two values read before the
//! configuration exists, and every other flag handed to the configuration as it was given.
//!
//! A flag's value follows it (`--http-port 5030`) or is attached with `=` (`--http-port=5030`);
//! a boolean flag takes no following value, so `--debug` is true and `--debug=false` false.
//! Short boolean flags may be grouped (`-nx`). A list flag is repeated for each element. A flag
//! no option has is passed on with its attached value, if any, so the configuration warns
//! about it.

mod commands;
mod help;

use std::ffi::OsString;
use std::path::PathBuf;

use cascade::{Kind, Schema};

pub use commands::{generate_cert, generate_secret, hash_password};
pub use help::{banner, envars, help};

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
  /// run the daemon
  Run(Invocation),
  /// print the version and exit
  Version,
  /// print the flags, the environment variables, or both, and exit
  Help {
    /// print the flags
    flags: bool,
    /// print the environment variables
    envars: bool,
    /// leave out the banner
    no_logo: bool,
  },
  /// write a self-signed certificate as PKCS#12 and exit
  GenerateCert {
    /// where to write it, `reseek.pfx` in the working directory when not given
    path: Option<PathBuf>,
  },
  /// print a random secret of this many bytes and exit
  GenerateSecret {
    /// how many random bytes
    length: usize,
  },
  /// read a password and print its argon2id hash
  HashPassword,
}

/// The command that prints a password's hash, given as the only argument.
pub const HASH_PASSWORD: &str = "hash-password";

/// What the daemon is started with.
#[derive(Debug, Default, PartialEq)]
pub struct Invocation {
  /// the application directory, from `--app-dir` or `RESEEK_APP_DIR`
  pub app_dir: Option<PathBuf>,
  /// the configuration files, from `--config`/`--config-file` or `RESEEK_CONFIG`, in the
  /// order they apply
  pub config_files: Vec<PathBuf>,
  /// every other flag as `(long flag, value)`, in the order given
  pub flags: Vec<(String, String)>,
}

/// A command line that can not be understood; printed as `Invalid command line input: ...`.
#[derive(Debug, PartialEq, thiserror::Error)]
#[error("Invalid command line input: {0}")]
pub struct UsageError(pub String);

/// The fewest and most bytes `--generate-secret` makes.
const SECRET_LENGTHS: std::ops::RangeInclusive<usize> = 16..=255;

/// A flag of the command line itself, not an option.
#[derive(Clone, Copy, PartialEq)]
enum Bootstrap {
  AppDir,
  Config,
  Version,
  Help,
  Envars,
  GenerateCert,
  GenerateSecret,
}

/// A flag of the command line itself: its short form, long form, the value it takes and help.
type BootstrapFlag = (
  Bootstrap,
  Option<char>,
  &'static str,
  Option<&'static str>,
  &'static str,
);

/// The command line's own flags.
const BOOTSTRAP: &[BootstrapFlag] = &[
  (
    Bootstrap::Version,
    Some('v'),
    "--version",
    None,
    "Prints the version and exits.",
  ),
  (
    Bootstrap::Help,
    Some('h'),
    "--help",
    None,
    "Prints the command-line flags and exits.",
  ),
  (
    Bootstrap::Envars,
    Some('e'),
    "--envars",
    None,
    "Prints the environment variables and exits.",
  ),
  (
    Bootstrap::GenerateCert,
    Some('g'),
    "--generate-cert",
    Some("path"),
    "Writes a self-signed certificate as PKCS#12, to reseek.pfx when no path is given, prints \
     its password and exits.",
  ),
  (
    Bootstrap::GenerateSecret,
    None,
    "--generate-secret",
    Some("int"),
    "Prints a random secret made of this many bytes, 16 to 255, and exits.",
  ),
  (
    Bootstrap::AppDir,
    Some('a'),
    "--app-dir",
    Some("path"),
    "Keeps the configuration, data, cache and logs under this one directory.",
  ),
  (
    Bootstrap::Config,
    Some('c'),
    "--config",
    Some("path"),
    "A configuration file, instead of config.toml in the configuration directory; repeat for \
     more files, later ones win.",
  ),
];

/// A second spelling of `--config`.
const CONFIG_FILE: &str = "--config-file";

/// One flag the parser recognised.
enum Flag<'a> {
  Bootstrap(Bootstrap, bool),
  Option(&'a cascade::Field),
}

/// Reads the command line (without the program name) and the bootstrap environment variables.
pub fn parse(
  args: impl IntoIterator<Item = OsString>,
  env: impl Fn(&str) -> Option<OsString>,
  schema: &Schema,
) -> Result<Command, UsageError> {
  let args: Vec<String> = args
    .into_iter()
    .map(|a| {
      a.into_string()
        .map_err(|a| UsageError(format!("`{}` is not UTF-8", a.to_string_lossy())))
    })
    .collect::<Result<_, _>>()?;
  if args.first().map(String::as_str) == Some(HASH_PASSWORD) {
    return match args.get(1) {
      Some(extra) => Err(UsageError(format!(
        "{HASH_PASSWORD} takes no arguments, not `{extra}`"
      ))),
      None => Ok(Command::HashPassword),
    };
  }
  let mut invocation = Invocation::default();
  let mut seen: Vec<(Bootstrap, Option<String>)> = Vec::new();
  let mut i = 0;
  while i < args.len() {
    let token = &args[i];
    i += 1;
    if token == "--" {
      if let Some(operand) = args.get(i) {
        return Err(UsageError(format!("unexpected argument `{operand}`")));
      }
      break;
    }
    if let Some(long) = token.strip_prefix("--") {
      let (name, attached) = match long.split_once('=') {
        Some((name, value)) => (format!("--{name}"), Some(value.to_string())),
        None => (token.clone(), None),
      };
      match lookup_long(&name, schema) {
        Some(flag) => {
          let next = (attached.is_none()).then(|| args.get(i)).flatten();
          let (value, consumed) = value_for(&flag, &name, attached, next)?;
          i += usize::from(consumed);
          record(flag, &name, value, &mut invocation, &mut seen);
        }
        None => invocation
          .flags
          .push((name, attached.unwrap_or_else(|| "true".to_string()))),
      }
      continue;
    }
    if let Some(shorts) = token.strip_prefix('-').filter(|s| !s.is_empty()) {
      let chars = shorts.char_indices();
      for (at, c) in chars {
        let display = format!("-{c}");
        let Some(flag) = lookup_short(c, schema) else {
          invocation.flags.push((display, "true".to_string()));
          continue;
        };
        if !takes_value(&flag) {
          record(
            flag,
            &display,
            "true".to_string(),
            &mut invocation,
            &mut seen,
          );
          continue;
        }
        let rest = &shorts[at + c.len_utf8()..];
        let attached =
          (!rest.is_empty()).then(|| rest.strip_prefix('=').unwrap_or(rest).to_string());
        let next = (attached.is_none()).then(|| args.get(i)).flatten();
        let (value, consumed) = value_for(&flag, &display, attached, next)?;
        i += usize::from(consumed);
        record(flag, &display, value, &mut invocation, &mut seen);
        break;
      }
      continue;
    }
    return Err(UsageError(format!(
      "unexpected argument `{token}`; a boolean flag takes its value attached, as in \
       `--debug=false`"
    )));
  }
  command(seen, invocation, env)
}

/// The command the recognised flags ask for, in the order version, help and environment
/// variables, certificate, secret, then running the daemon.
fn command(
  seen: Vec<(Bootstrap, Option<String>)>,
  mut invocation: Invocation,
  env: impl Fn(&str) -> Option<OsString>,
) -> Result<Command, UsageError> {
  let has = |b: Bootstrap| seen.iter().any(|(s, _)| *s == b);
  let value = |b: Bootstrap| {
    seen
      .iter()
      .rev()
      .find(|(s, _)| *s == b)
      .and_then(|(_, v)| v.clone())
  };
  if has(Bootstrap::Version) {
    return Ok(Command::Version);
  }
  if has(Bootstrap::Help) || has(Bootstrap::Envars) {
    let no_logo = invocation
      .flags
      .iter()
      .any(|(flag, value)| flag == "--no-logo" && value != "false");
    return Ok(Command::Help {
      flags: has(Bootstrap::Help),
      envars: has(Bootstrap::Envars),
      no_logo,
    });
  }
  if has(Bootstrap::GenerateCert) {
    return Ok(Command::GenerateCert {
      path: value(Bootstrap::GenerateCert).map(PathBuf::from),
    });
  }
  if has(Bootstrap::GenerateSecret) {
    let length = value(Bootstrap::GenerateSecret)
      .and_then(|v| v.parse::<usize>().ok())
      .filter(|n| SECRET_LENGTHS.contains(n))
      .ok_or_else(|| {
        UsageError("secret length must be between 16 and 255, inclusive".to_string())
      })?;
    return Ok(Command::GenerateSecret { length });
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

fn lookup_long<'a>(name: &str, schema: &'a Schema) -> Option<Flag<'a>> {
  if name == CONFIG_FILE {
    return Some(Flag::Bootstrap(Bootstrap::Config, true));
  }
  if let Some((b, _, _, value, _)) = BOOTSTRAP.iter().find(|(_, _, long, _, _)| *long == name) {
    return Some(Flag::Bootstrap(*b, value.is_some()));
  }
  schema
    .fields()
    .iter()
    .find(|f| f.cli.as_deref() == Some(name))
    .map(Flag::Option)
}

fn lookup_short(c: char, schema: &Schema) -> Option<Flag<'_>> {
  if let Some((b, _, _, value, _)) = BOOTSTRAP.iter().find(|(_, short, ..)| *short == Some(c)) {
    return Some(Flag::Bootstrap(*b, value.is_some()));
  }
  schema
    .fields()
    .iter()
    .find(|f| f.short == Some(c))
    .map(Flag::Option)
}

fn takes_value(flag: &Flag<'_>) -> bool {
  match flag {
    Flag::Bootstrap(_, value) => *value,
    Flag::Option(field) => field.kind != Kind::Bool,
  }
}

/// The value of `flag`: the attached one, else the next token when the flag takes one. Returns
/// whether the next token was used. `--generate-cert` takes the next token only when it is not
/// a flag.
fn value_for(
  flag: &Flag<'_>,
  shown: &str,
  attached: Option<String>,
  next: Option<&String>,
) -> Result<(String, bool), UsageError> {
  if let Some(value) = attached {
    return Ok((value, false));
  }
  if !takes_value(flag) {
    return Ok(("true".to_string(), false));
  }
  let optional = matches!(flag, Flag::Bootstrap(Bootstrap::GenerateCert, _));
  match next {
    Some(next) if !(optional && next.starts_with('-')) => Ok((next.clone(), true)),
    _ if optional => Ok((String::new(), false)),
    _ => Err(UsageError(format!("`{shown}` needs a value"))),
  }
}

fn record(
  flag: Flag<'_>,
  shown: &str,
  value: String,
  invocation: &mut Invocation,
  seen: &mut Vec<(Bootstrap, Option<String>)>,
) {
  match flag {
    Flag::Bootstrap(Bootstrap::AppDir, _) => invocation.app_dir = Some(PathBuf::from(value)),
    Flag::Bootstrap(Bootstrap::Config, _) => invocation.config_files.push(PathBuf::from(value)),
    Flag::Bootstrap(b, _) => seen.push((b, Some(value).filter(|v| !v.is_empty()))),
    Flag::Option(field) => {
      // a short flag is handed on by its long form, which the configuration knows it by
      let long = field.cli.clone().unwrap_or_else(|| shown.to_string());
      invocation.flags.push((long, value));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::Options;

  fn run(args: &[&str]) -> Result<Command, UsageError> {
    let schema = Schema::of::<Options>();
    parse(args.iter().map(OsString::from), |_| None, &schema)
  }

  fn flags(args: &[&str]) -> Vec<(String, String)> {
    match run(args).unwrap() {
      Command::Run(invocation) => invocation.flags,
      other => panic!("{other:?}"),
    }
  }

  fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
      .iter()
      .map(|(f, v)| (f.to_string(), v.to_string()))
      .collect()
  }

  #[test]
  fn values_follow_or_attach_and_booleans_take_none() {
    assert_eq!(
      flags(&["--http-port", "5030", "--debug", "-l=1", "--no-https=false"]),
      pairs(&[
        ("--http-port", "5030"),
        ("--debug", "true"),
        ("--http-port", "1"),
        ("--no-https", "false"),
      ])
    );
    assert_eq!(
      flags(&["-nx", "-l5031", "-s", "/a", "--shared", "/b"]),
      pairs(&[
        ("--no-logo", "true"),
        ("--no-start", "true"),
        ("--http-port", "5031"),
        ("--shared", "/a"),
        ("--shared", "/b"),
      ])
    );
  }

  #[test]
  fn unknown_flags_are_passed_on_and_stray_words_refused() {
    assert_eq!(
      flags(&["--htp-port=1", "-q"]),
      pairs(&[("--htp-port", "1"), ("-q", "true")])
    );
    assert!(
      run(&["--debug", "false"])
        .unwrap_err()
        .0
        .contains("`false`")
    );
    assert_eq!(
      run(&["--http-port"]).unwrap_err(),
      UsageError("`--http-port` needs a value".into())
    );
  }

  #[test]
  fn bootstrap_values_and_commands() {
    let Command::Run(invocation) =
      run(&["-a", "/app", "-c", "/a.toml", "--config-file", "/b.toml"]).unwrap()
    else {
      panic!()
    };
    assert_eq!(invocation.app_dir, Some(PathBuf::from("/app")));
    assert_eq!(
      invocation.config_files,
      [PathBuf::from("/a.toml"), PathBuf::from("/b.toml")]
    );
    assert_eq!(run(&["--help", "-v"]).unwrap(), Command::Version);
    assert_eq!(
      run(&["-h", "--envars", "-n"]).unwrap(),
      Command::Help {
        flags: true,
        envars: true,
        no_logo: true
      }
    );
    assert_eq!(run(&["-g"]).unwrap(), Command::GenerateCert { path: None });
    assert_eq!(
      run(&["--generate-cert", "out.pfx"]).unwrap(),
      Command::GenerateCert {
        path: Some("out.pfx".into())
      }
    );
    assert_eq!(
      run(&["--generate-secret", "32"]).unwrap(),
      Command::GenerateSecret { length: 32 }
    );
    for bad in [["--generate-secret", "15"], ["--generate-secret", "x"]] {
      assert_eq!(
        run(&bad).unwrap_err().to_string(),
        "Invalid command line input: secret length must be between 16 and 255, inclusive"
      );
    }
    assert_eq!(run(&["hash-password"]).unwrap(), Command::HashPassword);
    assert!(run(&["hash-password", "x"]).is_err());
    assert_eq!(
      flags(&["-k", "abc"]),
      pairs(&[("--api-key", "abc")]),
      "-k is the API key, not the secret generator"
    );
  }

  #[test]
  fn the_environment_gives_the_bootstrap_values_the_command_line_does_not() {
    let schema = Schema::of::<Options>();
    let env = |name: &str| match name {
      "RESEEK_APP_DIR" => Some(OsString::from("/env-app")),
      "RESEEK_CONFIG" => Some(OsString::from("/env.toml")),
      _ => None,
    };
    let Command::Run(invocation) =
      parse(["-a", "/cli-app"].map(OsString::from), env, &schema).unwrap()
    else {
      panic!()
    };
    assert_eq!(invocation.app_dir, Some(PathBuf::from("/cli-app")));
    assert_eq!(invocation.config_files, [PathBuf::from("/env.toml")]);
  }
}
