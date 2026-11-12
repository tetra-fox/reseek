//! Uploading a file to a peer that asked for it. An upload first waits its turn: one at a time per
//! user, then the application's slot policy, then the bound on uploads at once. We then offer the
//! file to the peer; once it accepts we open the transfer connection, the peer says where to
//! start, and we write the rest of the file. The peer closes the connection when it has every
//! byte; we close it ourselves only after a grace period. An upload that does not succeed tells
//! the peer, which queues the file again unless we canceled it.

use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::peer::{TransferRequest, TransferResponse, UploadDenied, UploadFailed};
use soulseek_proto::{PeerCode, PeerMessage, RemoteFilename, TransferDirection};
use tokio::io::AsyncSeekExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::io::{self, Linger, Pace};
use super::options::{StateChanged, TransferOptions, UploadInput, UploadRequest, UploadSource};
use super::record::{Tracker, Transfer};
use super::state::TransferState;
use super::task::{self, Stop, TransferTask};
use super::{NewTransfer, Registration};
use crate::client::{Client, non_blank};
use crate::conn::{Conn, ConnHandle};
use crate::error::{ClientError, ErrorSource, MessageErrorKind};
use crate::limit::Permit;
use crate::peers::write_peer;
use crate::state::ClientState;
use crate::waiter::WaitKey;

/// how long telling the peer about an upload that did not succeed may take
const COURTESY_BUDGET: Duration = Duration::from_secs(5);

impl Client {
  /// Uploads a file and returns its final snapshot. The upload runs on a task of its own: it
  /// stops when `cancel` fires, not when this future is dropped.
  pub async fn upload(
    &self,
    request: UploadRequest,
    cancel: &CancellationToken,
  ) -> Result<Transfer, ClientError> {
    TransferTask::running(self.start_upload(request, cancel, None).await?).await
  }

  /// Starts an upload and returns its task once it is queued here, which is at once; an upload
  /// that fails before that reports its error here.
  pub async fn enqueue_upload(
    &self,
    request: UploadRequest,
    cancel: &CancellationToken,
  ) -> Result<TransferTask, ClientError> {
    let (observer, reached) = task::observer(TransferState::QueuedLocally);
    let handle = self.start_upload(request, cancel, Some(observer)).await?;
    task::enqueued(handle, reached).await
  }

