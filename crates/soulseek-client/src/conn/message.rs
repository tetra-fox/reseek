//! A message connection: a connection split into a framing read loop that hands each frame to
//! its owner, and a writer task that drains a bounded queue of outgoing frames so writes never
//! interleave. A queue that fills up means the remote has stopped reading; the connection is
//! closed rather than buffering without bound.

use std::fmt;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use soulseek_proto::Limits;
use soulseek_proto::frame::{self, FrameHeader, FrameKind};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tracing::Instrument;

use super::{Conn, ConnHandle, ConnReadHalf, ConnWriteHalf};
use crate::error::{ClientError, ConnectionErrorKind};

/// frames of one gated connection being handled at once, so a peer whose handlers back up holds
/// at most this many of the shared gate's permits and other connections keep being read
const CONNECTION_HANDLERS: usize = 16;

/// What the read loop reports about the frames it reads, in order
#[derive(Debug)]
pub enum ReadEvent {
  /// a frame's length and code, read before its payload
  Header { len: u32, code: u32 },
  /// how much of a frame's payload has arrived, starting at 0 right after its header
  Progress {
    code: u32,
    current: usize,
    total: usize,
  },
  /// a complete frame, length prefix included, as the message decoders take it
  Frame(Vec<u8>),
}

/// What the read loop does with the payload of the frame whose header it just reported
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameBody {
  /// reads it into the frame and reports its progress
  Read,
  /// reads past it without keeping it, so no frame is reported
  Skip,
}

/// A complete message frame, length prefix included, that was encoded ahead of time and is read
/// from `reader` as it is written, so a large one is never held in memory whole
pub struct EncodedFrame {
  /// bytes the reader yields for the frame, length prefix included
  pub len: u64,
  /// where the frame is read from, a file for instance; it must yield `len` bytes
  pub reader: Pin<Box<dyn AsyncRead + Send>>,
}

/// the reader has nothing worth printing
impl fmt::Debug for EncodedFrame {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("EncodedFrame")
      .field("len", &self.len)
      .finish_non_exhaustive()
  }
}

enum Payload {
  Bytes(Vec<u8>),
  Encoded(EncodedFrame),
  /// closes the connection with this reason once everything queued before it is written
  Close(String),
}

/// What the handler of a gated frame holds until the frame is handled: one of the shared gate's
/// permits and one of its connection's own
pub(crate) struct HandlerPermit {
  _shared: OwnedSemaphorePermit,
  _connection: OwnedSemaphorePermit,
}

/// resolves once the write it belongs to is on the socket, or with why it never got there
pub(crate) type Receipt = oneshot::Receiver<Result<(), ClientError>>;

struct Outgoing {
  payload: Payload,
  written: Option<oneshot::Sender<Result<(), ClientError>>>,
}

struct Reading {
  half: ConnReadHalf,
  frame_kind: FrameKind,
  limits: Limits,
}

/// A connection that exchanges framed messages. Clones share the connection.
#[derive(Clone)]
pub struct MessageConn {
  handle: ConnHandle,
  frame_kind: FrameKind,
  queue: mpsc::Sender<Outgoing>,
  /// set once the connection is to close after its queued writes; later writes are refused
  closing: Arc<AtomicBool>,
  /// the read half until the read loop takes it
  reading: Arc<Mutex<Option<Reading>>>,
}

/// shows the connection underneath
impl fmt::Debug for MessageConn {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.handle.fmt(f)
  }
}

impl Conn {
  /// Turns the connection into a message connection whose frames have `frame_kind`'s code width
  /// and limits. Writing works at once; reading starts with [`MessageConn::start_reading`].
  pub fn into_message(
    self,
    frame_kind: FrameKind,
    limits: Limits,
  ) -> Result<MessageConn, ClientError> {
    let queue_size = self.handle.options().write_queue_size as usize;
    let (read, write) = self.into_split()?;
    let handle = read.handle().clone();
    let (queue, outgoing) = mpsc::channel(queue_size);
    tokio::spawn(write_loop(write, outgoing).instrument(handle.span().clone()));
    Ok(MessageConn {
      handle,
      frame_kind,
      queue,
      closing: Arc::new(AtomicBool::new(false)),
      reading: Arc::new(Mutex::new(Some(Reading {
        half: read,
        frame_kind,
        limits,
      }))),
    })
  }
}

