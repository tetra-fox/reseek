//! Downloading a file from a peer. We ask for it, by default with `QueueDownload`, and the peer
//! queues it; when the file reaches the front of the peer's queue the peer offers to send it, we
//! accept, and the peer connects to us (or we to it, when it cannot) with a transfer connection.
//! We say where to start, read the rest of the file and close the connection, which tells the
//! peer we have it all. The peer can refuse or fail the download at any point before that.

use std::io::SeekFrom;
use std::net::SocketAddr;

use soulseek_proto::peer::{QueueDownload, TransferRequest, TransferResponse};
use soulseek_proto::{PeerCode, PeerMessage, RemoteFilename, TransferDirection};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::io::{self, Pace};
use super::options::{
  DownloadOutput, DownloadRequest, DownloadSink, RequestStyle, StateChanged, TransferOptions,
};
use super::record::{Tracker, Transfer};
use super::size::resolve_size;
use super::state::TransferState;
use super::task::{self, Stop, TransferTask};
use super::{NewTransfer, Registration};
use crate::client::{Client, non_blank};
use crate::conn::{Conn, ConnHandle, MessageConn};
use crate::error::ClientError;
use crate::peers::write_peer;
use crate::waiter::WaitKey;

impl Client {
  /// Downloads a file and returns its final snapshot. The download runs on a task of its own:
  /// it stops when `cancel` fires, not when this future is dropped.
  pub async fn download(
    &self,
    request: DownloadRequest,
    cancel: &CancellationToken,
  ) -> Result<Transfer, ClientError> {
    TransferTask::running(self.start_download(request, cancel, None).await?).await
  }

  /// Starts a download and returns its task as soon as the peer has the request in its queue,
  /// so the caller can record that and await the rest separately. A download that fails before
  /// that reports its error here.
  pub async fn enqueue_download(
    &self,
    request: DownloadRequest,
    cancel: &CancellationToken,
  ) -> Result<TransferTask, ClientError> {
    let (observer, reached) = task::observer(TransferState::QUEUED_REMOTELY);
    let handle = self.start_download(request, cancel, Some(observer)).await?;
    task::enqueued(handle, reached).await
  }

  /// checks the request, takes the token and the file, and starts the download's task
  async fn start_download(
    &self,
    request: DownloadRequest,
    cancel: &CancellationToken,
    observer: Option<StateChanged>,
  ) -> Result<tokio::task::JoinHandle<Result<Transfer, ClientError>>, ClientError> {
    let DownloadRequest {
      username,
      filename,
      size,
      start_offset,
      token,
      output,
      mut options,
    } = request;
    non_blank("username", &username)?;
    non_blank("filename", filename.as_str())?;
    if let DownloadOutput::File(path) = &output {
      if path.as_os_str().is_empty() {
        return Err(ClientError::InvalidArgument(
          "The local filename must not be empty".into(),
        ));
      }
      // the file is closed whatever way the download ends
      options.dispose_output_stream_on_completion = true;
    }
    if start_offset > size {
      return Err(ClientError::InvalidArgument(format!(
        "The start offset {start_offset} is beyond the size {size}"
      )));
    }
    self.require_logged_in(&format!("download {filename} from {username}"))?;
    if let DownloadOutput::File(path) = &output {
      check_resumable(path, start_offset).await?;
    }
    let (registration, remote_failure) = self.transfers().register(
      self.tokens(),
      token,
      NewTransfer {
        direction: TransferDirection::Download,
        username: &username,
        filename: &filename,
        size,
        start_offset,
        cancel,
      },
    )?;
    let state_changed = observer
      .into_iter()
      .chain(options.state_changed.clone())
      .collect();
    let tracker = Tracker::new(
      self.clone(),
      registration.record.clone(),
      state_changed,
      options.progress_updated.clone(),
    );
    let token = tracker.snapshot().token;
    let span = tracing::info_span!("transfer", direction = "download", username, token, %filename);
    let download = Download {
      client: self.clone(),
      tracker,
      stop: Stop::new(registration.cancel.clone(), remote_failure),
      username,
      filename,
      token,
      start_offset,
      output: Some(output),
      options,
      _registration: registration,
    };
    Ok(tokio::spawn(download.run().instrument(span)))
  }
}

