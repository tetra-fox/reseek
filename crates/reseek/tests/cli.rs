//! The program from the outside: the commands, the exit codes, and a start that serves
//! `/health` and stops cleanly on SIGTERM.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = env!("CARGO_BIN_EXE_reseek-headless");

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

/// A port nothing listens on, for a moment.
fn free_port() -> u16 {
  TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap()
    .port()
}

fn get(port: u16, path: &str) -> Option<String> {
  let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
  write!(
    stream,
    "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
  )
  .ok()?;
  let mut answer = String::new();
  stream.read_to_string(&mut answer).ok()?;
  Some(answer)
}

#[cfg(unix)]
#[test]
fn the_daemon_serves_health_and_stops_on_sigterm() {
  let dir = tempfile::tempdir().unwrap();
  let port = free_port();
  let mut child = Command::new(PROGRAM)
    .args([
      "--app-dir",
      dir.path().to_str().unwrap(),
      "-n",
      "--no-https",
    ])
    .args([
      "--http-ip-address",
      "127.0.0.1",
      "--http-port",
      &port.to_string(),
    ])
    .env_clear()
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  let started = Instant::now();
  let answer = loop {
    if let Some(answer) = get(port, "/health") {
      break answer;
    }
    assert!(
      started.elapsed() < Duration::from_secs(20),
      "never answered"
    );
    std::thread::sleep(Duration::from_millis(50));
  };
  assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
  assert!(answer.ends_with("Healthy"), "{answer}");
  let unauthenticated = get(port, "/api/v0/options/view").unwrap();
  assert!(
    unauthenticated.starts_with("HTTP/1.1 401"),
    "{unauthenticated}"
  );
  Command::new("kill")
    .args(["-TERM", &child.id().to_string()])
    .status()
    .unwrap();
  let status = child.wait().unwrap();
  let mut log = String::new();
  child
    .stdout
    .take()
    .unwrap()
    .read_to_string(&mut log)
    .unwrap();
  assert_eq!(status.code(), Some(0), "{log}");
  assert!(log.contains("Listening for HTTP requests"), "{log}");
  assert!(
    logs_written(&dir.path().join("logs")),
    "a log file is written"
  );
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
