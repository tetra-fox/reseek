//! The files of agents that peers download through the controller. Before an upload is queued
//! the agent may be asked whether the file still exists and how long it is. When the upload
//! starts, the agent is asked for the file from the peer's offset with a fresh token: it
//! acknowledges on its socket, then sends the bytes in an HTTP call that stays open while the
//! peer reads them, and is answered with how the upload ended. A refusal on the socket fails
//! the upload at once, as does the agent leaving before its bytes arrive.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use api_types::relay::{FileFailure, RelayFileUploadResult, RelayMessage, RelayUploadOutcome};
use axum::extract::Multipart;
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, timeout};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::Controller;
use crate::api::error::ApiError;
use crate::relay::credential::Purpose;
use crate::relay::frames;
use crate::safety::traverses;
use crate::services::AgentCredential;
use crate::sync::lock;

/// How many chunks of an agent's file wait to be read by the upload.
const CHUNKS: usize = 8;

/// A file info request waiting for its answer.
pub(super) struct FileInfoWait {
  pub agent: String,
  answer: oneshot::Sender<(bool, u64)>,
}

/// Why an agent's file did not arrive.
#[derive(Debug)]
pub(super) enum StreamFailure {
  /// the agent left
  Disconnected,
  /// the agent could not serve it
  Refused(FileFailure, Option<String>),
}

/// The bytes of an agent's file as they arrive.
type Chunks = mpsc::Receiver<io::Result<Bytes>>;

/// A file request waiting for the agent's acknowledgement and its bytes.
pub(super) struct StreamWait {
  pub agent: String,
  /// the remote filename asked for
  filename: String,
  /// the upload it is for
  transfer: Uuid,
  /// when the token stops being accepted
  expires: Instant,
  /// the agent acknowledged it
  ack: Option<oneshot::Sender<()>>,
  stream: Option<oneshot::Sender<Result<Chunks, StreamFailure>>>,
}

impl StreamWait {
  /// Fails the wait with `failure`.
  pub(super) fn fail(mut self, failure: StreamFailure) {
    if let Some(stream) = self.stream.take() {
      // the upload stopped waiting already when its receiver is gone
      drop(stream.send(Err(failure)));
    }
  }
}

/// An agent's file as an upload reads it, from the offset the peer asked for. It cannot seek:
/// the agent starts at the offset.
pub struct RelayStream {
  chunks: Chunks,
  /// the part of the last chunk not read yet
  left: Bytes,
}

impl AsyncRead for RelayStream {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    while self.left.is_empty() {
      match self.chunks.poll_recv(cx) {
        Poll::Ready(Some(Ok(chunk))) => self.left = chunk,
        Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(e)),
        Poll::Ready(None) => return Poll::Ready(Ok(())),
        Poll::Pending => return Poll::Pending,
      }
    }
    let n = self.left.len().min(buf.remaining());
    let chunk = self.left.split_to(n);
    buf.put_slice(&chunk);
    Poll::Ready(Ok(()))
  }
}

impl AsyncSeek for RelayStream {
  fn start_seek(self: Pin<&mut Self>, _: io::SeekFrom) -> io::Result<()> {
    Err(io::Error::new(
      io::ErrorKind::Unsupported,
      "an agent's file is sent from its offset and cannot seek",
    ))
  }

  fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
    Poll::Ready(Ok(0))
  }
}

/// Removes a file request's wait when the upload stops waiting for it, whatever the reason.
struct Waiting<'a> {
  controller: &'a Controller,
  token: Uuid,
}

impl Drop for Waiting<'_> {
  fn drop(&mut self) {
    lock(&self.controller.registry).streams.remove(&self.token);
  }
}

