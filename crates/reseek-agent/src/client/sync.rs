//! Uploading the agent's shares: a copy of the local index and the share list go to the
//! controller with a token it hands out on the socket. One upload runs at a time; one asked
//! for meanwhile runs once after it. While the local shares are being scanned the upload
//! waits, and the scan's end asks for it again, so the controller never gets half an index.
//! Until the first upload on a socket, the controller is told every minute that the scan runs,
//! so it waits for the shares. The first upload on a socket failing closes the socket to try
//! again from the start; a later one failing leaves the last upload in place until the next
//! change.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use api_types::relay::{RelayErrorCode, RelayMessage};
use reqwest::multipart::{Form, Part};
use reseek_relay_proto::credential::Purpose;
use reseek_relay_proto::frames;
use reseek_shares::{Shares, backoff};
use tokio_util::io::ReaderStream;
use tracing::{Instrument, debug, info, warn};
use uuid::Uuid;

use super::AgentClient;
use super::session::Link;

/// How many times the agent asks for an upload token.
const TOKEN_ATTEMPTS: u32 = 3;

/// How long it waits for the controller's answer each time.
const TOKEN_WAIT: Duration = Duration::from_secs(10);

/// The longest wait between two asks.
const TOKEN_BACKOFF: Duration = Duration::from_secs(5);

/// Why an upload failed.
#[derive(Debug, thiserror::Error)]
enum SyncError {
  /// the controller's share index has another schema
  #[error("the controller's share index has another schema: {0}")]
  SchemaMismatch(String),
  /// anything else
  #[error("{0}")]
  Failed(String),
}

impl AgentClient {
  /// Uploads the shares now, or once the upload running ends; nothing while not logged in or
  /// while the local shares are being scanned.
  pub(super) fn request_sync(&self) {
    if !self.current().is_some_and(|link| link.logged_in()) {
      return debug!("not logged in to the controller; the shares are uploaded at the next login");
    }
    if self.shares.scanning() {
      self.defer_while_scanning();
      return debug!("the shares are being scanned; they are uploaded when the scan ends");
    }
    if self.syncing.swap(true, Ordering::AcqRel) {
      self.again.store(true, Ordering::Release);
      return;
    }
    let agent = self.this();
    tokio::spawn(async move { agent.sync_while_asked().await }.in_current_span());
  }

  /// Tells the controller the shares come once the scan running ends, while none were sent on
  /// this socket, so it waits for them.
  pub(super) fn defer_while_scanning(&self) {
    let Some(link) = self.current() else {
      return;
    };
    if link.logged_in() && !link.ready() && self.shares.scanning() {
      debug!("telling the controller the shares come after the scan");
      link.send(
        None,
        RelayMessage::SyncDeferred {
          reason: "scanning".to_string(),
        },
      );
    }
  }

  /// Uploads until no further upload was asked for.
  async fn sync_while_asked(self: Arc<Self>) {
    loop {
      self.again.store(false, Ordering::Release);
      if let Some(link) = self.current().filter(|link| link.logged_in()) {
        self.sync_once(&link).await;
      }
      if self.again.load(Ordering::Acquire) {
        continue;
      }
      self.syncing.store(false, Ordering::Release);
      // asked for between the check and the release, and nobody else took it
      if !self.again.load(Ordering::Acquire) || self.syncing.swap(true, Ordering::AcqRel) {
        return;
      }
    }
  }

  /// One upload on `link`, its outcome put into the state.
  async fn sync_once(&self, link: &Arc<Link>) {
    let first = !link.ready();
    match self.upload_shares(link).await {
      Ok(()) => {
        link.set_ready();
        info!("Shares uploaded. Ready to relay files.");
      }
      Err(e) => {
        warn!(error = %e, "Failed to upload shares to relay controller");
        if let SyncError::SchemaMismatch(_) = e {
          link.refused(e.to_string());
        }
        if first {
          link.close(frames::close::NORMAL);
        }
      }
    }
  }

