//! The controller's requests for the agent's files: whether one exists and how long it is, and
//! its bytes for a peer's download. A file is looked up among the local shares only and read
//! from disk; one the index has but the disk does not asks for a scan of the shares, and the
//! controller is told so. The bytes go in an HTTP call that stays open while the peer reads
//! them; the call ends with the daemon, which fails the transfer on the controller.

use std::io::SeekFrom;
use std::sync::Arc;

use api_types::relay::{FileFailure, RelayFileUploadResult, RelayMessage};
use reqwest::multipart::{Form, Part};
use tokio::io::AsyncSeekExt;
use tokio_util::io::ReaderStream;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::AgentClient;
use super::session::Link;
use crate::relay::credential::Purpose;
use crate::safety::traverses;

/// Why a file cannot be sent, as the controller is told.
struct Refusal {
  reason: FileFailure,
  status: Option<u16>,
  detail: Option<String>,
}

impl Refusal {
  fn new(reason: FileFailure, detail: impl Into<String>) -> Self {
    Self {
      reason,
      status: None,
      detail: Some(detail.into()),
    }
  }
}

impl AgentClient {
  /// Answers the file info request `id` for `filename`.
  pub(super) async fn answer_file_info(&self, link: &Link, id: Uuid, filename: String) {
    let (exists, length) = match self.on_disk(link, &filename).await {
      Ok((_, length)) => (true, length),
      Err(refusal) => {
        debug!(
          filename,
          detail = refusal.detail,
          "file info answered as missing"
        );
        (false, 0)
      }
    };
    link.send(Some(id), RelayMessage::FileInfo { exists, length });
  }

  /// Where the shared `filename` is on disk and how long it is. A name with a `.` or `..`
  /// part is refused; a file the index has and the disk does not asks for a scan.
  async fn on_disk(
    &self,
    link: &Link,
    filename: &str,
  ) -> Result<(std::path::PathBuf, u64), Refusal> {
    if traverses(filename) {
      warn!(
        filename,
        "Suspicious request for a file containing unsafe path segments"
      );
      return Err(Refusal::new(
        FileFailure::NotFound,
        "the name has unsafe path segments",
      ));
    }
    let resolved = match self.shares.resolve_local(filename).await {
      Ok(Some(resolved)) => resolved,
      Ok(None) => return Err(Refusal::new(FileFailure::NotFound, "not shared")),
      Err(e) => {
        return Err(Refusal::new(
          FileFailure::Error,
          format!("the share index was not read: {e}"),
        ));
      }
    };
    match tokio::fs::metadata(&resolved.path).await {
      Ok(metadata) if metadata.is_file() => Ok((resolved.path, metadata.len())),
      _ => {
        let detail = format!(
          "The file '{}' could not be located on disk. A share scan should be performed.",
          resolved.path.display()
        );
        warn!(path = %resolved.path.display(), "The shared file could not be located on disk. A share scan should be performed");
        if let Err(e) = self.shares.request_scan().await {
          warn!(error = %e, "share scan not requested");
        }
        link.send(
          None,
          RelayMessage::RescanRequested {
            reason: detail.clone(),
          },
        );
        Err(Refusal::new(FileFailure::NotFound, detail))
      }
    }
  }

  /// Sends `filename` from `start` for the upload `transfer`, as the controller asked with
  /// `token`.
  pub(super) async fn send_file(
    self: Arc<Self>,
    token: Uuid,
    filename: String,
    start: u64,
    transfer: Uuid,
  ) {
    info!(filename, %token, %transfer, "Relay controller requested file");
    let Some(link) = self.current() else {
      return debug!(filename, "the socket closed before the file was sent");
    };
    let outcome = tokio::select! {
      () = self.shutdown.cancelled() => Err(Refusal::new(FileFailure::Cancelled, "the agent is stopping")),
      outcome = self.upload(&link, token, &filename, start) => outcome,
    };
    match outcome {
      Ok(result) => info!(
        filename,
        %token,
        outcome = ?result.outcome,
        bytes = result.bytes,
        "Upload of file succeeded."
      ),
      Err(refusal) => {
        warn!(
          filename,
          %token,
          reason = ?refusal.reason,
          status = refusal.status,
          detail = refusal.detail,
          "Upload of file failed"
        );
        // the socket the request came on may have been replaced since
        let link = self.current().unwrap_or(link);
        link.send(
          Some(token),
          RelayMessage::FileFailed {
            reason: refusal.reason,
            status: refusal.status,
            detail: refusal.detail,
          },
        );
      }
    }
  }

  /// Opens the file at `start`, acknowledges the request and posts its bytes.
  async fn upload(
    &self,
    link: &Link,
    token: Uuid,
    filename: &str,
    start: u64,
  ) -> Result<RelayFileUploadResult, Refusal> {
    let (path, length) = self.on_disk(link, filename).await?;
    let mut file = tokio::fs::File::open(&path)
      .await
      .map_err(|e| Refusal::new(FileFailure::Unreadable, e.to_string()))?;
    file
      .seek(SeekFrom::Start(start))
      .await
      .map_err(|e| Refusal::new(FileFailure::Unreadable, e.to_string()))?;
    link.send(Some(token), RelayMessage::FileAck {});
    info!(filename, %token, "Beginning upload of file");
    let body = reqwest::Body::wrap_stream(ReaderStream::new(file));
    let part =
      Part::stream_with_length(body, length.saturating_sub(start)).file_name(filename.to_string());
    let settings = &link.settings;
    let response = settings
      .http
      .post(settings.url(&format!("relay/controller/files/{token}")))
      .header("x-api-key", &settings.api_key)
      .header("x-relay-agent", &settings.agent)
      .header(
        "x-relay-credential",
        settings.key.credential(Purpose::File, &token.to_string()),
      )
      .multipart(Form::new().part("file", part))
      .send()
      .await
      .map_err(|e| Refusal::new(FileFailure::Error, e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
      return Err(Refusal {
        reason: FileFailure::Rejected,
        status: Some(status.as_u16()),
        detail: Some(format!("the controller answered {status}")),
      });
    }
    let body = response.bytes().await.map_err(|e| {
      Refusal::new(
        FileFailure::Error,
        format!("the controller's answer did not arrive: {e}"),
      )
    })?;
    serde_json::from_slice(&body).map_err(|e| {
      Refusal::new(
        FileFailure::Error,
        format!("the controller's answer did not read: {e}"),
      )
    })
  }
}