/// Whether the filename a multipart part was `sent` with names the file `asked` for. Encoders
/// disagree on backslashes in a quoted filename: reqwest escapes each as `\\`, browsers send
/// them as they are, and multer undoes only an escaped quote. Either form is taken.
fn names_the_same(sent: &str, asked: &str) -> bool {
  if sent == asked {
    return true;
  }
  let mut unescaped = String::with_capacity(sent.len());
  let mut chars = sent.chars();
  while let Some(c) = chars.next() {
    match c {
      '\\' => unescaped.extend(chars.next()),
      c => unescaped.push(c),
    }
  }
  unescaped == asked
}

impl Controller {
  /// Asks the agent `agent` whether `filename` exists and how long it is. `None` when the agent
  /// is not ready, leaves, or does not answer within the configured time.
  pub async fn file_info(&self, agent: &str, filename: &str) -> Option<(bool, u64)> {
    if !self.ready(agent) {
      debug!(agent, "Agent is not registered");
      return None;
    }
    let id = Uuid::new_v4();
    let (answer, answered) = oneshot::channel();
    lock(&self.registry).file_infos.insert(
      id,
      FileInfoWait {
        agent: agent.to_string(),
        answer,
      },
    );
    let asked = self.send(
      agent,
      frames::encode(
        Some(id),
        RelayMessage::FileInfoRequest {
          filename: filename.to_string(),
        },
      ),
    );
    debug!(agent, filename, %id, "Requested file information from Agent. Waiting for response.");
    let limit = self
      .config
      .current()
      .values
      .relay
      .controller_timeouts
      .file_info;
    if !asked {
      lock(&self.registry).file_infos.remove(&id);
      return None;
    }
    let answer = timeout(limit, answered).await;
    lock(&self.registry).file_infos.remove(&id);
    match answer {
      Ok(Ok(answer)) => Some(answer),
      Ok(Err(_)) => {
        debug!(
          agent,
          filename, "the agent left before it said whether it has the file"
        );
        None
      }
      Err(_) => {
        warn!(
          agent,
          filename,
          ?limit,
          "Timed out waiting for file information from agent"
        );
        None
      }
    }
  }

  /// Takes the agent `agent`'s answer `id` to a file info request; false when nobody waits
  /// for it.
  pub(super) fn file_info_arrived(&self, agent: &str, id: Uuid, exists: bool, length: u64) -> bool {
    let mut registry = lock(&self.registry);
    if !registry
      .file_infos
      .get(&id)
      .is_some_and(|w| w.agent == agent)
    {
      return false;
    }
    let wait = registry
      .file_infos
      .remove(&id)
      .expect("the wait was just found");
    if wait.answer.send((exists, length)).is_err() {
      debug!(agent, %id, "file info arrived after its asker gave up");
    }
    true
  }

