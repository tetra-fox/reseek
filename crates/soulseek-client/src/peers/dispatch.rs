//! What the client does with each message a peer sends on a message connection: answer it from
//! the application's resolvers, complete the wait of the request it answers, or raise the event
//! it carries. A search response is checked against our searches before anything else of it is
//! inflated. Every frame is handled on a task of its own, with a bound on how many run at once
//! across all connections; at the bound a connection's reader waits before reading its next
//! frame, so a stuck resolver slows the peers sending to it and loses nothing.

use std::sync::Arc;

use soulseek_proto::{DecodeError, PeerCode, PeerMessage, TransferDirection, peek_search_response};
use tokio::sync::{OwnedSemaphorePermit, watch};
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

impl Client {
  /// Starts the read loop of a peer message connection. A browse response's header completes
  /// the wait for it before the payload arrives, and its progress is published as it does.
  pub(crate) fn start_peer_reading(&self, conn: &MessageConn) {
    let client = self.downgrade();
    let reader = conn.clone();
    let mut browse: Option<watch::Sender<usize>> = None;
    let gate = self.peers().handlers.clone();
    conn.start_reading_gated(gate, move |event, permit| match event {
      ReadEvent::Header { len, code } if code == PeerCode::BrowseResponse.code() => {
        let (progress, received) = watch::channel(0);
        browse = Some(progress);
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
          if client.waiter().complete_all(&key, incoming) == 0 {
            tracing::trace!("browse response nobody follows");
          }
        }
      }
      ReadEvent::Progress { code, current, .. } if code == PeerCode::BrowseResponse.code() => {
        if let Some(progress) = &browse {
          progress.send_replace(current);
        }
      }
      ReadEvent::Frame(frame) => {
        if let Some(client) = client.upgrade() {
          client.dispatch_peer_frame(&reader, frame, permit);
        }
      }
      ReadEvent::Header { .. } | ReadEvent::Progress { .. } => {}
    });
  }

  /// Hands `frame` to a task of its own, which keeps `permit` until the frame is handled
  fn dispatch_peer_frame(
    &self,
    conn: &MessageConn,
    frame: Vec<u8>,
    permit: Option<OwnedSemaphorePermit>,
  ) {
    let client = self.clone();
    let conn = conn.clone();
    let span = conn.handle().span().clone();
    tokio::spawn(
      async move { client.handle_peer_frame(&conn, &frame, permit).await }.instrument(span),
    );
  }

  /// A search response goes to the search its token names. Only the username and token are
  /// inflated first, so a response for a search that is over costs next to nothing.
  fn search_response_received(&self, conn: &MessageConn, frame: &[u8]) {
    let limits = self.options().limits.frame_limits();
    // a malformed one has been logged by the peek
    let Ok((username, token)) = peek_search_response(frame, &limits) else {
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
    if let Ok(PeerMessage::SearchResponse(response)) = PeerMessage::decode_with(frame, &limits) {
      self.search_response(response);
    }
  }

  async fn handle_peer_frame(
    &self,
    conn: &MessageConn,
    frame: &[u8],
    _permit: Option<OwnedSemaphorePermit>,
  ) {
    if frame.get(4..8) == Some(&PeerCode::SearchResponse.code().to_le_bytes()) {
      return self.search_response_received(conn, frame);
    }
    let username = conn.handle().username().unwrap_or_default().to_owned();
    let limits = self.options().limits.frame_limits();
    let message = match PeerMessage::decode_with(frame, &limits) {
      Ok(message) => message,
      Err(DecodeError::UnknownCode { code }) => {
        return tracing::debug!(code, len = frame.len(), "unhandled peer message");
      }
      Err(error) => {
        // the decoder has logged it; the waits for what it was fail now rather than time out
        if frame.get(4..8) == Some(&PeerCode::BrowseResponse.code().to_le_bytes()) {
          self.waiter().fault_all(
            &WaitKey::Peer {
              code: PeerCode::BrowseResponse,
              username,
            },
            ClientError::Message {
              kind: MessageErrorKind::Read,
              message: "The peer returned an invalid browse response".into(),
              source: Some(std::sync::Arc::new(error)),
            },
          );
        } else {
          self.fault_unparseable(&username, frame, &error);
        }
        return;
      }
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