  /// Copies the local index to a file, uploads it and deletes it. One upload runs at a time,
  /// so the copy has one name, and a copy an interrupted upload left is written over.
  async fn upload_shares(&self, link: &Link) -> Result<(), SyncError> {
    let dump = self.uploads.join("share_index.db");
    let uploaded = match self.shares.dump(&dump).await {
      Ok(bytes) => {
        info!(bytes, "Beginning upload of shares");
        self.send_dump(link, &dump, bytes).await
      }
      Err(e) => Err(SyncError::Failed(format!(
        "the share index was not copied: {e}"
      ))),
    };
    match tokio::fs::remove_file(&dump).await {
      Ok(()) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
      Err(e) => warn!(file = %dump.display(), error = %e, "copy of the share index not deleted"),
    }
    uploaded
  }

  /// Asks for a token and uploads the copy `dump` of `bytes` with the share list.
  async fn send_dump(
    &self,
    link: &Link,
    dump: &std::path::Path,
    bytes: u64,
  ) -> Result<(), SyncError> {
    let shares = self.shares.local_shares();
    let (files, directories) = shares.iter().fold((0u32, 0u32), |(f, d), share| {
      (
        f.saturating_add(share.files.unwrap_or(0)),
        d.saturating_add(share.directories.unwrap_or(0)),
      )
    });
    let mut token = None;
    for attempt in 0..TOKEN_ATTEMPTS {
      if attempt > 0 {
        tokio::time::sleep(backoff::wait(attempt, backoff::AT_ONCE, TOKEN_BACKOFF)).await;
      }
      let asked = RelayMessage::SharesBegin {
        schema_version: Shares::schema_version(),
        shares: shares.clone(),
        file_count: files,
        directory_count: directories,
        database_bytes: bytes,
      };
      match link.ask_shares(asked, TOKEN_WAIT).await {
        Some(RelayMessage::SharesToken { token: given, .. }) => {
          token = Some(given);
          break;
        }
        Some(RelayMessage::Error {
          code: RelayErrorCode::SchemaMismatch,
          detail,
          ..
        }) => return Err(SyncError::SchemaMismatch(detail.unwrap_or_default())),
        Some(RelayMessage::Error { code, detail, .. })
          if code != RelayErrorCode::SyncInProgress =>
        {
          return Err(SyncError::Failed(format!(
            "{code:?}: {}",
            detail.unwrap_or_default()
          )));
        }
        answer => debug!(attempt, ?answer, "no share upload token yet"),
      }
    }
    let token = token.ok_or_else(|| SyncError::Failed("no share upload token".to_string()))?;
    self.post_dump(link, token, &shares, dump, bytes).await
  }

  /// Posts the share list and the copy `dump` with `token`.
  async fn post_dump(
    &self,
    link: &Link,
    token: Uuid,
    shares: &[api_types::shares::Share],
    dump: &std::path::Path,
    bytes: u64,
  ) -> Result<(), SyncError> {
    let failed = |e: String| SyncError::Failed(e);
    let file = tokio::fs::File::open(dump)
      .await
      .map_err(|e| failed(format!("{}: {e}", dump.display())))?;
    let list = serde_json::to_string(shares).map_err(|e| failed(e.to_string()))?;
    let database =
      Part::stream_with_length(reqwest::Body::wrap_stream(ReaderStream::new(file)), bytes)
        .file_name("shares");
    let form = Form::new().text("shares", list).part("database", database);
    let settings = &link.settings;
    let started = Instant::now();
    let response = settings
      .http
      .post(settings.url(&format!("relay/controller/shares/{token}")))
      .header("x-api-key", &settings.api_key)
      .header("x-relay-agent", &settings.agent)
      .header(
        "x-relay-credential",
        settings.key.credential(Purpose::Shares, &token.to_string()),
      )
      .multipart(form)
      .send()
      .await
      .map_err(|e| failed(e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
      // the status says what failed; a body that does not arrive only says less
      let body = response.text().await.unwrap_or_default();
      return Err(failed(format!(
        "Failed to upload shares to relay controller: {status} {body}"
      )));
    }
    info!(
      bytes,
      ms = started.elapsed().as_millis() as u64,
      "Upload of shares succeeded"
    );
    Ok(())
  }
}