impl MessageConn {
  /// the underlying connection
  pub fn handle(&self) -> &ConnHandle {
    &self.handle
  }

  /// the framing of this connection's messages
  pub fn frame_kind(&self) -> FrameKind {
    self.frame_kind
  }

  /// Starts the read loop, which hands every [`ReadEvent`] to `sink` in order until the
  /// connection closes. `sink` runs on the loop's task, so it must not block; anything slow
  /// belongs on a task of its own. Only the first call starts a loop; later ones return false.
  pub fn start_reading(&self, mut sink: impl FnMut(ReadEvent) + Send + 'static) -> bool {
    self.spawn_read_loop(None, move |event, _| {
      sink(event);
      FrameBody::Read
    })
  }

  /// Starts the read loop like [`MessageConn::start_reading`], except that after reading each
  /// frame the loop waits for one of the connection's `CONNECTION_HANDLERS` turns, then for a
  /// permit from `gate`, and hands both to `sink` with the frame. The next frame is read only
  /// once both were free, so while the permits are held by frames being handled the remote's
  /// writes back up instead of frames being dropped. Header and progress events come without a
  /// permit. What `sink` answers to a header decides whether the frame's payload is read or
  /// skipped; its answer to other events is not used.
  pub(crate) fn start_reading_gated(
    &self,
    gate: Arc<Semaphore>,
    sink: impl FnMut(ReadEvent, Option<HandlerPermit>) -> FrameBody + Send + 'static,
  ) -> bool {
    self.spawn_read_loop(Some(gate), sink)
  }

  fn spawn_read_loop(
    &self,
    gate: Option<Arc<Semaphore>>,
    sink: impl FnMut(ReadEvent, Option<HandlerPermit>) -> FrameBody + Send + 'static,
  ) -> bool {
    let reading = self
      .reading
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .take();
    let Some(reading) = reading else {
      return false;
    };
    tokio::spawn(read_loop(reading, gate, sink).instrument(self.handle.span().clone()));
    true
  }

  /// Queues `bytes` to be written without waiting for the write. Fails at once when the queue
  /// is full, which closes the connection; a write that fails later closes it as well.
  pub fn send(&self, bytes: Vec<u8>) -> Result<(), ClientError> {
    self.enqueue(Payload::Bytes(bytes), None)
  }

  /// queues `bytes` and waits until they are written
  pub async fn write(&self, bytes: Vec<u8>) -> Result<(), ClientError> {
    written(self.send_with_receipt(bytes)?).await
  }

  /// Queues `frame` and waits until all of it is written. A reader that fails or ends early
  /// after part of the frame went out closes the connection, whose stream can no longer be
  /// framed.
  pub async fn write_encoded(&self, frame: EncodedFrame) -> Result<(), ClientError> {
    written(self.send_encoded_with_receipt(frame)?).await
  }

  /// queues `bytes` and returns what resolves once they are written, so several writes can be
  /// waited for together
  pub(crate) fn send_with_receipt(&self, bytes: Vec<u8>) -> Result<Receipt, ClientError> {
    let (written, done) = oneshot::channel();
    self.enqueue(Payload::Bytes(bytes), Some(written))?;
    Ok(done)
  }

  /// [`MessageConn::send_with_receipt`] for a frame encoded ahead of time
  pub(crate) fn send_encoded_with_receipt(
    &self,
    frame: EncodedFrame,
  ) -> Result<Receipt, ClientError> {
    let (written, done) = oneshot::channel();
    self.enqueue(Payload::Encoded(frame), Some(written))?;
    Ok(done)
  }