/// A resumed download needs the file to hold at least the bytes before the start offset; one
/// that holds fewer was changed by something else since the offset was taken from it
async fn check_resumable(path: &std::path::Path, start_offset: u64) -> Result<(), ClientError> {
  if start_offset == 0 {
    return Ok(());
  }
  let len = match tokio::fs::metadata(path).await {
    Ok(metadata) => metadata.len(),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
    Err(error) => {
      return Err(ClientError::io(
        format!("Failed to read the length of {}: {error}", path.display()),
        error,
      ));
    }
  };
  if len < start_offset {
    return Err(ClientError::InvalidArgument(
      "local file is shorter than the start offset".into(),
    ));
  }
  Ok(())
}

/// One download's task
struct Download {
  client: Client,
  tracker: Tracker,
  stop: Stop,
  username: String,
  filename: RemoteFilename,
  token: u32,
  start_offset: u64,
  output: Option<DownloadOutput>,
  options: TransferOptions,
  /// keeps the download tracked until the task ends
  _registration: Registration,
}

impl Download {
  async fn run(mut self) -> Result<Transfer, ClientError> {
    let result = self.transfer().await;
    let outcome = task::finish(&self.tracker, result);
    match &outcome {
      Ok(transfer) => tracing::info!(
        bytes = transfer.bytes_transferred,
        average_speed = transfer.average_speed,
        "download completed"
      ),
      Err(error) => tracing::info!(state = %self.tracker.state(), %error, "download ended"),
    }
    outcome
  }

  async fn transfer(&mut self) -> Result<(), ClientError> {
    self.tracker.emit_state(TransferState::QUEUED_LOCALLY, None);
    let limit = self.client.options().maximum_concurrent_downloads;
    let transfers = self.client.transfers().clone();
    let cancel = self.stop.cancel().clone();
    let _slot = self
      .stop
      .run(transfers.download_slots.acquire(limit, &cancel))
      .await?;
    let endpoint = self
      .stop
      .run(self.client.get_user_endpoint(&self.username))
      .await?;
    let conn = self.message_connection(endpoint).await?;
    // waited for before the request is written, so an offer that comes at once is not missed
    let offered = self.client.waiter().wait_within::<TransferRequest>(
      WaitKey::PeerFile {
        code: PeerCode::TransferRequest,
        username: self.username.clone(),
        filename: self.filename.clone(),
      },
      self.options.queued_timeout,
      cancel.clone(),
    );
    match self.options.request_style {
      RequestStyle::Queue => {
        let request = PeerMessage::QueueDownload(QueueDownload {
          filename: self.filename.clone(),
        });
        self.stop.run(write_peer(&conn, request)).await?;
        self.tracker.emit_state(TransferState::REQUESTED, None);
        // the peer does not answer this request; refusing it later is all it can do
        self.queued();
      }
      RequestStyle::TransferRequest => {
        if let Some(size) = self.ask(&conn).await? {
          // the peer sends at once, and expects us to connect with our own token
          self.tracker.emit_state(TransferState::INITIALIZING, None);
          let conn = self
            .stop
            .run(
              self
                .client
                .get_transfer_connection(&self.username, endpoint, self.token, &cancel),
            )
            .await?;
          return self.receive(conn, size).await;
        }
        self.queued();
      }
    }
    let offer = self.stop.run(offered).await?;
    let size = self.resolve_size(offer.file_size)?;
    self.tracker.set_remote_token(offer.token);
    self.tracker.emit_state(TransferState::INITIALIZING, None);
    tracing::info!(remote_token = offer.token, "download offered by the peer");
    let conn = self.accept(offer.token).await?;
    self.receive(conn, size).await
  }