  /// Asks the agent `agent` for `filename` from `start` for the upload `transfer`, and waits
  /// for its bytes: for the acknowledgement as long as the options say, then as long again for
  /// the bytes. An agent not logged in or without the file fails as not found, a wait too
  /// long as timed out.
  pub async fn open_stream(
    &self,
    agent: &str,
    filename: &str,
    start: u64,
    transfer: Uuid,
  ) -> io::Result<RelayStream> {
    if !self.ready(agent) {
      return Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("Agent {agent} is not connected"),
      ));
    }
    let timeouts = self
      .config
      .current()
      .values
      .relay
      .controller_timeouts
      .clone();
    let token = Uuid::new_v4();
    let (ack, acknowledged) = oneshot::channel();
    let (stream, arrived) = oneshot::channel();
    lock(&self.registry).streams.insert(
      token,
      StreamWait {
        agent: agent.to_string(),
        filename: filename.to_string(),
        transfer,
        expires: Instant::now() + timeouts.file_ack + timeouts.file_stream,
        ack: Some(ack),
        stream: Some(stream),
      },
    );
    let _waiting = Waiting {
      controller: self,
      token,
    };
    let asked = self.send(
      agent,
      frames::encode(
        Some(token),
        RelayMessage::FileRequest {
          filename: filename.to_string(),
          start_offset: start,
          transfer_id: transfer,
          token,
        },
      ),
    );
    if !asked {
      return Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("Agent {agent} is not connected"),
      ));
    }
    info!(agent, filename, %token, "Requested file from Agent. Waiting for incoming connection.");
    let timed_out = || {
      warn!(
        agent,
        filename, "Timed out attempting to retrieve the file from agent"
      );
      io::Error::new(
        io::ErrorKind::TimedOut,
        format!("Timed out attempting to retrieve the file {filename} from agent {agent}"),
      )
    };
    tokio::pin!(arrived);
    // the bytes may arrive before the acknowledgement does
    let early = tokio::select! {
      acked = acknowledged => {
        if acked.is_ok() {
          debug!(agent, %token, "agent acknowledged the file request");
        }
        None
      }
      result = &mut arrived => Some(result),
      () = tokio::time::sleep(timeouts.file_ack) => return Err(timed_out()),
    };
    let result = match early {
      Some(result) => result,
      None => timeout(timeouts.file_stream, &mut arrived)
        .await
        .map_err(|_| timed_out())?,
    };
    match result {
      Ok(Ok(chunks)) => {
        info!(agent, filename, %token, "Agent provided file stream for file");
        Ok(RelayStream {
          chunks,
          left: Bytes::new(),
        })
      }
      Ok(Err(StreamFailure::Refused(FileFailure::NotFound, detail))) => {
        debug!(agent, filename, detail, "agent does not have the file");
        Err(io::Error::new(
          io::ErrorKind::NotFound,
          format!("File could not be found on agent {agent}"),
        ))
      }
      Ok(Err(StreamFailure::Refused(reason, detail))) => Err(io::Error::other(format!(
        "Agent {agent} could not send the file ({reason:?}): {}",
        detail.unwrap_or_default()
      ))),
      Ok(Err(StreamFailure::Disconnected)) | Err(_) => Err(io::Error::new(
        io::ErrorKind::ConnectionAborted,
        format!("Agent {agent} disconnected"),
      )),
    }
  }

  /// Takes the agent's acknowledgement of the file request `id`; false when nobody waits for
  /// it.
  pub(super) fn stream_acknowledged(&self, agent: &str, id: Uuid) -> bool {
    let mut registry = lock(&self.registry);
    let Some(wait) = registry.streams.get_mut(&id).filter(|w| w.agent == agent) else {
      return false;
    };
    if let Some(ack) = wait.ack.take()
      && ack.send(()).is_err()
    {
      debug!(agent, %id, "file acknowledged after its upload gave up");
    }
    true
  }

  /// Fails the file request `id` the agent could not serve; false when nobody waits for it.
  pub(super) fn stream_failed(
    &self,
    agent: &str,
    id: Uuid,
    reason: FileFailure,
    status: Option<u16>,
    detail: Option<String>,
  ) -> bool {
    let wait = {
      let mut registry = lock(&self.registry);
      if !registry
        .streams
        .get(&id)
        .is_some_and(|w| w.agent == agent && w.stream.is_some())
      {
        return false;
      }
      registry
        .streams
        .remove(&id)
        .expect("the wait was just found")
    };
    debug!(agent, %id, ?reason, status, detail, "agent could not send a file");
    wait.fail(StreamFailure::Refused(reason, detail));
    true
  }

  /// Takes the file the agent `caller` sends with `token`, passes its bytes on to the upload
  /// that asked for it, and answers once that upload has ended.
  pub async fn receive_file(
    &self,
    token: Uuid,
    caller: AgentCredential,
    mut form: Multipart,
  ) -> Result<RelayFileUploadResult, ApiError> {
    let name = caller.agent.clone();
    info!(agent = name, %token, "Handling file upload from a caller claiming to be agent");
    // the token is used up whether or not the call checks out
    let wait = {
      let mut registry = lock(&self.registry);
      let found = registry
        .streams
        .get_mut(&token)
        .filter(|w| w.agent == name && w.expires > Instant::now());
      found.map(|w| (w.filename.clone(), w.transfer, w.stream.take()))
    };
    let Some((filename, transfer, stream)) = wait else {
      warn!(agent = name, %token, "file upload with an unknown or expired token");
      return Err(ApiError::unauthorized("the token is unknown or expired").logged());
    };
    let Some(stream) = stream else {
      warn!(agent = name, %token, "file upload with a used token");
      return Err(ApiError::unauthorized("the token was used").logged());
    };
    self.authenticate(&caller, Purpose::File, token)?;
    let mut field = form
      .next_field()
      .await
      .map_err(|e| ApiError::bad_request(e.body_text()))?
      .ok_or_else(|| ApiError::bad_request("the file part is missing"))?;
    let named = field.file_name().unwrap_or_default().to_string();
    if named.is_empty() {
      return Err(ApiError::bad_request("the file part has no filename"));
    }
    if traverses(&named) {
      warn!(
        agent = name,
        filename = named,
        "Suspicious file upload with unsafe path segments"
      );
      return Err(ApiError::bad_request("the filename contains a traversal segment").logged());
    }
    if !names_the_same(&named, &filename) {
      warn!(
        agent = name,
        asked = filename,
        sent = named,
        "agent sent a file other than the one asked for"
      );
      return Err(ApiError::bad_request("the filename is not the one asked for").logged());
    }
    let (chunks, read) = mpsc::channel(CHUNKS);
    let (done, ended) = oneshot::channel();
    lock(&self.registry).completions.insert(transfer, done);
    if stream.send(Ok(read)).is_err() {
      lock(&self.registry).completions.remove(&transfer);
      return Err(ApiError::conflict(
        "no_pending_request",
        Some("A file stream response matching this token is not expected".to_string()),
      ));
    }
    info!(agent = name, filename, %token, "Agent authenticated for token. Forwarding file stream");
    loop {
      let chunk = tokio::select! {
        () = self.shutdown.cancelled() => break,
        chunk = field.chunk() => chunk,
      };
      let sent = match chunk {
        Ok(Some(bytes)) => chunks.send(Ok(bytes)).await,
        Ok(None) => break,
        Err(e) => {
          debug!(agent = name, error = %e, "agent's file stopped arriving");
          // the upload reads the error, or has ended
          drop(chunks.send(Err(io::Error::other(e.body_text()))).await);
          break;
        }
      };
      if sent.is_err() {
        // the upload stopped reading: it ended
        break;
      }
    }
    drop(chunks);
    let result = tokio::select! {
      () = self.shutdown.cancelled() => None,
      result = ended => result.ok(),
    };
    lock(&self.registry).completions.remove(&transfer);
    let result = result.unwrap_or(RelayFileUploadResult {
      outcome: RelayUploadOutcome::Failed,
      bytes: 0,
    });
    info!(agent = name, filename, %token, outcome = ?result.outcome, bytes = result.bytes, "File upload from agent complete");
    Ok(result)
  }

  /// Tells the agent's call that sent the file of the upload `transfer` how that upload ended;
  /// nothing when the upload's file was not an agent's.
  pub fn finished(&self, transfer: Uuid, outcome: RelayUploadOutcome, bytes: u64) {
    if let Some(done) = lock(&self.registry).completions.remove(&transfer) {
      // the call is gone when the agent hung up first
      drop(done.send(RelayFileUploadResult { outcome, bytes }));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_part_filename_is_taken_escaped_or_not() {
    let asked = "Music\\Album\\01 \"live\".flac";
    assert!(names_the_same(asked, asked));
    assert!(names_the_same("Music\\\\Album\\\\01 \"live\".flac", asked));
    assert!(!names_the_same("Music\\Album\\02.flac", asked));
  }
}
