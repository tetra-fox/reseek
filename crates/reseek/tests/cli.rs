//! The program from the outside: the commands, the exit codes, a start that serves `/health`
//! and stops cleanly on SIGTERM, and a restart asked for over HTTP.

mod support;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use support::{Daemon, PROGRAM, send};

/// The program run with `args` and only the variables in `env`.
fn run(args: &[&str], env: &[(&str, &str)]) -> Output {
  Command::new(PROGRAM)
    .args(args)
    .env_clear()
    .envs(env.iter().copied())
    .output()
    .unwrap()
}

fn text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn version_and_help_print_and_exit_zero() {
  let version = run(&["--version"], &[]);
  assert_eq!(version.status.code(), Some(0));
  assert!(text(&version.stdout).starts_with(env!("CARGO_PKG_VERSION")));
  let help = run(&["-h", "-n"], &[]);
  assert_eq!(help.status.code(), Some(0));
  let help = text(&help.stdout);
  assert!(help.starts_with("\nusage: reseek [arguments]"), "{help}");
  assert!(help.contains("-l|--http-port <int>"));
  let envars = run(&["--envars", "--no-logo"], &[]);
  assert!(text(&envars.stdout).contains("RESEEK_HTTP_PORT <int>"));
}

/// The one-shot commands into a pipe whose reader closed before they wrote, as `| head -c1`
/// leaves it: they stop quietly with 0.
#[test]
fn one_shot_output_into_a_closed_pipe_exits_zero() {
  for args in [
    &["--help"][..],
    &["man"],
    &["completions", "zsh"],
    &["--version"],
  ] {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let output = Command::new(PROGRAM)
      .args(args)
      .env_clear()
      .stdout(writer)
      .stderr(Stdio::piped())
      .output()
      .unwrap();
    assert_eq!(
      output.status.code(),
      Some(0),
      "{args:?}: {}",
      text(&output.stderr)
    );
    assert!(
      output.stderr.is_empty(),
      "{args:?}: {}",
      text(&output.stderr)
    );
  }
}

#[test]
fn usage_errors_exit_two() {
  for args in [
    &["--debug", "false"][..],
    &["--generate-secret", "15"],
    &["--http-port"],
  ] {
    let output = run(args, &[]);
    assert_eq!(output.status.code(), Some(2), "{args:?}");
    assert!(text(&output.stderr).starts_with("Invalid command line input: "));
  }
}

#[test]
fn secrets_and_certificates_are_generated() {
  let secret = run(&["--generate-secret", "32"], &[]);
  assert_eq!(secret.status.code(), Some(0));
  let secret = text(&secret.stdout);
  assert!(secret.trim().len() >= 40, "{secret}");
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("out.pfx");
  let cert = run(&["--generate-cert", path.to_str().unwrap()], &[]);
  assert_eq!(cert.status.code(), Some(0), "{}", text(&cert.stderr));
  assert!(text(&cert.stdout).contains("Password: "));
  assert!(path.exists());
}

#[test]
fn hash_password_prints_an_argon2id_hash_of_stdin() {
  let mut child = Command::new(PROGRAM)
    .arg("hash-password")
    .env_clear()
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  child.stdin.take().unwrap().write_all(b"hunter2\n").unwrap();
  let output = child.wait_with_output().unwrap();
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  let hash = text(&output.stdout);
  assert!(hash.starts_with("$argon2id$v=19$"), "{hash}");
  assert!(!hash.contains("hunter2"));
}

#[test]
fn an_invalid_configuration_exits_three_and_a_held_lock_five() {
  let dir = tempfile::tempdir().unwrap();
  let app_dir = dir.path().to_str().unwrap();
  let invalid = run(
    &["--app-dir", app_dir, "--no-logo"],
    &[("RESEEK_SLSK_LISTEN_PORT", "80")],
  );
  assert_eq!(invalid.status.code(), Some(3), "{}", text(&invalid.stderr));
  assert!(text(&invalid.stderr).contains("soulseek.listen_port"));
  let blank = run(
    &["--app-dir", app_dir, "--no-logo"],
    &[("RESEEK_HTTP_IP_ADDRESS", "")],
  );
  assert_eq!(
    blank.status.code(),
    Some(3),
    "an empty variable is set, not absent"
  );
  assert!(text(&blank.stderr).contains("web.ip_address"));

  let start = run(
    &["--app-dir", app_dir, "-n", "--no-start", "--no-https"],
    &[],
  );
  assert_eq!(start.status.code(), Some(0), "{}", text(&start.stdout));
  assert!(
    dir.path().join("config.toml").exists(),
    "the template is written"
  );
  let lock = std::fs::File::open(dir.path().join("data/reseek.lock")).unwrap();
  lock.try_lock().unwrap();
  let locked = run(
    &["--app-dir", app_dir, "-n", "--no-start", "--no-https"],
    &[],
  );
  assert_eq!(locked.status.code(), Some(5), "{}", text(&locked.stdout));
}

fn get(port: u16, path: &str) -> Option<String> {
  send(port, "GET", path)
}

#[cfg(unix)]
#[test]
fn the_daemon_serves_health_and_stops_on_sigterm() {
  let dir = tempfile::tempdir().unwrap();
  let mut daemon = Daemon::start(dir.path(), &[], &[]);
  let answer = get(daemon.port, "/health").unwrap();
  assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
  assert!(answer.ends_with("Healthy"), "{answer}");
  let unauthenticated = get(daemon.port, "/api/v0/options/view").unwrap();
  assert!(
    unauthenticated.starts_with("HTTP/1.1 401"),
    "{unauthenticated}"
  );
  Command::new("kill")
    .args(["-TERM", &daemon.child.id().to_string()])
    .status()
    .unwrap();
  let status = daemon.child.wait().unwrap();
  let log = daemon.output();
  assert_eq!(status.code(), Some(0), "{log}");
  assert!(log.contains("Listening for HTTP requests"), "{log}");
  assert!(
    logs_written(&dir.path().join("logs")),
    "a log file is written"
  );
}

#[test]
fn a_restart_exits_75_for_the_supervisor() {
  let dir = tempfile::tempdir().unwrap();
  let trusted = [("RESEEK_TRUSTED_NETWORKS", "127.0.0.0/8")];
  let mut daemon = Daemon::start(dir.path(), &["--supervised"], &trusted);
  let answer = send(daemon.port, "PUT", "/api/v0/application").unwrap();
  assert!(answer.starts_with("HTTP/1.1 204"), "{answer}");
  let status = daemon.child.wait().unwrap();
  let log = daemon.output();
  assert_eq!(status.code(), Some(75), "{log}");
  assert!(log.contains("restart asked for"), "{log}");
}

fn logs_written(dir: &Path) -> bool {
  std::fs::read_dir(dir)
    .map(|entries| {
      entries
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("reseek-"))
    })
    .unwrap_or(false)
}