  /// the peer has the request in its queue
  fn queued(&self) {
    self
      .tracker
      .emit_state(TransferState::QUEUED_REMOTELY, None);
    tracing::info!("download queued by the peer");
  }

  /// the cached message connection to the peer, or a new one
  async fn message_connection(&mut self, endpoint: SocketAddr) -> Result<MessageConn, ClientError> {
    let cancel = self.stop.cancel().clone();
    self
      .stop
      .run(
        self
          .client
          .message_connection(&self.username, Some(endpoint), None, &cancel),
      )
      .await
  }

  /// Asks with a download `TransferRequest` and waits for the answer: `Some(size)` when the peer
  /// sends at once, `None` when it queued the request
  async fn ask(&mut self, conn: &MessageConn) -> Result<Option<u64>, ClientError> {
    let answer = self.client.waiter().wait::<TransferResponse>(
      WaitKey::PeerToken {
        code: PeerCode::TransferResponse,
        username: self.username.clone(),
        token: self.token,
      },
      Some(self.client.options().transfer_response_timeout),
      self.stop.cancel().clone(),
    );
    let request = PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Download,
      token: self.token,
      filename: self.filename.clone(),
      file_size: None,
    });
    self.stop.run(write_peer(conn, request)).await?;
    self.tracker.emit_state(TransferState::REQUESTED, None);
    match self.stop.run(answer).await? {
      TransferResponse::Denied { message, .. } if is_queued(&message) => Ok(None),
      TransferResponse::Denied { message, .. } => Err(ClientError::TransferRejected(message)),
      TransferResponse::Allowed { file_size, .. } => {
        let size = self.resolve_size(file_size)?;
        // passed through so every download that starts was queued at the peer first
        self
          .tracker
          .emit_state(TransferState::QUEUED_REMOTELY, None);
        Ok(Some(size))
      }
    }
  }

  /// Accepts the peer's offer and waits for its transfer connection. A peer that cannot reach
  /// us gets a second chance: we connect to it with the token of its offer.
  async fn accept(&mut self, remote_token: u32) -> Result<Conn, ClientError> {
    // the user may have moved, and the message connection closed, during a long wait in the queue
    let endpoint = self
      .stop
      .run(self.client.get_user_endpoint(&self.username))
      .await?;
    let conn = self.message_connection(endpoint).await?;
    let cancel = self.stop.cancel().clone();
    let incoming = self.client.await_transfer_connection(
      &self.username,
      self.filename.clone(),
      remote_token,
      &cancel,
    );
    let accepted = PeerMessage::TransferResponse(TransferResponse::Allowed {
      token: remote_token,
      file_size: None,
    });
    self.stop.run(write_peer(&conn, accepted)).await?;
    let incoming = self.stop.run(async { Ok(incoming.await) }).await?;
    match incoming {
      Ok(conn) => Ok(conn),
      Err(error) => {
        tracing::debug!(%error, "the uploader did not connect; connecting to it instead");
        self
          .stop
          .run(
            self
              .client
              .get_transfer_connection(&self.username, endpoint, remote_token, &cancel),
          )
          .await
      }
    }
  }

  /// the size to download once the peer named one
  fn resolve_size(&self, remote: Option<u64>) -> Result<u64, ClientError> {
    let snapshot = self.tracker.snapshot();
    let size = resolve_size(
      &snapshot,
      remote,
      self.options.size_mismatch_resolver.as_ref(),
    )?;
    if size != snapshot.size {
      tracing::debug!(
        local = snapshot.size,
        ?remote,
        size,
        "download size settled"
      );
      self.tracker.set_size(size);
    }
    Ok(size)
  }

  /// Reads the file from the transfer connection, from the start offset on, and closes the
  /// connection once every byte is in
  async fn receive(&mut self, conn: Conn, size: u64) -> Result<(), ClientError> {
    let handle = conn.handle().clone();
    self.tracker.set_ip_end_point(handle.remote());
    let result = self.receive_on(conn, size, &handle).await;
    match &result {
      Ok(()) => handle.disconnect("Transfer complete", None),
      Err(ClientError::Cancelled) => handle.disconnect("Transfer cancelled", None),
      Err(error) => handle.disconnect(error.to_string(), Some(error.clone())),
    };
    result
  }

  async fn receive_on(
    &mut self,
    conn: Conn,
    size: u64,
    handle: &ConnHandle,
  ) -> Result<(), ClientError> {
    let (mut read, mut write) = conn.into_split()?;
    let mut sink = self.open_output().await?;
    let start = self.start_offset;
    let result = async {
      self
        .stop
        .run(write.write_all(&start.to_le_bytes()))
        .await
        .map_err(|error| io::connection_failure(handle, error))?;
      self.tracker.emit_state(TransferState::IN_PROGRESS, None);
      self.tracker.emit_progress(start);
      tracing::info!(size, start, "download started");
      let transfers = self.client.transfers().clone();
      let cancel = self.stop.cancel().clone();
      let pace = Pace {
        tracker: &self.tracker,
        governor: self.options.governor.clone(),
        reporter: self.options.reporter.clone(),
        bucket: &transfers.download_speed,
        cancel: &cancel,
      };
      let received = io::receive(&mut read, sink.as_mut(), size - start, start, &pace);
      self
        .stop
        .run(received)
        .await
        .map_err(|error| io::connection_failure(handle, error))
    }
    .await;
    if self.options.dispose_output_stream_on_completion
      && let Err(error) = sink.shutdown().await
    {
      tracing::warn!(%error, "the download's output did not close cleanly");
    }
    result
  }

  /// Opens where the bytes go, positioned at the start offset. A file keeps what is before the
  /// offset and loses what is after it.
  async fn open_output(&mut self) -> Result<Box<dyn DownloadSink>, ClientError> {
    let start = self.start_offset;
    let output = self
      .output
      .take()
      .expect("the output is opened once per download");
    let sink: Box<dyn DownloadSink> = match output {
      DownloadOutput::File(path) => {
        let file = tokio::fs::OpenOptions::new()
          .read(true)
          .write(true)
          .create(true)
          .truncate(false)
          .open(&path)
          .await
          .map_err(|error| {
            ClientError::io(format!("Failed to open {}: {error}", path.display()), error)
          })?;
        let len = file
          .metadata()
          .await
          .map_err(|error| {
            ClientError::io(
              format!("Failed to read the length of {}: {error}", path.display()),
              error,
            )
          })?
          .len();
        if len < start {
          return Err(ClientError::InvalidArgument(
            "local file is shorter than the start offset".into(),
          ));
        }
        file.set_len(start).await.map_err(|error| {
          ClientError::io(
            format!("Failed to truncate {}: {error}", path.display()),
            error,
          )
        })?;
        let mut file = file;
        file.seek(SeekFrom::Start(start)).await.map_err(|error| {
          ClientError::io(
            format!("Failed to seek in {}: {error}", path.display()),
            error,
          )
        })?;
        return Ok(Box::new(file));
      }
      DownloadOutput::Stream(open) => open()
        .await
        .map_err(|error| ClientError::io(format!("Failed to open the output: {error}"), error))?,
    };
    let mut sink = sink;
    if start > 0 && self.options.seek_output_stream_automatically {
      sink
        .seek(SeekFrom::Start(start))
        .await
        .map_err(|error| ClientError::Transfer {
          message: format!(
            "Requested non-zero start offset but output stream does not support seeking: {error}"
          ),
          source: Some(std::sync::Arc::new(error)),
        })?;
    }
    Ok(sink)
  }
}

/// whether a refusal is the peer saying it queued the request
fn is_queued(reason: &str) -> bool {
  reason.trim_end_matches('.').eq_ignore_ascii_case("Queued")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn queued_is_matched_without_case_or_trailing_dots() {
    assert!(is_queued("Queued"));
    assert!(is_queued("queued."));
    assert!(is_queued("QUEUED.."));
    assert!(!is_queued("Queued later"));
    assert!(!is_queued("File not shared."));
  }
}
