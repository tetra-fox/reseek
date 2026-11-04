//! What the client does with each message a peer sends on a message connection: answer it from
//! the application's resolvers, complete the wait of the request it answers, or raise the event
//! it carries. Every frame is handled on a task of its own, with a bound on how many run at once
//! across all connections; at the bound a connection's reader waits before reading its next
//! frame, so a stuck resolver slows the peers sending to it and loses nothing.
//!
//! Browse, folder and search responses are compressed, and anyone can connect and send them
//! under any name. Each is inflated only when something waits for it: a browse response only
//! when a browse followed its header on this connection, a folder or search response only once
//! the token at its start, inflated alone, is one we wait for. A connection inflates one of them
//! at a time, on the blocking pool.

use std::sync::Arc;

use soulseek_proto::frame::{self, FrameKind};
use soulseek_proto::{
  DecodeError, Limits, PeerCode, PeerMessage, TransferDirection, peek_folder_contents_token,
  peek_search_response,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tracing::Instrument;

use crate::client::Client;
use crate::conn::{MessageConn, ReadEvent};
use crate::error::{ClientError, MessageErrorKind};
use crate::waiter::WaitKey;

/// The start of a browse response on its way, so its progress can be followed: the frame's
/// length, the connection bringing it, and how many payload bytes of it have arrived
#[derive(Clone, Debug)]
pub struct IncomingBrowse {
  /// the frame's length as its prefix gives it: code and payload
  pub length: u32,
  /// the message connection the response arrives on
  pub connection: MessageConn,
  /// payload bytes received so far
  pub received: watch::Receiver<usize>,
}

/// What a frame's handler is given by the connection it came on
struct Arrival {
  frame: Vec<u8>,
  /// held until the frame is handled, against the bound on frames handled at once
  permit: Option<OwnedSemaphorePermit>,
  /// a browse followed the header of this frame, which is a browse response
  followed: bool,
  /// the connection's turn to inflate, one compressed message at a time
  inflating: Arc<Semaphore>,
}

impl Client {
  /// Starts the read loop of a peer message connection. A browse response's header completes
  /// the wait for it before the payload arrives, and its progress is published as it does.
  pub(crate) fn start_peer_reading(&self, conn: &MessageConn) {
    let client = self.downgrade();
    let reader = conn.clone();
    let mut browse: Option<watch::Sender<usize>> = None;
    let mut followed = false;
    let inflating = Arc::new(Semaphore::new(1));
    let gate = self.peers().handlers.clone();
    conn.start_reading_gated(gate, move |event, permit| match event {
      ReadEvent::Header { len, code } if code == PeerCode::BrowseResponse.code() => {
        let (progress, received) = watch::channel(0);
        browse = Some(progress);
        followed = false;
        if let (Some(client), Some(username)) = (client.upgrade(), reader.handle().username()) {
          let key = WaitKey::BrowseResponseConnection {
            username: username.to_owned(),
          };
          let incoming = IncomingBrowse {
            length: len,
            connection: reader.clone(),
            received,
          };
          // every browse of this user follows the response
          followed = client.waiter().complete_all(&key, incoming) > 0;
        }
      }
      ReadEvent::Progress { code, current, .. } if code == PeerCode::BrowseResponse.code() => {
        if let Some(progress) = &browse {
          progress.send_replace(current);
        }
      }
      ReadEvent::Frame(frame) => {
        if let Some(client) = client.upgrade() {
          let arrival = Arrival {
            frame,
            permit,
            followed,
            inflating: inflating.clone(),
          };
          client.dispatch_peer_frame(&reader, arrival);
        }
      }
      ReadEvent::Header { .. } | ReadEvent::Progress { .. } => {}
    });
  }

  /// Hands the frame to a task of its own, which keeps its permit until the frame is handled
  fn dispatch_peer_frame(&self, conn: &MessageConn, arrival: Arrival) {
    let client = self.clone();
    let conn = conn.clone();
    let span = conn.handle().span().clone();
    tokio::spawn(async move { client.handle_peer_frame(&conn, arrival).await }.instrument(span));
  }

  /// A search response goes to the search its token names. Only the username and token are
  /// inflated first, so a response for a search that is over costs next to nothing.
  async fn search_response_received(
    &self,
    conn: &MessageConn,
    frame: Vec<u8>,
    inflating: &Semaphore,
  ) {
    let limits = self.options().limits.frame_limits();
    // a malformed one has been logged by the peek
    let Ok((username, token)) = peek_search_response(&frame, &limits) else {
      return;
    };
    if !self.searches().is_running(token) {
      return tracing::trace!(
        username,
        token,
        peer = conn.handle().username(),
        "search response for no active search, dropped"
      );
    }
    // a malformed one has been logged by the decoder
    if let Some(Ok(PeerMessage::SearchResponse(response))) =
      decode_compressed(frame, inflating, limits).await
    {
      self.search_response(response);
    }
  }

  /// Whether the compressed message `frame` of `code`, which `username` sent, is one something
  /// waits for; one that is not is dropped here, before more than its first fields are inflated.
  /// `followed` tells whether a browse followed the frame's header.
  fn awaited(&self, username: &str, code: PeerCode, frame: &[u8], followed: bool) -> bool {
    match code {
      PeerCode::BrowseResponse if !followed => {
        tracing::debug!("browse response nobody follows, dropped before inflating");
        false
      }
      PeerCode::FolderContentsResponse => {
        let limits = self.options().limits.frame_limits();
        // a malformed one has been logged by the peek
        let Ok(token) = peek_folder_contents_token(frame, &limits) else {
          return false;
        };
        let key = WaitKey::PeerToken {
          code,
          username: username.to_owned(),
          token,
        };
        let awaited = self.waiter().has_wait(&key);
        if !awaited {
          tracing::debug!(
            token,
            "folder contents response nobody waits for, dropped before inflating"
          );
        }
        awaited
      }
      _ => true,
    }
  }

  async fn handle_peer_frame(&self, conn: &MessageConn, arrival: Arrival) {
    let Arrival {
      frame,
      permit: _permit,
      followed,
      inflating,
    } = arrival;
    let limits = self.options().limits.frame_limits();
    // the read loop hands over whole frames, whose header it has checked
    let code = frame::header(FrameKind::Peer, &frame, &limits)
      .ok()
      .flatten()
      .and_then(|header| PeerCode::from_code(header.code));
    if code == Some(PeerCode::SearchResponse) {
      return self.search_response_received(conn, frame, &inflating).await;
    }
    let username = conn.handle().username().unwrap_or_default().to_owned();
    let message = match code.filter(|&code| limits.inflated_bytes(code).is_some()) {
      Some(code) if !self.awaited(&username, code, &frame, followed) => return,
      Some(code) => match decode_compressed(frame, &inflating, limits).await {
        Some(Ok(message)) => message,
        Some(Err(error)) => {
          // the decoder has logged it; the browses waiting for it fail now rather than time out
          if code == PeerCode::BrowseResponse {
            self.waiter().fault_all(
              &WaitKey::Peer { code, username },
              ClientError::Message {
                kind: MessageErrorKind::Read,
                message: "The peer returned an invalid browse response".into(),
                source: Some(Arc::new(error)),
              },
            );
          }
          return;
        }
        // the runtime is shutting down
        None => return,
      },
      None => match PeerMessage::decode_with(&frame, &limits) {
        Ok(message) => message,
        Err(DecodeError::UnknownCode { code }) => {
          return tracing::debug!(code, len = frame.len(), "unhandled peer message");
        }
        Err(error) => {
          // the decoder has logged it; the wait for what it was fails now rather than times out
          return self.fault_unparseable(&username, &frame, &error);
        }
      },
    };
    let code = message.code();
    if let Err(error) = self.handle_peer_message(conn, &username, message).await {
      tracing::warn!(code = code.code(), name = code.name(), %error, "Error handling peer message");
    }
  }

  async fn handle_peer_message(
    &self,
    conn: &MessageConn,
    username: &str,
    message: PeerMessage,
  ) -> Result<(), ClientError> {
    use PeerMessage as M;
    let peer = |code| WaitKey::Peer {
      code,
      username: username.to_owned(),
    };
    let endpoint = conn.handle().remote();
    match message {
      M::BrowseRequest => self.send_browse_response(conn, username, endpoint).await?,
      M::BrowseResponse(response) => {
        // every browse of this user takes the same response
        let key = peer(PeerCode::BrowseResponse);
        if self.waiter().complete_all(&key, Arc::new(response)) == 0 {
          tracing::debug!("browse response nobody waits for, dropped");
        }
      }
      M::SearchRequest(request) => {
        self
          .respond_on(conn, username, request.token, &request.query)
          .await;
      }
      // routed by its token before decoding
      M::SearchResponse(_) => {}
      M::InfoRequest => self.send_user_info(conn, username, endpoint).await?,
      M::InfoResponse(info) => self.user_info_received(username, info),
      M::FolderContentsRequest(request) => {
        self
          .send_folder_contents(conn, username, endpoint, request)
          .await?;
      }
      M::FolderContentsResponse(response) => self.complete_peer(
        WaitKey::PeerToken {
          code: PeerCode::FolderContentsResponse,
          username: username.to_owned(),
          token: response.token,
        },
        response,
      ),
      M::TransferRequest(request) => match request.direction {
        TransferDirection::Upload => self.upload_offered(conn, username, request).await?,
        TransferDirection::Download => {
          self
            .download_requested(conn, username, endpoint, request)
            .await?;
        }
      },
      M::TransferResponse(response) => self.complete_peer(
        WaitKey::PeerToken {
          code: PeerCode::TransferResponse,
          username: username.to_owned(),
          token: response.token(),
        },
        response,
      ),
      M::UploadPlacehold(_) => tracing::debug!("obsolete peer message UploadPlacehold ignored"),
      M::QueueDownload(request) => {
        self
          .queue_requested(conn, username, endpoint, request.filename)
          .await?;
      }
      M::PlaceInQueueResponse(response) => self.place_in_queue_received(username, response),
      M::UploadFailed(failed) => self.download_failed(username, failed),
      M::UploadDenied(denied) => self.download_denied(username, denied),
      M::PlaceInQueueRequest(request) => {
        self
          .try_send_place_in_queue(conn, username, endpoint, &request.filename)
          .await;
      }
      M::UploadQueueNotification => tracing::trace!("upload queue notification ignored"),
    }
    Ok(())
  }

  /// completes the wait under `key`; a reply nobody waits for any more is dropped
  fn complete_peer<T: Send + 'static>(&self, key: WaitKey, value: T) {
    if !self.waiter().complete(&key, value) {
      tracing::debug!(?key, "peer reply nobody waits for, dropped");
    }
  }
}

/// Inflates and decodes the compressed message `frame` once its connection's turn comes, on the
/// blocking pool, within the current span; a large one takes long enough to hold up the tasks
/// sharing a runtime thread. `None` when the runtime shut down before the decode ran.
async fn decode_compressed(
  frame: Vec<u8>,
  inflating: &Semaphore,
  limits: Limits,
) -> Option<Result<PeerMessage, DecodeError>> {
  let _turn = inflating
    .acquire()
    .await
    .expect("a connection's inflate turn is never closed");
  let span = tracing::Span::current();
  let dispatch = tracing::dispatcher::get_default(Clone::clone);
  let decoded = tokio::task::spawn_blocking(move || {
    tracing::dispatcher::with_default(&dispatch, || {
      span.in_scope(|| PeerMessage::decode_with(&frame, &limits))
    })
  })
  .await;
  match decoded {
    Ok(decoded) => Some(decoded),
    Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
    Err(_) => None,
  }
}
