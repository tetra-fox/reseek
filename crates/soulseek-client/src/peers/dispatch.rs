//! What the client does with each message a peer sends on a message connection: answer it from
//! the application's resolvers, complete the wait of the request it answers, or raise the event
//! it carries. Every frame is handled on a task of its own, with a bound on how many run at once
//! across all connections and a smaller one per connection; at a bound a connection's reader
//! waits before reading its next frame, so a stuck resolver slows the peers sending to it and
//! loses nothing.
//!
//! A frame is read in only when its code is one a message has and its length is within what
//! that message may need; any other is skipped unread. Anyone can connect and send answers, so
//! an answer is read in only when something of ours waits for one of its kind from the user of
//! the connection: a browse response when a browse follows its header, user info or a folder
//! when we asked that user for one, a search response while a search of ours runs. The others
//! are skipped unread from their header. Browse, folder and search responses are compressed; a
//! folder or search response is inflated only once the token at its start, inflated alone, is
//! one we wait for, and a search response is taken only under the name of the connection it
//! came on. A connection inflates one message at a time, on the blocking pool.

use std::sync::Arc;

use soulseek_proto::frame::{self, FrameKind};
use soulseek_proto::{
  DecodeError, Limits, PeerCode, PeerMessage, TransferDirection, peek_folder_contents_token,
  peek_search_response,
};
use tokio::sync::{Semaphore, watch};
use tracing::Instrument;

use crate::client::Client;
use crate::conn::{FrameBody, HandlerPermit, MessageConn, ReadEvent};
use crate::error::{ClientError, MessageErrorKind};
use crate::log::LogLimiter;
use crate::waiter::WaitKey;

static OVERSIZE: LogLimiter = LogLimiter::new();
static UNASKED: LogLimiter = LogLimiter::new();
static MISNAMED: LogLimiter = LogLimiter::new();

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
  permit: Option<HandlerPermit>,
  /// the connection's turn to inflate, one compressed message at a time
  inflating: Arc<Semaphore>,
}

impl Client {
  /// Starts the read loop of a peer message connection. A browse response's header completes
  /// the wait for it before the payload arrives, and its progress is published as it does; one
  /// no browse follows is skipped unread, like any frame whose header rules it out.
  pub(crate) fn start_peer_reading(&self, conn: &MessageConn) {
    let client = self.downgrade();
    let reader = conn.clone();
    let mut browse: Option<watch::Sender<usize>> = None;
    let inflating = Arc::new(Semaphore::new(1));
    let gate = self.peers().handlers.clone();
    conn.start_reading_gated(gate, move |event, permit| match event {
      ReadEvent::Header { len, code } if code == PeerCode::BrowseResponse.code() => {
        let (progress, received) = watch::channel(0);
        let followed = client
          .upgrade()
          .is_some_and(|client| client.browse_arriving(&reader, len, received));
        browse = followed.then_some(progress);
        if followed {
          FrameBody::Read
        } else {
          FrameBody::Skip
        }
      }
      // nothing is handled once the client is gone
      ReadEvent::Header { len, code } => client.upgrade().map_or(FrameBody::Skip, |client| {
        client.frame_body(&reader, len, code)
      }),
      ReadEvent::Progress { code, current, .. } if code == PeerCode::BrowseResponse.code() => {
        if let Some(progress) = &browse {
          progress.send_replace(current);
        }
        FrameBody::Read
      }
      ReadEvent::Frame(frame) => {
        if let Some(client) = client.upgrade() {
          let arrival = Arrival {
            frame,
            permit,
            inflating: inflating.clone(),
          };
          client.dispatch_peer_frame(&reader, arrival);
        }
        FrameBody::Read
      }
      ReadEvent::Progress { .. } => FrameBody::Read,
    });
  }

