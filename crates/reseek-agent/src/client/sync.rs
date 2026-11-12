//! Uploading the agent's shares: the share list and a copy of the local index go to the
//! controller with a token it hands out on the socket. The token is asked for first, with the
//! index's size, so an index the controller does not take is never copied. One upload runs at
//! a time; one asked for meanwhile runs once after it.
//!
//! While the startup load or a first scan fills the index, it is not uploaded: until the
//! first upload on a socket the controller is told every minute that the shares come after the
//! scan, and the end of the load or the scan asks for the upload. During a scan over a filled
//! index, a socket with no upload yet gets that index at once; one that has keeps it until the
//! scan ends. With no load or scan running the index is uploaded as it is.
//!
//! An upload stops when its socket closes, or when the controller takes none of the index for
//! thirty seconds. The first upload on a socket failing closes the socket to try again from
//! the start, and one the controller refuses for good backs off as a refused login does. A
//! later one failing is tried again after a wait that grows, until one succeeds.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use api_types::relay::{RelayErrorCode, RelayMessage};
use api_types::shares::Share;
use futures_util::StreamExt;
use reqwest::multipart::{Form, Part};
use reseek_relay_proto::credential::Purpose;
use reseek_relay_proto::frames;
use reseek_shares::{Shares, backoff, lock};
use tokio_util::io::ReaderStream;
use tracing::{Instrument, debug, info, warn};
use uuid::Uuid;

use super::session::Link;
use super::{AgentClient, warn_once};

/// How many times the agent asks for an upload token.
const TOKEN_ATTEMPTS: u32 = 3;

/// How long it waits for the controller's answer each time.
const TOKEN_WAIT: Duration = Duration::from_secs(10);

/// The longest wait between two asks.
const TOKEN_BACKOFF: Duration = Duration::from_secs(5);

/// How long an upload may go without the controller taking any of the index.
const STALL: Duration = Duration::from_secs(30);

/// The longest wait before a failed upload on a socket that had one is tried again.
const RETRY_CAP: Duration = Duration::from_secs(300);

/// Why an upload failed.
#[derive(Debug, thiserror::Error)]
enum SyncError {
  /// the controller does not take this index: its schema is another, or it is too large
  #[error("{0}")]
  Refused(String),
  /// the socket closed, which ends what was asked on it
  #[error("the socket closed")]
  Closed,
  /// anything else, which may pass
  #[error("{0}")]
  Failed(String),
}

impl AgentClient {
  /// Uploads the shares now, or once the upload running ends; nothing while not logged in, and
  /// nothing yet while [`AgentClient::defers`] says so.
  pub(super) fn request_sync(&self) {
    let Some(link) = self.current().filter(|link| link.logged_in()) else {
      return debug!("not logged in to the controller; the shares are uploaded at the next login");
    };
    if self.defers(&link) {
      self.defer_while_scanning();
      return debug!("the shares are being loaded or scanned; they are uploaded when that ends");
    }
    if self.syncing.swap(true, Ordering::AcqRel) {
      self.again.store(true, Ordering::Release);
      return;
    }
    let agent = self.this();
    tokio::spawn(async move { agent.sync_while_asked().await }.in_current_span());
  }

  /// Whether the upload waits for the load or the scan running to end: one that fills the
  /// index for the first time always, one over a filled index once `link` had an upload.
  /// Outside a load or scan the index is uploaded as it is, filled or not.
  fn defers(&self, link: &Link) -> bool {
    self.shares.loading_or_scanning() && (!self.shares.index_filled() || link.ready())
  }

