//! What the client does with each message a peer sends on a message connection: answer it from
//! the application's resolvers, complete the wait of the request it answers, or raise the event
//! it carries. A search response is checked against our searches before anything else of it is
//! inflated; every other frame is handled on a task of its own, with a bound on how many run at
//! once so a stuck resolver cannot pile up work without end.

use std::net::SocketAddr;

use soulseek_proto::peer::{
  BrowseResponse, FolderContentsRequest, FolderContentsResponse, PlaceInQueueResponse,
  UploadDenied, UserInfo,
};
use soulseek_proto::peer::{TransferRequest, TransferResponse};
use soulseek_proto::{
  DecodeError, PeerCode, PeerMessage, RemoteFilename, TransferDirection, peek_search_response,
};
use tokio::sync::{OwnedSemaphorePermit, watch};
use tracing::Instrument;

use crate::client::Client;
use crate::conn::{MessageConn, ReadEvent};
use crate::error::{ClientError, MessageErrorKind};
use crate::events::Event;
use crate::log::LogLimiter;
use crate::resolvers::BrowseReply;
use crate::waiter::WaitKey;

static HANDLERS_BUSY: LogLimiter = LogLimiter::new();

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
    conn.start_reading(move |event| match event {
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
          if !client.waiter().complete(&key, incoming) {
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
          client.dispatch_peer_frame(&reader, frame);
        }
      }
      ReadEvent::Header { .. } | ReadEvent::Progress { .. } => {}
    });
  }

  fn dispatch_peer_frame(&self, conn: &MessageConn, frame: Vec<u8>) {
    let code = frame
      .get(4..8)
      .map(|code| u32::from_le_bytes([code[0], code[1], code[2], code[3]]));
    if code == Some(PeerCode::SearchResponse.code()) {
      return self.search_response_received(conn, &frame);
    }
    let Some(permit) = self.peers().handlers.clone().try_acquire_owned().ok() else {
      if let Some(suppressed) = HANDLERS_BUSY.allow() {
        tracing::warn!(
          code,
          suppressed,
          "too many peer messages being handled, message dropped"
        );
      }
      return;
    };
    let client = self.clone();
    let conn = conn.clone();
    let span = conn.handle().span().clone();
    tokio::spawn(
      async move { client.handle_peer_frame(&conn, &frame, permit).await }.instrument(span),
    );
  }

  /// Our searches take the response; this layer has none of its own, so a response is checked
  /// for its token only and dropped
  fn search_response_received(&self, conn: &MessageConn, frame: &[u8]) {
    let limits = self.options().limits.frame_limits();
    // a malformed one has been logged by the peek
    if let Ok((username, token)) = peek_search_response(frame, &limits) {
      tracing::trace!(
        username,
        token,
        peer = conn.handle().username(),
        "search response for no active search, dropped"
      );
    }
  }

  async fn handle_peer_frame(
    &self,
    conn: &MessageConn,
    frame: &[u8],
    _permit: OwnedSemaphorePermit,
  ) {
    let username = conn.handle().username().unwrap_or_default().to_owned();
    let limits = self.options().limits.frame_limits();
    let message = match PeerMessage::decode_with(frame, &limits) {
      Ok(message) => message,
      Err(DecodeError::UnknownCode { code }) => {
        return tracing::debug!(code, len = frame.len(), "unhandled peer message");
      }
      Err(error) => {
        // the decoder has logged it; a browse response somebody waits for fails that wait
        if frame.get(4..8) == Some(&PeerCode::BrowseResponse.code().to_le_bytes()) {
          self.waiter().fault(
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
    let file = |code, filename: &RemoteFilename| WaitKey::PeerFile {
      code,
      username: username.to_owned(),
      filename: filename.clone(),
    };
    let endpoint = conn.handle().remote();
    match message {
      M::BrowseRequest => self.send_browse_response(conn, username, endpoint).await?,
      M::BrowseResponse(response) => self.complete_peer(peer(PeerCode::BrowseResponse), response),
      M::SearchRequest(request) => {
        self
          .respond_on(conn, username, request.token, &request.query)
          .await;
      }
      // routed by its token before decoding
      M::SearchResponse(_) => {}
      M::InfoRequest => self.send_user_info(conn, username, endpoint).await?,
      M::InfoResponse(info) => self.complete_peer(peer(PeerCode::InfoResponse), info),
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
        match self
          .try_enqueue_download(username, endpoint, &request.filename)
          .await
        {
          Ok(()) => {
            self
              .try_send_place_in_queue(conn, username, endpoint, &request.filename)
              .await;
          }
          Err(reason) => {
            write_peer(
              conn,
              PeerMessage::UploadDenied(UploadDenied {
                filename: request.filename,
                reason,
              }),
            )
            .await?;
          }
        }
      }
      M::PlaceInQueueResponse(response) => {
        let key = file(PeerCode::PlaceInQueueResponse, &response.filename);
        self.complete_peer(key, response);
      }
      M::UploadFailed(failed) => {
        tracing::debug!(filename = %failed.filename, "download reported as failed by the uploader");
        self.waiter().fault(
          &file(PeerCode::TransferRequest, &failed.filename),
          ClientError::TransferReportedFailed(
            "Download reported as failed by remote client".into(),
          ),
        );
        self.emit(Event::DownloadFailed {
          username: username.to_owned(),
          filename: failed.filename,
        });
      }
      M::UploadDenied(denied) => {
        tracing::debug!(filename = %denied.filename, reason = denied.reason, "download denied by the uploader");
        self.waiter().fault(
          &file(PeerCode::TransferRequest, &denied.filename),
          ClientError::TransferRejected(denied.reason.clone()),
        );
        self.emit(Event::DownloadDenied {
          username: username.to_owned(),
          filename: denied.filename,
          message: denied.reason,
        });
      }
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

  /// An uploader is about to send us a file. A download waiting for it takes the request;
  /// anything else is refused.
  async fn upload_offered(
    &self,
    conn: &MessageConn,
    username: &str,
    request: TransferRequest,
  ) -> Result<(), ClientError> {
    let key = WaitKey::PeerFile {
      code: PeerCode::TransferRequest,
      username: username.to_owned(),
      filename: request.filename.clone(),
    };
    let token = request.token;
    if let Err(request) = self.waiter().offer(&key, request) {
      tracing::debug!(filename = %request.filename, token, "Rejecting unknown upload");
      write_peer(
        conn,
        PeerMessage::TransferResponse(TransferResponse::Denied {
          token,
          message: "Cancelled".into(),
        }),
      )
      .await?;
    }
    Ok(())
  }

  /// A peer asks for a file with the legacy transfer request. It is queued, never started at
  /// once: the answer is "Queued" and the place in the queue, or the refusal twice over.
  async fn download_requested(
    &self,
    conn: &MessageConn,
    username: &str,
    endpoint: SocketAddr,
    request: TransferRequest,
  ) -> Result<(), ClientError> {
    match self
      .try_enqueue_download(username, endpoint, &request.filename)
      .await
    {
      Ok(()) => {
        write_peer(
          conn,
          PeerMessage::TransferResponse(TransferResponse::Denied {
            token: request.token,
            message: "Queued".into(),
          }),
        )
        .await?;
        self
          .try_send_place_in_queue(conn, username, endpoint, &request.filename)
          .await;
      }
      Err(reason) => {
        write_peer(
          conn,
          PeerMessage::TransferResponse(TransferResponse::Denied {
            token: request.token,
            message: reason.clone(),
          }),
        )
        .await?;
        write_peer(
          conn,
          PeerMessage::UploadDenied(UploadDenied {
            filename: request.filename,
            reason,
          }),
        )
        .await?;
      }
    }
    Ok(())
  }

  /// Asks the application to queue an upload; the refusal reason the peer sees, which never
  /// carries anything internal
  async fn try_enqueue_download(
    &self,
    username: &str,
    endpoint: SocketAddr,
    filename: &RemoteFilename,
  ) -> Result<(), String> {
    let Some(enqueue) = self.options().resolvers.enqueue_download.clone() else {
      return Ok(());
    };
    match enqueue(username.to_owned(), endpoint, filename.clone()).await {
      Ok(()) => Ok(()),
      Err(ClientError::DownloadEnqueue(reason)) => {
        tracing::debug!(%filename, reason, "upload refused");
        Err(reason)
      }
      Err(error) => {
        tracing::warn!(%filename, %error, "Failed to invoke QueueDownload action");
        Err("Enqueue failed due to internal error".into())
      }
    }
  }

  /// tells the peer its place in our queue for `filename`, when the application knows it
  async fn try_send_place_in_queue(
    &self,
    conn: &MessageConn,
    username: &str,
    endpoint: SocketAddr,
    filename: &RemoteFilename,
  ) {
    let Some(resolve) = self.options().resolvers.place_in_queue_resolver.clone() else {
      return;
    };
    let place = match resolve(username.to_owned(), endpoint, filename.clone()).await {
      Ok(Some(place)) => place,
      Ok(None) => return,
      Err(error) => {
        return tracing::warn!(%filename, %error, "Failed to resolve place in queue");
      }
    };
    let response = PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: filename.clone(),
      place,
    });
    if let Err(error) = write_peer(conn, response).await {
      tracing::debug!(%filename, %error, "place in queue not sent");
    }
  }

  async fn send_browse_response(
    &self,
    conn: &MessageConn,
    username: &str,
    endpoint: SocketAddr,
  ) -> Result<(), ClientError> {
    let empty = || {
      BrowseReply::Response(BrowseResponse {
        directories: Vec::new(),
        locked_directories: Vec::new(),
      })
    };
    let reply = match self.options().resolvers.browse_response_resolver.clone() {
      None => empty(),
      Some(resolve) => resolve(username.to_owned(), endpoint)
        .await
        .unwrap_or_else(|error| {
          tracing::warn!(%error, "Failed to resolve browse response; sending an empty one");
          empty()
        }),
    };
    match reply {
      BrowseReply::Response(response) => {
        write_peer(conn, PeerMessage::BrowseResponse(response)).await?;
      }
      BrowseReply::Encoded(frame) => {
        conn.write_encoded(frame).await?;
        tracing::debug!(code = PeerCode::BrowseResponse.code(), "peer message sent");
      }
    }
    tracing::debug!("Share contents sent");
    Ok(())
  }

  async fn send_user_info(
    &self,
    conn: &MessageConn,
    username: &str,
    endpoint: SocketAddr,
  ) -> Result<(), ClientError> {
    let empty = || UserInfo {
      description: String::new(),
      picture: None,
      upload_slots: 0,
      queue_length: 0,
      has_free_upload_slot: false,
      upload_permitted: None,
    };
    let mut info = match self.options().resolvers.user_info_resolver.clone() {
      None => empty(),
      Some(resolve) => resolve(username.to_owned(), endpoint)
        .await
        .unwrap_or_else(|error| {
          tracing::warn!(%error, "Failed to resolve user info; sending an empty one");
          empty()
        }),
    };
    // everyone may upload to us unless the application says otherwise
    info.upload_permitted.get_or_insert(1);
    write_peer(conn, PeerMessage::InfoResponse(info)).await?;
    tracing::debug!("User info sent");
    Ok(())
  }

  async fn send_folder_contents(
    &self,
    conn: &MessageConn,
    username: &str,
    endpoint: SocketAddr,
    request: FolderContentsRequest,
  ) -> Result<(), ClientError> {
    let Some(resolve) = self.options().resolvers.directory_contents_resolver.clone() else {
      return Ok(());
    };
    let directories = match resolve(
      username.to_owned(),
      endpoint,
      request.token,
      request.directory_name.clone(),
    )
    .await
    {
      Ok(Some(directories)) => directories,
      Ok(None) => return Ok(()),
      Err(error) => {
        tracing::warn!(directory = %request.directory_name, %error, "Failed to resolve folder contents");
        return Ok(());
      }
    };
    write_peer(
      conn,
      PeerMessage::FolderContentsResponse(FolderContentsResponse {
        token: request.token,
        directory_name: request.directory_name,
        directories,
      }),
    )
    .await?;
    tracing::debug!(token = request.token, "Folder contents sent");
    Ok(())
  }
}

/// writes `message` on a peer connection and waits until it is on the socket
pub(crate) async fn write_peer(
  conn: &MessageConn,
  message: PeerMessage,
) -> Result<(), ClientError> {
  let code = message.code();
  conn.write(message.encode()).await?;
  tracing::debug!(code = code.code(), name = code.name(), "peer message sent");
  Ok(())
}