  /// Closes the connection with `reason` once the writes already queued are on the socket, and
  /// refuses writes from now on; reading goes on until the close. A full queue means the remote
  /// has stopped reading, so the connection is closed at once instead.
  pub(crate) fn close_when_flushed(&self, reason: &str) {
    self.closing.store(true, Ordering::Release);
    let close = Outgoing {
      payload: Payload::Close(reason.to_owned()),
      written: None,
    };
    match self.queue.try_send(close) {
      Ok(()) => {}
      Err(mpsc::error::TrySendError::Full(_)) => {
        self.handle.disconnect(reason, None);
      }
      // the write loop has ended, which closed the connection
      Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
  }

  /// writes waiting in the queue
  pub fn write_queue_depth(&self) -> usize {
    self.queue.max_capacity() - self.queue.capacity()
  }

  fn enqueue(
    &self,
    payload: Payload,
    written: Option<oneshot::Sender<Result<(), ClientError>>>,
  ) -> Result<(), ClientError> {
    let empty = match &payload {
      Payload::Bytes(bytes) => bytes.is_empty(),
      Payload::Encoded(frame) => frame.len == 0,
      Payload::Close(_) => false,
    };
    if empty {
      return Err(ClientError::InvalidArgument(
        "Invalid attempt to send empty data".into(),
      ));
    }
    if !self.handle.is_connected() {
      return Err(self.handle.not_connected());
    }
    if self.closing.load(Ordering::Acquire) {
      return Err(ClientError::InvalidOperation(
        "Invalid attempt to send to a connection that closes once its queued writes are out".into(),
      ));
    }
    match self.queue.try_send(Outgoing { payload, written }) {
      Ok(()) => Ok(()),
      Err(mpsc::error::TrySendError::Full(_)) => {
        self.handle.disconnect("The write buffer is full", None);
        Err(ClientError::connection(
          ConnectionErrorKind::WriteDropped,
          format!(
            "Dropped buffered message to {}; the write buffer is full",
            self.handle.remote()
          ),
        ))
      }
      Err(mpsc::error::TrySendError::Closed(_)) => Err(self.handle.not_connected()),
    }
  }
}

async fn write_loop(mut half: ConnWriteHalf, mut queue: mpsc::Receiver<Outgoing>) {
  loop {
    let outgoing = tokio::select! {
      biased;
      () = half.handle().cancellation().cancelled() => return,
      outgoing = queue.recv() => outgoing,
    };
    // every sender is gone, so nothing more can be written
    let Some(Outgoing { payload, written }) = outgoing else {
      return;
    };
    let result = match payload {
      Payload::Bytes(bytes) => half.write_all(&bytes).await,
      Payload::Encoded(frame) => write_encoded(&mut half, frame).await,
      Payload::Close(reason) => {
        half.handle().disconnect(reason, None);
        return;
      }
    };
    if let Some(written) = written {
      // the writer may have stopped waiting; the connection has logged the failure either way
      let _ = written.send(result);
    }
    // a failed socket write has closed the connection; an encoded frame whose reader failed
    // before any of it was written leaves the connection usable
    if !half.handle().is_connected() {
      return;
    }
  }
}

/// what a write that was queued comes to; a writer that went away took the connection with it
pub(crate) async fn written(done: Receipt) -> Result<(), ClientError> {
  done.await.unwrap_or_else(|_| {
    Err(ClientError::connection(
      ConnectionErrorKind::Write,
      "Write aborted after 0 bytes written; the connection has been or is being disposed/disconnected",
    ))
  })
}

/// Copies the frame from its reader in chunks of the write buffer size. Once part of it is on the
/// socket the frame must be finished, so a reader that fails or ends early closes the connection.
async fn write_encoded(half: &mut ConnWriteHalf, frame: EncodedFrame) -> Result<(), ClientError> {
  let EncodedFrame { len, mut reader } = frame;
  let mut chunk = vec![0; half.handle().options().write_buffer_size];
  let mut total = 0;
  while total < len {
    let want = chunk
      .len()
      .min(usize::try_from(len - total).unwrap_or(usize::MAX));
    let read = match reader.read(&mut chunk[..want]).await {
      Ok(0) => Err(format!(
        "the encoded message ended after {total} of {len} bytes"
      )),
      Ok(read) => Ok(read),
      Err(error) => Err(format!(
        "reading the encoded message failed after {total} of {len} bytes: {error}"
      )),
    };
    let read = match read {
      Ok(read) => read,
      Err(reason) => {
        let error = ClientError::connection(ConnectionErrorKind::Write, reason.clone());
        if total > 0 {
          half
            .handle()
            .disconnect(format!("Write error: {reason}"), Some(error.clone()));
        }
        return Err(error);
      }
    };
    half.write_all(&chunk[..read]).await?;
    total += read as u64;
  }
  Ok(())
}

async fn read_loop(
  reading: Reading,
  gate: Option<Arc<Semaphore>>,
  mut sink: impl FnMut(ReadEvent, Option<HandlerPermit>) -> FrameBody,
) {
  let Reading {
    mut half,
    frame_kind,
    limits,
  } = reading;
  let gate = gate.map(|shared| (shared, Arc::new(Semaphore::new(CONNECTION_HANDLERS))));
  let width = frame_kind.code_width();
  let mut chunk = vec![0; half.handle().options().read_buffer_size];
  loop {
    let mut frame = vec![0; 4 + width];
    // a failed read has closed the connection and said why, so the loop just ends
    if half.read_exact(&mut frame[..4]).await.is_err() {
      return;
    }
    // the length alone can disqualify the frame, so check it before waiting for the code
    if let Err(error) = frame::header(frame_kind, &frame[..4], &limits) {
      return out_of_range(half.handle(), error);
    }
    if half.read_exact(&mut frame[4..]).await.is_err() {
      return;
    }
    let FrameHeader { len, code } = frame::header(frame_kind, &frame, &limits)
      .ok()
      .flatten()
      .expect("a length that passed the check, followed by the code, is a complete header");
    let total = len as usize - width;
    if sink(ReadEvent::Header { len, code }, None) == FrameBody::Skip {
      let mut skipped = 0;
      while skipped < total {
        let size = chunk.len().min(total - skipped);
        if half.read_exact(&mut chunk[..size]).await.is_err() {
          return;
        }
        skipped += size;
      }
      tracing::trace!(len, code, "frame skipped");
      continue;
    }
    sink(
      ReadEvent::Progress {
        code,
        current: 0,
        total,
      },
      None,
    );
    // the frame grows with what arrives, not with what the length prefix claims
    let mut current = 0;
    while current < total {
      let size = chunk.len().min(total - current);
      if half.read_exact(&mut chunk[..size]).await.is_err() {
        return;
      }
      frame.extend_from_slice(&chunk[..size]);
      current += size;
      sink(
        ReadEvent::Progress {
          code,
          current,
          total,
        },
        None,
      );
    }
    tracing::trace!(len, code, "frame read");
    // a frame read whole is handed over even when the connection closes while it waits, since
    // a wait for it may already count on it; the next read on the closed connection ends the loop
    // the connection's own turn comes first, so a connection at its bound waits without taking
    // a shared permit from the others
    let permit = match &gate {
      None => None,
      Some((shared, connection)) => {
        let connection = connection
          .clone()
          .acquire_owned()
          .await
          .expect("a connection's handler semaphore is never closed");
        let shared = shared
          .clone()
          .acquire_owned()
          .await
          .expect("a read gate is never closed");
        Some(HandlerPermit {
          _shared: shared,
          _connection: connection,
        })
      }
    };
    sink(ReadEvent::Frame(frame), permit);
  }
}

fn out_of_range(handle: &ConnHandle, error: soulseek_proto::DecodeError) {
  handle.disconnect(
    "message length out of range",
    Some(ClientError::message_read(
      format!("message length out of range: {error}"),
      error,
    )),
  );
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use soulseek_proto::LIMIT_DISTRIBUTED_MESSAGE_BYTES;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio::net::TcpStream;

  use super::*;
  use crate::conn::ConnState;
  use crate::options::ConnectionOptions;

  async fn pair(options: ConnectionOptions, frame_kind: FrameKind) -> (MessageConn, TcpStream) {
    let (conn, remote) = crate::conn::tests::pair(options).await;
    (
      conn.into_message(frame_kind, Limits::DEFAULT).unwrap(),
      remote,
    )
  }

  fn collect(conn: &MessageConn) -> mpsc::UnboundedReceiver<ReadEvent> {
    let (events, received) = mpsc::unbounded_channel();
    assert!(conn.start_reading(move |event| {
      // the test drops the receiver when it is done looking
      let _ = events.send(event);
    }));
    received
  }

  /// the permits of the frames `conn` reads under `gate`, in order
  fn collect_gated(
    conn: &MessageConn,
    gate: Arc<Semaphore>,
  ) -> mpsc::UnboundedReceiver<HandlerPermit> {
    let (permits, received) = mpsc::unbounded_channel();
    assert!(conn.start_reading_gated(gate, move |event, permit| {
      if let ReadEvent::Frame(_) = event {
        // the test drops the receiver when it is done looking
        let _ = permits.send(permit.expect("a gated frame comes with a permit"));
      }
      FrameBody::Read
    }));
    received
  }

  #[tokio::test]
  async fn a_connection_holding_its_handlers_leaves_the_gate_to_others() {
    let gate = Arc::new(Semaphore::new(CONNECTION_HANDLERS + 1));
    let (flooding, mut flooder) = pair(ConnectionOptions::default(), FrameKind::Peer).await;
    let (other, mut remote) = pair(ConnectionOptions::default(), FrameKind::Peer).await;
    let mut held = collect_gated(&flooding, gate.clone());
    let mut handled = collect_gated(&other, gate);
    // a frame with a code and no payload
    let frame = [4, 0, 0, 0, 1, 0, 0, 0];
    flooder
      .write_all(&frame.repeat(CONNECTION_HANDLERS + 4))
      .await
      .unwrap();
    let mut permits = Vec::new();
    for _ in 0..CONNECTION_HANDLERS {
      permits.push(held.recv().await.unwrap());
    }
    remote.write_all(&frame).await.unwrap();
    let _handled = tokio::time::timeout(Duration::from_secs(5), handled.recv())
      .await
      .expect("the other connection's frame is handled")
      .unwrap();
    assert!(
      held.try_recv().is_err(),
      "the flooding connection waits for its own handlers"
    );
    drop(permits.pop());
    let _next = held.recv().await.unwrap();
  }

  #[tokio::test]
  async fn a_close_after_queued_writes_sends_them_first_and_refuses_more() {
    let (conn, mut remote) = pair(ConnectionOptions::default(), FrameKind::Peer).await;
    let first = conn.send_with_receipt(vec![1; 100_000]).unwrap();
    let second = conn.send_with_receipt(vec![2; 10]).unwrap();
    conn.close_when_flushed("Superseded");
    assert!(
      conn.send(vec![3]).is_err(),
      "a write after the close is refused"
    );
    let mut received = Vec::new();
    remote.read_to_end(&mut received).await.unwrap();
    assert_eq!(received.len(), 100_010);
    assert!(received[100_000..].iter().all(|byte| *byte == 2));
    assert!(first.await.unwrap().is_ok());
    assert!(second.await.unwrap().is_ok());
    assert_eq!(conn.handle().disconnected().await.message, "Superseded");
  }

  #[tokio::test]
  async fn frames_are_reported_with_header_progress_and_bytes() {
    let options = ConnectionOptions {
      read_buffer_size: 2,
      ..ConnectionOptions::default()
    };
    let (conn, mut remote) = pair(options, FrameKind::Peer).await;
    let mut events = collect(&conn);
    assert!(!conn.start_reading(|_| {}), "the loop starts only once");
    let frame = [7, 0, 0, 0, 9, 0, 0, 0, 1, 2, 3];
    remote.write_all(&frame).await.unwrap();
    assert!(matches!(
      events.recv().await,
      Some(ReadEvent::Header { len: 7, code: 9 })
    ));
    let mut progress = Vec::new();
    loop {
      match events.recv().await.unwrap() {
        ReadEvent::Progress { current, total, .. } => progress.push((current, total)),
        ReadEvent::Frame(bytes) => {
          assert_eq!(bytes, frame);
          break;
        }
        other => panic!("unexpected {other:?}"),
      }
    }
    assert_eq!(progress, [(0, 3), (2, 3), (3, 3)]);
  }

  #[tokio::test]
  async fn a_length_over_the_limit_closes_the_connection() {
    let (conn, mut remote) = pair(ConnectionOptions::default(), FrameKind::Distributed).await;
    let _events = collect(&conn);
    remote
      .write_all(&(LIMIT_DISTRIBUTED_MESSAGE_BYTES + 1).to_le_bytes())
      .await
      .unwrap();
    let reason = conn.handle().disconnected().await;
    assert_eq!(reason.message, "message length out of range");
  }

  #[tokio::test]
  async fn a_length_shorter_than_the_code_closes_the_connection_without_waiting() {
    let (conn, mut remote) = pair(ConnectionOptions::default(), FrameKind::Server).await;
    let _events = collect(&conn);
    remote.write_all(&2u32.to_le_bytes()).await.unwrap();
    let reason = conn.handle().disconnected().await;
    assert_eq!(reason.message, "message length out of range");
  }

  #[tokio::test]
  async fn writes_arrive_whole_and_in_order() {
    let options = ConnectionOptions {
      write_buffer_size: 3,
      ..ConnectionOptions::default()
    };
    let (conn, mut remote) = pair(options, FrameKind::Peer).await;
    conn.send(vec![1, 2, 3, 4]).unwrap();
    conn.write(vec![5, 6]).await.unwrap();
    let mut received = [0; 6];
    remote.read_exact(&mut received).await.unwrap();
    assert_eq!(received, [1, 2, 3, 4, 5, 6]);
  }

  #[tokio::test]
  async fn a_full_write_queue_drops_the_write_and_closes_the_connection() {
    let options = ConnectionOptions {
      write_queue_size: 1,
      ..ConnectionOptions::default()
    };
    // the remote never reads, so the writer blocks on a frame larger than the socket buffers
    let (conn, _remote) = pair(options, FrameKind::Peer).await;
    conn.send(vec![0; 64 << 20]).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
      while conn.write_queue_depth() > 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("the writer takes the first frame");
    conn.send(vec![1]).unwrap();
    let error = conn.send(vec![1]).unwrap_err();
    assert_eq!(
      error.connection_kind(),
      Some(ConnectionErrorKind::WriteDropped)
    );
    assert!(
      error.to_string().ends_with("the write buffer is full"),
      "{error}"
    );
    let reason = conn.handle().disconnected().await;
    assert_eq!(reason.message, "The write buffer is full");
    assert!(matches!(
      conn.send(vec![1]),
      Err(ClientError::InvalidOperation(_))
    ));
    assert!(matches!(conn.handle().state(), ConnState::Disconnected(_)));
  }

  #[tokio::test]
  async fn an_encoded_frame_streams_whole_between_other_writes() {
    let options = ConnectionOptions {
      write_buffer_size: 4,
      ..ConnectionOptions::default()
    };
    let (conn, mut remote) = pair(options, FrameKind::Peer).await;
    let frame: Vec<u8> = (0..23).collect();
    conn.send(vec![100]).unwrap();
    conn
      .write_encoded(EncodedFrame {
        len: frame.len() as u64,
        reader: Box::pin(std::io::Cursor::new(frame.clone())),
      })
      .await
      .unwrap();
    conn.write(vec![200]).await.unwrap();
    let mut received = vec![0; 25];
    remote.read_exact(&mut received).await.unwrap();
    assert_eq!(received[0], 100);
    assert_eq!(received[1..24], frame);
    assert_eq!(received[24], 200);
  }

  #[tokio::test]
  async fn an_encoded_frame_that_ends_early_closes_the_connection() {
    let options = ConnectionOptions {
      write_buffer_size: 4,
      ..ConnectionOptions::default()
    };
    let (conn, _remote) = pair(options, FrameKind::Peer).await;
    let error = conn
      .write_encoded(EncodedFrame {
        len: 10,
        reader: Box::pin(std::io::Cursor::new(vec![1; 6])),
      })
      .await
      .unwrap_err();
    assert_eq!(
      error.to_string(),
      "the encoded message ended after 6 of 10 bytes"
    );
    let reason = conn.handle().disconnected().await;
    assert_eq!(
      reason.message,
      "Write error: the encoded message ended after 6 of 10 bytes"
    );
  }

  #[tokio::test]
  async fn an_encoded_frame_with_nothing_to_read_leaves_the_connection_usable() {
    let (conn, mut remote) = pair(ConnectionOptions::default(), FrameKind::Peer).await;
    let error = conn
      .write_encoded(EncodedFrame {
        len: 10,
        reader: Box::pin(std::io::Cursor::new(Vec::new())),
      })
      .await
      .unwrap_err();
    assert_eq!(
      error.to_string(),
      "the encoded message ended after 0 of 10 bytes"
    );
    conn.write(vec![7]).await.unwrap();
    let mut received = [0; 1];
    remote.read_exact(&mut received).await.unwrap();
    assert_eq!(received, [7]);
  }
}
