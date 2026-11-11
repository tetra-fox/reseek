//! What a caller passes with one download or upload: where the bytes go or come from, and the
//! options and callbacks that shape the transfer (speed governing, slot policy, size disputes,
//! how long an upload waits for the downloader to close, which message asks for a file).

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::RemoteFilename;
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
use tokio_util::sync::CancellationToken;

use super::Transfer;
use super::state::TransferState;
use crate::error::ClientError;
use crate::resolvers::ResolverFuture;

/// A boxed future the transfer callbacks return
pub type TransferFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// `(transfer, wanted, cancel)` to how many bytes the next chunk may move now, at most
/// `wanted`. It may wait until bytes are allowed, which is how a per-user limit is kept; one
/// that returns 0 is asked again after a short pause.
pub type Governor =
  Arc<dyn Fn(&Transfer, u32, CancellationToken) -> TransferFuture<u32> + Send + Sync>;

/// `(transfer, wanted, granted, moved)` after every chunk: the bytes the chunk wanted, the bytes
/// the governor and the speed limit allowed, and the bytes that actually moved
pub type Reporter = Arc<dyn Fn(&Transfer, u32, u32, u32) + Send + Sync>;

/// `(previous_state, transfer)` on every state change, before the client raises the event
pub type StateChanged = Arc<dyn Fn(TransferState, &Transfer) + Send + Sync>;

/// `(previous_bytes_transferred, transfer)` on every progress update, before the client raises
/// the event
pub type ProgressUpdated = Arc<dyn Fn(u64, &Transfer) + Send + Sync>;

/// `(transfer, cancel)`: resolves once the application's upload slot policy lets the upload
/// start. It must hold nothing when it fails; failing with `Canceled` cancels the upload, any
/// other error fails it.
pub type SlotAwaiter =
  Arc<dyn Fn(&Transfer, CancellationToken) -> ResolverFuture<()> + Send + Sync>;

/// `(transfer)`: the slot the awaiter granted is free again. Called once for every upload whose
/// awaiter succeeded, after the upload has let go of its turn with the user.
pub type SlotReleased = Arc<dyn Fn(&Transfer) + Send + Sync>;

/// `(transfer, remote_size)` to the size the download should use when the peer reports a size
/// other than the expected one. An error, or a size below the start offset, aborts the download.
pub type SizeMismatchResolver =
  Arc<dyn Fn(&Transfer, u64) -> Result<u64, ClientError> + Send + Sync>;

/// Something a download writes into
pub trait DownloadSink: AsyncWrite + AsyncSeek + Unpin + Send {}

impl<T: AsyncWrite + AsyncSeek + Unpin + Send> DownloadSink for T {}

/// Something an upload reads from
pub trait UploadSource: AsyncRead + AsyncSeek + Unpin + Send {}

impl<T: AsyncRead + AsyncSeek + Unpin + Send> UploadSource for T {}

/// Opens the sink of a download, once the uploader has connected, so a download that never
/// starts creates nothing
pub type OutputFactory =
  Box<dyn FnOnce() -> TransferFuture<std::io::Result<Box<dyn DownloadSink>>> + Send>;

/// Opens the source of an upload at the given start offset, once the downloader has said where
/// to start, so nothing is held open while the upload waits in a queue. The upload owns the
/// source and drops it when it ends.
pub type InputFactory =
  Box<dyn FnOnce(u64) -> TransferFuture<std::io::Result<Box<dyn UploadSource>>> + Send>;

/// Which message asks a peer for a file
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestStyle {
  /// `QueueDownload`, as every current client sends it; the peer does not answer it, so the
  /// download counts as queued at the peer once the request is written
  #[default]
  Queue,
  /// a download `TransferRequest`, which the peer answers with "queued", "send now" or a refusal
  TransferRequest,
  /// none: the peer offered the file without being asked and the client holds the offer, which
  /// the download takes at once. The sender waits for our answer, so the download does not wait
  /// for a download slot.
  Offered,
}