  /// Whether the payload of a frame `len` long of `code`, other than a browse response, is read
  /// in: one of a code no message has, an answer nothing of ours waits for, or one longer than
  /// its message may be is skipped. User info skipped as too long fails the request waiting for
  /// it.
  fn frame_body(&self, conn: &MessageConn, len: u32, code: u32) -> FrameBody {
    let Some(known) = PeerCode::from_code(code) else {
      tracing::debug!(code, len, "unhandled peer message");
      return FrameBody::Skip;
    };
    if !self.answer_expected(conn, known) {
      if let Some(suppressed) = UNASKED.allow() {
        tracing::debug!(
          code,
          name = known.name(),
          len,
          suppressed,
          "peer answer nobody asked for, skipped"
        );
      }
      return FrameBody::Skip;
    }
    let limit = self
      .options()
      .limits
      .frame_limits()
      .peer_message_bytes(known);
    if len <= limit {
      return FrameBody::Read;
    }
    if let Some(suppressed) = OVERSIZE.allow() {
      tracing::debug!(
        code,
        name = known.name(),
        len,
        limit,
        suppressed,
        "peer message longer than its limit, skipped"
      );
    }
    if known == PeerCode::InfoResponse
      && let Some(username) = conn.handle().username()
    {
      let error = ClientError::Message {
        kind: MessageErrorKind::Read,
        message: "user info too large".into(),
        source: None,
      };
      self.user_info_unreadable(username, error);
    }
    FrameBody::Skip
  }

  /// Whether something of ours waits for a message of `code` from the user of `conn`. User info
  /// and folders come only as answers to our requests, search responses only while a search of
  /// ours runs; any other message may come unasked.
  fn answer_expected(&self, conn: &MessageConn, code: PeerCode) -> bool {
    let username = conn.handle().username();
    match code {
      PeerCode::InfoResponse => username.is_some_and(|username| self.user_info_awaited(username)),
      PeerCode::FolderContentsResponse => {
        username.is_some_and(|username| self.folder_requests().any(username))
      }
      PeerCode::SearchResponse => self.searches().any_running(),
      _ => true,
    }
  }

  /// Hands the browses of the connection's user the start of the browse response `len` long
  /// whose payload is arriving; whether one follows it. One too large to accept fails them.
  fn browse_arriving(
    &self,
    conn: &MessageConn,
    len: u32,
    received: watch::Receiver<usize>,
  ) -> bool {
    let Some(username) = conn.handle().username() else {
      tracing::debug!("browse response from a connection with no name, skipped");
      return false;
    };
    let key = WaitKey::BrowseResponseConnection {
      username: username.to_owned(),
    };
    let limit = self
      .options()
      .limits
      .frame_limits()
      .peer_message_bytes(PeerCode::BrowseResponse);
    if len > limit {
      if let Some(suppressed) = OVERSIZE.allow() {
        tracing::debug!(
          len,
          limit,
          suppressed,
          "browse response longer than its limit, skipped"
        );
      }
      let error = ClientError::Message {
        kind: MessageErrorKind::Read,
        message: "browse response too large".into(),
        source: None,
      };
      self.waiter().fault_all(&key, error);
      return false;
    }
    let incoming = IncomingBrowse {
      length: len,
      connection: conn.clone(),
      received,
    };
    // every browse of this user follows the response
    let followed = self.waiter().complete_all(&key, incoming) > 0;
    if !followed {
      tracing::debug!(len, "browse response nobody follows, skipped");
    }
    followed
  }

  /// Hands the frame to a task of its own, which keeps its permit until the frame is handled
  fn dispatch_peer_frame(&self, conn: &MessageConn, arrival: Arrival) {
    let client = self.clone();
    let conn = conn.clone();
    let span = conn.handle().span().clone();
    tokio::spawn(async move { client.handle_peer_frame(&conn, arrival).await }.instrument(span));
  }

  /// A search response goes to the search its token names, when it names the user whose
  /// connection it came on. Only the username and token are inflated first, so a response for a
  /// search that is over costs next to nothing.
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
    let peer = conn.handle().username();
    if peer != Some(username.as_str()) {
      if let Some(suppressed) = MISNAMED.allow() {
        tracing::debug!(
          username,
          token,
          peer,
          suppressed,
          "search response under a name other than its connection's, dropped"
        );
      }
      return;
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
  /// A browse response that reaches here was followed from its header.
  fn awaited(&self, username: &str, code: PeerCode, frame: &[u8]) -> bool {
    match code {
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
      Some(code) if !self.awaited(&username, code, &frame) => return,
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
        // the decoder has logged it; the wait for what it was fails now rather than times out
        Err(error) if code == Some(PeerCode::InfoResponse) => {
          let error = ClientError::message_read("The peer returned invalid user info", error);
          return self.user_info_unreadable(&username, error);
        }
        Err(error) => return self.fault_unparseable(&username, &frame, &error),
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
        TransferDirection::Upload => {
          self
            .upload_offered(conn, username, endpoint, request)
            .await?;
        }
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