  /// Tells the controller the shares come once the load or scan running ends, while none were
  /// sent on this socket and none is being sent, so it waits for them.
  pub(super) fn defer_while_scanning(&self) {
    let Some(link) = self.current() else {
      return;
    };
    let uploading = self.syncing.load(Ordering::Acquire);
    if link.logged_in() && !link.ready() && !uploading && self.defers(&link) {
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
      // a load or scan that began since it was asked for asks again when it ends
      if let Some(link) = self.current().filter(|link| link.logged_in())
        && !self.defers(&link)
      {
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
    let e = match self.upload_shares(link).await {
      Ok(()) => {
        link.synced();
        // a later run of failures is warned about again
        *lock(&self.sync_warned) = None;
        return info!("Shares uploaded. Ready to relay files.");
      }
      Err(SyncError::Closed) => return debug!("share upload stopped: the socket closed"),
      Err(e) => e,
    };
    if let (true, SyncError::Refused(reason)) = (first, &e) {
      // warned about once as the socket ends, however often the agent dials again
      link.refused(format!("shares refused: {reason}"));
      return link.close(frames::close::NORMAL);
    }
    // a run of failures is warned about once, across the sockets a failed first upload closes
    warn_once(
      &self.sync_warned,
      &e.to_string(),
      "Failed to upload shares to relay controller",
    );
    if first {
      return link.close(frames::close::NORMAL);
    }
    let failures = link.sync_failed();
    let delay = backoff::wait(failures, backoff::AT_ONCE, RETRY_CAP);
    debug!(failures, ?delay, "share upload tried again after a wait");
    let (agent, link) = (self.this(), link.clone());
    tokio::spawn(
      async move {
        tokio::select! {
          () = link.closed() => {}
          () = tokio::time::sleep(delay) => {
            // an upload since, asked for by a change, succeeded or failed on its own
            if link.sync_failures() == failures {
              agent.request_sync();
            }
          }
        }
      }
      .in_current_span(),
    );
  }

  /// Asks for a token with the index's size, then copies the local index to a file, uploads it
  /// and deletes it; the ask and the upload stop when `link` closes. One upload runs at a time,
  /// so the copy has one name, and a copy a stopped program left is written over.
  async fn upload_shares(&self, link: &Link) -> Result<(), SyncError> {
    let failed = |what: &str, e: reseek_shares::db::DbError| {
      SyncError::Failed(format!("the share index was not {what}: {e}"))
    };
    let size = self
      .shares
      .index_bytes()
      .await
      .map_err(|e| failed("measured", e))?;
    let shares = self.shares.local_shares();
    let token = tokio::select! {
      () = link.closed() => Err(SyncError::Closed),
      token = ask_token(link, &shares, size) => token,
    }?;
    let dump = self.uploads.join("share_index.db");
    // the copy runs to its end even when the socket closes meanwhile, so it is deleted below
    // and not written after
    let uploaded = match self.shares.dump(&dump).await {
      Ok(bytes) => {
        info!(bytes, "Beginning upload of shares");
        tokio::select! {
          () = link.closed() => Err(SyncError::Closed),
          posted = post_dump(link, token, &shares, &dump, bytes) => posted,
        }
      }
      Err(e) => Err(failed("copied", e)),
    };
    match tokio::fs::remove_file(&dump).await {
      Ok(()) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
      Err(e) => warn!(file = %dump.display(), error = %e, "copy of the share index not deleted"),
    }
    uploaded
  }
}

/// Asks the controller on `link` for a token to upload `shares` and an index of about `size`
/// bytes with.
async fn ask_token(link: &Link, shares: &[Share], size: u64) -> Result<Uuid, SyncError> {
  let (files, directories) = shares.iter().fold((0u32, 0u32), |(f, d), share| {
    (
      f.saturating_add(share.files.unwrap_or(0)),
      d.saturating_add(share.directories.unwrap_or(0)),
    )
  });
  for attempt in 0..TOKEN_ATTEMPTS {
    if attempt > 0 {
      tokio::time::sleep(backoff::wait(attempt, backoff::AT_ONCE, TOKEN_BACKOFF)).await;
    }
    let asked = RelayMessage::SharesBegin {
      schema_version: Shares::schema_version(),
      shares: shares.to_vec(),
      file_count: files,
      directory_count: directories,
      database_bytes: size,
    };
    match link.ask_shares(asked, TOKEN_WAIT).await {
      Some(RelayMessage::SharesToken { token, .. }) => return Ok(token),
      Some(RelayMessage::Error {
        code: code @ (RelayErrorCode::SchemaMismatch | RelayErrorCode::TooLarge),
        detail,
        ..
      }) => {
        return Err(SyncError::Refused(format!(
          "{code:?}: {}",
          detail.unwrap_or_default()
        )));
      }
      Some(RelayMessage::Error { code, detail, .. }) if code != RelayErrorCode::SyncInProgress => {
        return Err(SyncError::Failed(format!(
          "{code:?}: {}",
          detail.unwrap_or_default()
        )));
      }
      answer => debug!(attempt, ?answer, "no share upload token yet"),
    }
  }
  Err(SyncError::Failed("no share upload token".to_string()))
}

/// Posts `shares` and the copy `dump` of `bytes` with `token`; fails when the controller takes
/// none of the copy for [`STALL`].
async fn post_dump(
  link: &Link,
  token: Uuid,
  shares: &[Share],
  dump: &Path,
  bytes: u64,
) -> Result<(), SyncError> {
  let failed = |e: String| SyncError::Failed(e);
  let file = tokio::fs::File::open(dump)
    .await
    .map_err(|e| failed(format!("{}: {e}", dump.display())))?;
  let list = serde_json::to_string(shares).map_err(|e| failed(e.to_string()))?;
  let taken = Arc::new(AtomicU64::new(0));
  let counted = taken.clone();
  let read = ReaderStream::new(file).inspect(move |chunk| {
    if let Ok(chunk) = chunk {
      counted.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
  });
  let database =
    Part::stream_with_length(reqwest::Body::wrap_stream(read), bytes).file_name("shares");
  let form = Form::new().text("shares", list).part("database", database);
  let settings = &link.settings;
  let started = Instant::now();
  let post = async {
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
    if status.is_success() {
      return Ok(());
    }
    // the status says what failed; a body that does not arrive only says less
    let body = response.text().await.unwrap_or_default();
    Err(failed(format!(
      "the controller answered the upload {status} {body}"
    )))
  };
  tokio::select! {
    posted = post => posted?,
    () = stalled(&taken, bytes) => {
      return Err(failed(format!("the controller took none of the share index for {STALL:?}")));
    }
  }
  info!(
    bytes,
    ms = started.elapsed().as_millis() as u64,
    "Upload of shares succeeded"
  );
  Ok(())
}

/// Returns once `taken`, the bytes of an upload of `total` read so far, stayed the same for
/// [`STALL`] short of the end; never once all were read, while the controller loads them.
async fn stalled(taken: &AtomicU64, total: u64) {
  let mut seen = taken.load(Ordering::Relaxed);
  loop {
    tokio::time::sleep(STALL).await;
    let now = taken.load(Ordering::Relaxed);
    if now == seen && now < total {
      return;
    }
    seen = now;
  }
}
