//! The soulseek-rs CLI as another user on a test server: it shares a directory while it serves,
//! and searches and downloads with one-shot commands. The client's interop test and the daemon's
//! smoke run both drive it; it depends on nothing else in the harness, so the smoke program can
//! carry it too.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};

/// One user of the CLI, with its own config, state, share and download directories.
pub struct Peer {
  /// the CLI
  pub binary: PathBuf,
  /// the server it logs in to
  pub server: SocketAddr,
  /// its account
  pub username: String,
  /// the account's password
  pub password: String,
  /// the port it takes peers' connections on
  pub listen_port: u16,
  /// the directory everything of this user's lives under
  pub root: PathBuf,
}

impl Peer {
  /// The directory it shares.
  pub fn share(&self) -> PathBuf {
    self.root.join("share")
  }

  /// The directory its downloads land in.
  pub fn downloads(&self) -> PathBuf {
    self.root.join("downloads")
  }

  /// The CLI with this user's flags and directories, then `args`.
  pub fn command(&self, args: &[&str]) -> Command {
    let mut command = Command::new(&self.binary);
    command
      .env("SOULSEEK_CONFIG_DIR", self.root.join("config"))
      .env("SOULSEEK_STATE_DIR", self.root.join("state"))
      .args(["--no-daemon", "--no-config", "--quiet"])
      .args(["--server", &self.server.to_string()])
      .args(["--username", &self.username, "--password", &self.password])
      .args(["--listener-port", &self.listen_port.to_string()])
      .arg("--shared-dir")
      .arg(self.share())
      .arg("--download-dir")
      .arg(self.downloads())
      .args(args)
      .stdin(Stdio::null());
    command
  }

  /// Runs a one-shot command to its end, failing the test when it fails.
  pub async fn run(&self, args: &[&str]) -> Output {
    let mut command = self.command(args);
    let output = tokio::task::spawn_blocking(move || command.output())
      .await
      .unwrap()
      .expect("soulseek-rs runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
      output.status.success(),
      "soulseek-rs {args:?} failed with {}: {stderr}",
      output.status
    );
    output
  }

  /// Stays online sharing until the returned guard is dropped.
  pub fn serve(&self) -> Serving {
    let child = self
      .command(&["serve", "--duration", "600"])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .expect("soulseek-rs runs");
    Serving(child)
  }
}

/// A running `serve`, stopped when dropped.
pub struct Serving(Child);

impl Drop for Serving {
  fn drop(&mut self) {
    // it may have exited already; it is gone either way
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}