/// The options and callbacks of one transfer. Every callback is optional.
#[derive(Clone)]
pub struct TransferOptions {
  /// asked before every chunk how many bytes may move; unset allows everything
  pub governor: Option<Governor>,
  /// told after every chunk what moved
  pub reporter: Option<Reporter>,
  /// called on every state change
  pub state_changed: Option<StateChanged>,
  /// called on every progress update
  pub progress_updated: Option<ProgressUpdated>,
  /// uploads: waits for the application's upload slot; unset starts at once
  pub slot_awaiter: Option<SlotAwaiter>,
  /// uploads: hands the slot back
  pub slot_released: Option<SlotReleased>,
  /// downloads: settles a size the peer reports differently; unset aborts, except for the
  /// size 0 that old clients report for files of 2 GiB and more
  pub size_mismatch_resolver: Option<SizeMismatchResolver>,
  /// uploads: how long to wait after the last byte for the downloader to close the connection
  /// before closing it ourselves
  pub maximum_linger_time: Duration,
  /// downloads: which message asks the peer for the file
  pub request_style: RequestStyle,
  /// uploads: position the source at the start offset; unset leaves that to the input factory
  pub seek_input_stream_automatically: bool,
  /// downloads: position the sink at the start offset; unset leaves that to the output factory
  pub seek_output_stream_automatically: bool,
  /// downloads: flush and close the sink when the download ends, however it ends; always set
  /// for a file
  pub dispose_output_stream_on_completion: bool,
  /// downloads: how long to wait in the peer's queue for it to start sending; unset waits as
  /// long as it takes
  pub queued_timeout: Option<Duration>,
}

impl Default for TransferOptions {
  fn default() -> Self {
    Self {
      governor: None,
      reporter: None,
      state_changed: None,
      progress_updated: None,
      slot_awaiter: None,
      slot_released: None,
      size_mismatch_resolver: None,
      maximum_linger_time: Duration::from_secs(3),
      request_style: RequestStyle::Queue,
      seek_input_stream_automatically: true,
      seek_output_stream_automatically: true,
      dispose_output_stream_on_completion: true,
      queued_timeout: None,
    }
  }
}

/// shows which callbacks are set; the callbacks themselves have nothing to print
impl fmt::Debug for TransferOptions {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("TransferOptions")
      .field("governor", &self.governor.is_some())
      .field("reporter", &self.reporter.is_some())
      .field("state_changed", &self.state_changed.is_some())
      .field("progress_updated", &self.progress_updated.is_some())
      .field("slot_awaiter", &self.slot_awaiter.is_some())
      .field("slot_released", &self.slot_released.is_some())
      .field(
        "size_mismatch_resolver",
        &self.size_mismatch_resolver.is_some(),
      )
      .field("maximum_linger_time", &self.maximum_linger_time)
      .field("request_style", &self.request_style)
      .field(
        "seek_input_stream_automatically",
        &self.seek_input_stream_automatically,
      )
      .field(
        "seek_output_stream_automatically",
        &self.seek_output_stream_automatically,
      )
      .field(
        "dispose_output_stream_on_completion",
        &self.dispose_output_stream_on_completion,
      )
      .field("queued_timeout", &self.queued_timeout)
      .finish()
  }
}

/// Where a download's bytes go
pub enum DownloadOutput {
  /// a local file, opened read-write once the uploader connects: a start offset of 0 truncates
  /// it, a larger one keeps what is before the offset and drops what is after
  File(PathBuf),
  /// whatever the factory opens once the uploader connects
  Stream(OutputFactory),
}

/// Where an upload's bytes come from
pub enum UploadInput {
  /// a local file; its size is taken now, and the upload fails if it differs once the file is
  /// opened to start
  File(PathBuf),
  /// whatever the factory opens once the downloader says where to start; `size` is what the
  /// downloader is told
  Stream { size: u64, open: InputFactory },
}

/// One file to download from a peer
pub struct DownloadRequest {
  pub username: String,
  /// the file as the peer names it
  pub filename: RemoteFilename,
  /// the size the download expects
  pub size: u64,
  /// where to resume; bytes before it are not asked for
  pub start_offset: u64,
  /// the transfer's token; a fresh one when unset
  pub token: Option<u32>,
  pub output: DownloadOutput,
  pub options: TransferOptions,
}

impl DownloadRequest {
  /// a download of `filename` from `username` into `output`, from the start, with default
  /// options
  pub fn new(
    username: impl Into<String>,
    filename: impl Into<RemoteFilename>,
    size: u64,
    output: DownloadOutput,
  ) -> Self {
    Self {
      username: username.into(),
      filename: filename.into(),
      size,
      start_offset: 0,
      token: None,
      output,
      options: TransferOptions::default(),
    }
  }
}

/// One file to upload to a peer
pub struct UploadRequest {
  pub username: String,
  /// the file as the peer asked for it
  pub filename: RemoteFilename,
  pub input: UploadInput,
  /// the transfer's token; a fresh one when unset
  pub token: Option<u32>,
  pub options: TransferOptions,
}

impl UploadRequest {
  /// an upload of `input` to `username` as `filename`, with default options
  pub fn new(
    username: impl Into<String>,
    filename: impl Into<RemoteFilename>,
    input: UploadInput,
  ) -> Self {
    Self {
      username: username.into(),
      filename: filename.into(),
      input,
      token: None,
      options: TransferOptions::default(),
    }
  }
}
