//! Copies of the controller's finished downloads. When the options ask for them, a download
//! the controller offers is fetched into the same path under the agent's downloads directory,
//! unless a filter leaves it out or a file of its length is there already. It is written beside
//! its place and renamed into it, so a half copy is never seen; up to three attempts are made.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use api_types::relay::{DownloadOutcome, RelayMessage};
use base64::Engine;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::AgentClient;
use crate::relay::credential::Purpose;
use crate::relay::frames;
use crate::safety::traverses;

/// How many times a download is fetched before it is given up.
const ATTEMPTS: u32 = 3;

/// The longest wait between two attempts.
const ATTEMPT_BACKOFF: Duration = Duration::from_secs(60);

/// The parts of the relative path `filename`, written with either separator; `None` when it is
/// empty, absolute, names a drive or steps outside.
fn parts(filename: &str) -> Option<Vec<&str>> {
  let parts: Vec<&str> = filename.split(['/', '\\']).collect();
  let safe = parts
    .iter()
    .all(|part| !part.is_empty() && *part != "." && *part != ".." && !part.contains(':'));
  safe.then_some(parts)
}

/// Whether `filters`, glob patterns on the path with `/` between its parts, let `filename`
/// through; every file passes no filter.
fn wanted(filters: &[String], filename: &[&str]) -> bool {
  let path = filename.join("/");
  filters.is_empty()
    || filters
      .iter()
      // loading the options checked every pattern
      .filter_map(|pattern| glob::Pattern::new(pattern).ok())
      .any(|pattern| pattern.matches(&path))
}

impl AgentClient {
  /// Fetches the finished download `filename`, offered with `token`, when the options ask
  /// for it.
  pub(super) async fn fetch_download(self: Arc<Self>, filename: String, token: Uuid) {
    if traverses(&filename) {
      return warn!(
        filename,
        "Suspicious attempt to solicit a file download containing unsafe path segments"
      );
    }
    let options = self.config.current();
    let controller = &options.values.relay.controller;
    if !controller.downloads {
      return debug!(filename, "finished download offered; fetching them is off");
    }
    info!(filename, %token, "Relay controller sent a download notification");
    let (outcome, detail) = match parts(&filename) {
      None => {
        warn!(
          filename,
          "finished download's path is not one under the downloads directory"
        );
        (
          DownloadOutcome::Failed,
          Some("the path is not relative".to_string()),
        )
      }
      Some(parts) if !wanted(&controller.downloads_filter, &parts) => {
        debug!(filename, "finished download left out by the filter");
        (
          DownloadOutcome::Skipped,
          Some("left out by the filter".to_string()),
        )
      }
      Some(parts) => {
        let mut destination = parts
          .iter()
          .fold(self.downloads.clone(), |path, part| path.join(part));
        if self.debug {
          // the controller is this daemon, whose downloads directory this is
          let mut named = destination.into_os_string();
          named.push(".relayed");
          destination = PathBuf::from(named);
        }
        self.fetch_attempts(&filename, token, &destination).await
      }
    };
    if let Some(link) = self.current() {
      link.send(Some(token), RelayMessage::DownloadDone { outcome, detail });
    }
  }

  /// Fetches the download into `destination`, trying again after a failure.
  async fn fetch_attempts(
    &self,
    filename: &str,
    token: Uuid,
    destination: &Path,
  ) -> (DownloadOutcome, Option<String>) {
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
      if attempt > 0 {
        tokio::select! {
          () = self.shutdown.cancelled() => break,
          () = tokio::time::sleep(frames::backoff(attempt, ATTEMPT_BACKOFF)) => {}
        }
      }
      let fetched = tokio::select! {
        () = self.shutdown.cancelled() => Err("the agent is stopping".to_string()),
        fetched = self.fetch_once(filename, token, destination) => fetched,
      };
      match fetched {
        Ok(true) => {
          info!(filename, destination = %destination.display(), "File successfully downloaded");
          return (DownloadOutcome::Stored, None);
        }
        Ok(false) => {
          debug!(filename, "a file of the same length is there already");
          return (DownloadOutcome::Skipped, Some("already there".to_string()));
        }
        Err(e) => {
          warn!(filename, %token, attempt = attempt + 1, error = e, "Failed to handle file download notification");
          last = e;
        }
      }
    }
    (DownloadOutcome::Failed, Some(last))
  }

  /// One attempt: true when the file was written, false when one of its length is there.
  async fn fetch_once(
    &self,
    filename: &str,
    token: Uuid,
    destination: &Path,
  ) -> Result<bool, String> {
    let link = self.current().ok_or("not connected to the controller")?;
    let settings = &link.settings;
    let response = settings
      .http
      .get(settings.url(&format!("relay/controller/downloads/{token}")))
      .header("x-api-key", &settings.api_key)
      .header("x-relay-agent", &settings.agent)
      .header(
        "x-relay-credential",
        settings
          .key
          .credential(Purpose::Download, &token.to_string()),
      )
      .header(
        "x-relay-filename-base64",
        base64::engine::general_purpose::STANDARD.encode(filename),
      )
      .send()
      .await
      .map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
      return Err(format!("the controller answered {status}"));
    }
    let length = response.content_length();
    if let (Some(length), Ok(there)) = (length, tokio::fs::metadata(destination).await)
      && there.len() == length
    {
      return Ok(false);
    }
    if let Some(parent) = destination.parent() {
      tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut partial = destination.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let written = write(response, &partial).await;
    if let Err(e) = &written {
      match tokio::fs::remove_file(&partial).await {
        Ok(()) => {}
        Err(removed) if removed.kind() == std::io::ErrorKind::NotFound => {}
        Err(removed) => {
          warn!(file = %partial.display(), error = %removed, "partial download not deleted")
        }
      }
      return Err(e.clone());
    }
    tokio::fs::rename(&partial, destination)
      .await
      .map_err(|e| format!("{}: {e}", destination.display()))?;
    Ok(true)
  }
}

/// Writes the body of `response` to `path` and flushes it to the disk.
async fn write(response: reqwest::Response, path: &Path) -> Result<(), String> {
  let failed = |e: std::io::Error| format!("{}: {e}", path.display());
  let mut file = tokio::fs::File::create(path).await.map_err(failed)?;
  let mut body = response.bytes_stream();
  while let Some(chunk) = body.next().await {
    let chunk = chunk.map_err(|e| e.to_string())?;
    file.write_all(&chunk).await.map_err(failed)?;
  }
  file.sync_all().await.map_err(failed)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_path_is_taken_apart_only_when_it_stays_inside() {
    assert_eq!(
      parts("alice\\Album/01.flac"),
      Some(vec!["alice", "Album", "01.flac"])
    );
    for unsafe_path in ["", "/etc/passwd", "a//b", "a/../b", "C:\\x", "./a"] {
      assert_eq!(parts(unsafe_path), None, "{unsafe_path:?}");
    }
  }

  #[test]
  fn a_filter_matches_the_path_with_forward_slashes() {
    let path = ["alice", "Album", "01.flac"];
    assert!(wanted(&[], &path));
    assert!(wanted(&["alice/**".to_string()], &path));
    assert!(wanted(
      &["*.mp3".to_string(), "**/*.flac".to_string()],
      &path
    ));
    assert!(!wanted(&["bob/**".to_string()], &path));
  }
}
