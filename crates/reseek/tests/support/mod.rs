//! Running the daemon program for a test: on a free loopback port, with its directory, and
//! stopped when the test is done with it.

#![allow(dead_code, reason = "each test binary uses a part of it")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The headless daemon program.
pub const PROGRAM: &str = env!("CARGO_BIN_EXE_reseek-headless");

/// A port nothing listens on, for a moment.
pub fn free_port() -> u16 {
  TcpListener::bind("127.0.0.1:0")
    .unwrap()
    .local_addr()
    .unwrap()
    .port()
}

/// The raw answer to a body-less request.
pub fn send(port: u16, method: &str, path: &str) -> Option<String> {
  let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
  write!(
    stream,
    "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
  )
  .ok()?;
  let mut answer = String::new();
  stream.read_to_string(&mut answer).ok()?;
  Some(answer)
}

/// The daemon running for a test.
pub struct Daemon {
  /// the process
  pub child: Child,
  /// the loopback port it serves HTTP on
  pub port: u16,
}

impl Daemon {
  /// Starts the daemon in `dir` on a free loopback port with `args` and only the variables in
  /// `env`, and waits until it answers `/health`.
  pub fn start(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    Self::start_on(free_port(), dir, args, env)
  }

  /// Starts the daemon as [`Daemon::start`] does, on `port`.
  pub fn start_on(port: u16, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
    let child = Command::new(PROGRAM)
      .args(["--app-dir", dir.to_str().unwrap(), "-n", "--no-https"])
      .args([
        "--http-ip-address",
        "127.0.0.1",
        "--http-port",
        &port.to_string(),
      ])
      .args(args)
      .env_clear()
      .envs(env.iter().copied())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    let daemon = Self { child, port };
    let started = Instant::now();
    while send(port, "GET", "/health").is_none() {
      assert!(
        started.elapsed() < Duration::from_secs(20),
        "never answered"
      );
      std::thread::sleep(Duration::from_millis(50));
    }
    daemon
  }

  /// The base URL to reach it at.
  pub fn url(&self) -> String {
    format!("http://127.0.0.1:{}", self.port)
  }

  /// Everything it printed, once it has exited.
  pub fn output(&mut self) -> String {
    let mut log = String::new();
    if let Some(mut stdout) = self.child.stdout.take() {
      stdout.read_to_string(&mut log).unwrap();
    }
    log
  }
}

impl Drop for Daemon {
  fn drop(&mut self) {
    // the daemon may have exited already, and then there is nothing to kill or reap
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}