  /// checks the request, takes the token and the file, and starts the upload's task
  async fn start_upload(
    &self,
    request: UploadRequest,
    cancel: &CancellationToken,
    observer: Option<StateChanged>,
  ) -> Result<tokio::task::JoinHandle<Result<Transfer, ClientError>>, ClientError> {
    let UploadRequest {
      username,
      filename,
      input,
      token,
      options,
    } = request;
    non_blank("username", &username)?;
    non_blank("filename", filename.as_str())?;
    if let UploadInput::File(path) = &input {
      if path.as_os_str().is_empty() {
        return Err(ClientError::InvalidArgument(
          "The local filename must not be empty".into(),
        ));
      }
      let exists = tokio::fs::try_exists(path).await.map_err(|error| {
        ClientError::io(
          format!("Failed to look for {}: {error}", path.display()),
          error,
        )
      })?;
      if !exists {
        return Err(ClientError::NotFound(path.display().to_string()));
      }
    }
    self.require_logged_in(&format!("upload {filename} to {username}"))?;
    let size = match &input {
      UploadInput::File(path) => file_size(path).await?,
      UploadInput::Stream { size, .. } => *size,
    };
    let (registration, _) = self.transfers().register(
      self.tokens(),
      token,
      NewTransfer {
        direction: TransferDirection::Upload,
        username: &username,
        filename: &filename,
        size,
        start_offset: 0,
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
    let span = tracing::info_span!("transfer", direction = "upload", username, token, %filename);
    let upload = Upload {
      client: self.clone(),
      turn: self.transfers().upload_turn(&username),
      tracker,
      stop: Stop::new(registration.cancel.clone(), None),
      username,
      filename,
      token,
      size,
      input: Some(input),
      options,
      registration: Some(registration),
    };
    Ok(tokio::spawn(upload.run().instrument(span)))
  }
}

/// Measures a file to upload, opening it the way the upload will, so a file that cannot be read
/// fails now
async fn file_size(path: &std::path::Path) -> Result<u64, ClientError> {
  let unreadable = |error: std::io::Error| {
    ClientError::io(
      format!(
        "The local file {} could not be opened for reading: {error}",
        path.display()
      ),
      error,
    )
  };
  let file = tokio::fs::File::open(path).await.map_err(unreadable)?;
  Ok(file.metadata().await.map_err(unreadable)?.len())
}

/// One upload's task
struct Upload {
  client: Client,
  tracker: Tracker,
  stop: Stop,
  username: String,
  filename: RemoteFilename,
  token: u32,
  size: u64,
  input: Option<UploadInput>,
  options: TransferOptions,
  /// this user's one upload at a time
  turn: Arc<Semaphore>,
  /// keeps the upload tracked until its cleanup lets go of it
  registration: Option<Registration>,
}

/// What an upload waited for and holds until its cleanup gives it back, in order
#[derive(Default)]
struct Held {
  turn: Option<OwnedSemaphorePermit>,
  /// the application's slot policy admitted the upload
  slot: bool,
  running: Option<Permit>,
}

impl Upload {
  async fn run(mut self) -> Result<Transfer, ClientError> {
    let mut held = Held::default();
    let result = self.transfer(&mut held).await;
    let outcome = task::finish(&self.tracker, result);
    match &outcome {
      Ok(transfer) => tracing::info!(
        bytes = transfer.bytes_transferred,
        average_speed = transfer.average_speed,
        "upload completed"
      ),
      Err(error) => tracing::info!(state = %self.tracker.state(), %error, "upload ended"),
    }
    self.clean_up(held, &outcome).await;
    outcome
  }

  async fn transfer(&mut self, held: &mut Held) -> Result<(), ClientError> {
    self.tracker.emit_first_state();
    let turn = self.turn.clone();
    held.turn = Some(
      self
        .stop
        .run(async {
          Ok(
            turn
              .acquire_owned()
              .await
              .expect("an upload turn is never closed"),
          )
        })
        .await?,
    );
    self.await_slot().await?;
    held.slot = true;
    let limit = self.client.options().maximum_concurrent_uploads;
    let transfers = self.client.transfers().clone();
    let cancel = self.stop.cancel().clone();
    held.running = Some(
      self
        .stop
        .run(transfers.upload_slots.acquire(limit, &cancel))
        .await?,
    );
    let endpoint = self
      .stop
      .run(self.client.get_user_endpoint(&self.username))
      .await?;
    let conn = self
      .stop
      .run(
        self
          .client
          .message_connection(&self.username, Some(endpoint), &cancel),
      )
      .await?;
    let answer = self.client.waiter().wait::<TransferResponse>(
      WaitKey::PeerToken {
        code: PeerCode::TransferResponse,
        username: self.username.clone(),
        token: self.token,
      },
      Some(self.client.options().transfer_response_timeout),
      cancel.clone(),
    );
    let offer = PeerMessage::TransferRequest(TransferRequest {
      direction: TransferDirection::Upload,
      token: self.token,
      filename: self.filename.clone(),
      file_size: Some(self.size),
    });
    self.stop.run(write_peer(&conn, offer)).await?;
    self.tracker.emit_state(TransferState::Requested, None)?;
    match self.stop.run(answer).await? {
      // a downloader may not queue us, so its "Queued" is a refusal too
      TransferResponse::Denied { message, .. } => {
        return Err(ClientError::TransferRejected(message));
      }
      TransferResponse::Allowed { .. } => {}
    }
    self.tracker.emit_state(TransferState::Initializing, None)?;
    let conn = self
      .stop
      .run(
        self
          .client
          .get_transfer_connection(&self.username, endpoint, self.token, &cancel),
      )
      .await?;
    self.send(conn).await
  }

  /// waits for the application's slot policy to admit the upload
  async fn await_slot(&mut self) -> Result<(), ClientError> {
    let Some(awaiter) = self.options.slot_awaiter.clone() else {
      return Ok(());
    };
    let transfer = self.tracker.snapshot();
    let slot = awaiter(&transfer, self.stop.cancel().clone());
    match self.stop.run(slot).await {
      Ok(()) => Ok(()),
      Err(ClientError::Canceled) => Err(ClientError::Canceled),
      Err(error) => Err(ClientError::Transfer {
        message: format!(
          "Failed to acquire an upload slot for {} to {}: {error}",
          self.filename, self.username
        ),
        source: Some(Arc::new(error) as ErrorSource),
      }),
    }
  }

  /// Reads where the downloader wants to start, writes the rest of the file and waits for the
  /// downloader to close the connection
  async fn send(&mut self, conn: Conn) -> Result<(), ClientError> {
    let handle = conn.handle().clone();
    self.tracker.set_ip_end_point(handle.remote());
    let result = self.send_on(conn, &handle).await;
    // the downloader or the linger may have closed it already; the first reason given stands
    match &result {
      Ok(()) => handle.disconnect("Transfer complete", None),
      Err(ClientError::Canceled) => handle.disconnect("Transfer canceled", None),
      Err(error) => handle.disconnect(error.to_string(), Some(error.clone())),
    };
    result
  }

  async fn send_on(&mut self, conn: Conn, handle: &ConnHandle) -> Result<(), ClientError> {
    let (mut read, mut write) = conn.into_split()?;
    let mut offset = [0; 8];
    self.stop.run(read.read_exact(&mut offset)).await.map_err(
      |error| match io::connection_failure(handle, error) {
        error @ (ClientError::Timeout(_) | ClientError::Canceled) => error,
        error => ClientError::Message {
          kind: MessageErrorKind::Read,
          message: "Failed to read transfer start offset".into(),
          source: Some(Arc::new(error)),
        },
      },
    )?;
    let start = self.check_offset(u64::from_le_bytes(offset))?;
    self.tracker.set_start_offset(start);
    let mut source = self.open_input(start).await?;
    self.tracker.emit_state(TransferState::InProgress, None)?;
    self.tracker.emit_progress(start);
    tracing::info!(size = self.size, start, "upload started");
    let transfers = self.client.transfers().clone();
    let cancel = self.stop.cancel().clone();
    let pace = Pace {
      tracker: &self.tracker,
      governor: self.options.governor.clone(),
      reporter: self.options.reporter.clone(),
      bucket: &transfers.upload_speed,
      cancel: &cancel,
    };
    // a peer that already has the whole file asks from its end, gets nothing more and closes
    let len = self.size - start;
    let sent = async {
      if len == 0 {
        return Ok(());
      }
      tokio::select! {
        // the last chunk written counts even when the peer closes right after reading it
        biased;
        sent = io::send(&mut write, source.as_mut(), len, start, &pace) => sent,
        closed = io::closed(&mut read) => Err(closed),
      }
    };
    self
      .stop
      .run(sent)
      .await
      .map_err(|error| io::connection_failure(handle, error))?;
    drop(source);
    match io::linger(&mut read, self.options.maximum_linger_time, &cancel).await {
      Linger::Closed | Linger::Forced => {}
      // every byte was written, so the upload counts as delivered
      Linger::Canceled => tracing::debug!("canceled after the last byte; the upload stands"),
    }
    Ok(())
  }

  /// The start offset the downloader asked for, unless it is past the end of the file. Soulseek
  /// NS sends the largest offset there is when it resumes a file over 2 GiB; that is refused the
  /// same way, logged once per peer, and the peer queues the file again.
  fn check_offset(&self, start: u64) -> Result<u64, ClientError> {
    if start == u64::MAX
      && self
        .client
        .transfers()
        .first_bad_offset_from(&self.username)
    {
      tracing::info!(
        username = self.username,
        "Peer sent an invalid resume offset (NS > 2 GiB bug); rejecting, the peer will re-queue"
      );
    }
    if start > self.size {
      return Err(ClientError::transfer(format!(
        "Requested start offset of {start} bytes exceeds file length of {} bytes",
        self.size
      )));
    }
    Ok(start)
  }

  /// Opens the source at `start`. A file is measured again: one whose size changed since the
  /// upload was queued fails it, and the peer queues the file again with the new size.
  async fn open_input(&mut self, start: u64) -> Result<Box<dyn UploadSource>, ClientError> {
    let input = self
      .input
      .take()
      .expect("the input is opened once per upload");
    match input {
      UploadInput::File(path) => {
        let failed = |error: std::io::Error| {
          ClientError::io(format!("Failed to open {}: {error}", path.display()), error)
        };
        let mut file = tokio::fs::File::open(&path).await.map_err(failed)?;
        let len = file.metadata().await.map_err(failed)?.len();
        if len != self.size {
          return Err(ClientError::transfer(format!(
            "file changed size since it was enqueued ({} bytes then, {len} bytes now)",
            self.size
          )));
        }
        file.seek(SeekFrom::Start(start)).await.map_err(failed)?;
        Ok(Box::new(file))
      }
      UploadInput::Stream { open, .. } => {
        let mut source = open(start)
          .await
          .map_err(|error| ClientError::io(format!("Failed to open the input: {error}"), error))?;
        if start > 0 && self.options.seek_input_stream_automatically {
          source
            .seek(SeekFrom::Start(start))
            .await
            .map_err(|error| ClientError::Transfer {
              message: format!(
                "Requested non-zero start offset but input stream does not support seeking: {error}"
              ),
              source: Some(Arc::new(error)),
            })?;
        }
        Ok(source)
      }
    }
  }

  /// Lets go of everything the upload held, in order. The record goes first, so a peer that
  /// queues the file again as soon as it hears of the failure is not refused as a duplicate.
  async fn clean_up(&mut self, held: Held, outcome: &Result<Transfer, ClientError>) {
    drop(self.registration.take());
    if let Err(error) = outcome {
      self.tell_peer(error).await;
    }
    drop(held.turn);
    if held.slot
      && let Some(released) = &self.options.slot_released
    {
      released(&self.tracker.snapshot());
    }
    drop(held.running);
  }

  /// Tells the peer an upload did not succeed: that we canceled it, so it drops the file, or
  /// that it failed, so it queues the file again. Best effort and bounded; skipped while the
  /// client disconnects or once it is disposed, but not because the server session was lost.
  async fn tell_peer(&mut self, error: &ClientError) {
    if self.client.state() == ClientState::Disconnecting
      || self.client.cancellation().is_cancelled()
    {
      tracing::debug!(
        "the client is disconnecting or disposed; the peer is not told the upload ended"
      );
      return;
    }
    if matches!(error, ClientError::UserOffline(_)) {
      return;
    }
    let message = match error {
      ClientError::Canceled => PeerMessage::UploadDenied(UploadDenied {
        filename: self.filename.clone(),
        reason: "Cancelled".into(),
      }),
      _ => PeerMessage::UploadFailed(UploadFailed {
        filename: self.filename.clone(),
      }),
    };
    let tell = async {
      // the address is looked up only when there is no connection to the peer to reuse
      let conn = self
        .client
        .message_connection(&self.username, None, &CancellationToken::new())
        .await?;
      write_peer(&conn, message).await
    };
    match tokio::time::timeout(COURTESY_BUDGET, tell).await {
      Ok(Ok(())) => tracing::debug!("the peer was told the upload ended"),
      Ok(Err(error)) => tracing::debug!(%error, "the peer could not be told the upload ended"),
      Err(_) => tracing::debug!("telling the peer the upload ended took too long; given up"),
    }
  }
}
