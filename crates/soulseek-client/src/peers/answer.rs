//! How the client answers what a peer asks of it on a message connection: shares, user info
//! and folder contents come from the application's resolvers, and a download the peer wants is
//! queued through the application, or refused when the application shares nothing. A resolver
//! that fails yields an empty answer, no answer or a generic refusal; its error never reaches the
//! peer. An answer is worked out in full, then queued in one write.

use std::net::SocketAddr;

use soulseek_proto::peer::{
  BrowseResponse, FolderContentsRequest, FolderContentsResponse, PlaceInQueueResponse,
  TransferRequest, TransferResponse, UploadDenied, UserInfo,
};
use soulseek_proto::{PeerCode, PeerMessage, RemoteFilename};

use super::dispatch::Turn;
use crate::client::Client;
use crate::conn::{EncodedFrame, HandlerPermit, MessageConn, written};
use crate::error::ClientError;
use crate::resolvers::BrowseReply;

/// What the handler of a peer frame holds while it works out its answer: its place against the
/// bound on frames handled at once and, for a frame handled in order, its turn. Both are let go
/// once the answer is queued on the connection, before it is written. An answer sent from
/// outside a handler holds nothing.
#[derive(Default)]
pub(crate) struct Answering {
  permit: Option<HandlerPermit>,
  turn: Option<Turn>,
}

impl Answering {
  pub(super) fn new(permit: Option<HandlerPermit>, turn: Option<Turn>) -> Self {
    Self { permit, turn }
  }

  /// Queues `messages` on `conn` in one write, lets go of what the handler holds, and waits
  /// until they are on the socket
  pub(crate) async fn send(
    &mut self,
    conn: &MessageConn,
    messages: &[PeerMessage],
  ) -> Result<(), ClientError> {
    let bytes = messages.iter().flat_map(PeerMessage::encode).collect();
    let receipt = conn.send_with_receipt(bytes);
    self.let_go();
    written(receipt?).await?;
    for message in messages {
      let code = message.code();
      tracing::debug!(code = code.code(), name = code.name(), "peer message sent");
    }
    Ok(())
  }

  /// [`Answering::send`] for a message already encoded into `frame`
  pub(crate) async fn send_frame(
    &mut self,
    conn: &MessageConn,
    frame: Vec<u8>,
  ) -> Result<(), ClientError> {
    let receipt = conn.send_with_receipt(frame);
    self.let_go();
    written(receipt?).await
  }

  /// [`Answering::send`] for a frame read from a stream as it is written
  pub(crate) async fn send_encoded(
    &mut self,
    conn: &MessageConn,
    frame: EncodedFrame,
  ) -> Result<(), ClientError> {
    let receipt = conn.send_encoded_with_receipt(frame);
    self.let_go();
    written(receipt?).await
  }

  fn let_go(&mut self) {
    self.permit = None;
    self.turn = None;
  }
}

impl Client {
  /// A peer asks to queue a download of `filename`; it is told its place in the queue, or why
  /// the application refused
  pub(super) async fn queue_requested(
    &self,
    conn: &MessageConn,
    answering: &mut Answering,
    username: &str,
    endpoint: SocketAddr,
    filename: RemoteFilename,
  ) -> Result<(), ClientError> {
    match self
      .try_enqueue_download(username, endpoint, &filename)
      .await
    {
      Ok(()) => {
        self
          .try_send_place_in_queue(conn, answering, username, endpoint, &filename)
          .await;
        Ok(())
      }
      Err(reason) => {
        let denied = PeerMessage::UploadDenied(UploadDenied { filename, reason });
        answering.send(conn, &[denied]).await
      }
    }
  }

  /// A peer asks for a file with the legacy transfer request. It is queued, never started at
  /// once: the answer is "Queued" and the place in the queue, or the refusal twice over.
  pub(super) async fn download_requested(
    &self,
    conn: &MessageConn,
    answering: &mut Answering,
    username: &str,
    endpoint: SocketAddr,
    request: TransferRequest,
  ) -> Result<(), ClientError> {
    let answer = match self
      .try_enqueue_download(username, endpoint, &request.filename)
      .await
    {
      Ok(()) => {
        let queued = PeerMessage::TransferResponse(TransferResponse::Denied {
          token: request.token,
          message: "Queued".into(),
        });
        let place = self
          .resolve_place_in_queue(username, endpoint, &request.filename)
          .await;
        std::iter::once(queued).chain(place).collect::<Vec<_>>()
      }
      Err(reason) => vec![
        PeerMessage::TransferResponse(TransferResponse::Denied {
          token: request.token,
          message: reason.clone(),
        }),
        PeerMessage::UploadDenied(UploadDenied {
          filename: request.filename,
          reason,
        }),
      ],
    };
    answering.send(conn, &answer).await
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
      // with nothing to queue uploads through, nothing is shared
      tracing::debug!(%filename, "upload refused; no upload queue");
      return Err("File not shared.".into());
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
  pub(super) async fn try_send_place_in_queue(
    &self,
    conn: &MessageConn,
    answering: &mut Answering,
    username: &str,
    endpoint: SocketAddr,
    filename: &RemoteFilename,
  ) {
    let Some(response) = self
      .resolve_place_in_queue(username, endpoint, filename)
      .await
    else {
      return;
    };
    if let Err(error) = answering.send(conn, &[response]).await {
      tracing::debug!(%filename, %error, "place in queue not sent");
    }
  }

  /// the message telling the peer its place in our queue for `filename`, when the application
  /// knows it
  async fn resolve_place_in_queue(
    &self,
    username: &str,
    endpoint: SocketAddr,
    filename: &RemoteFilename,
  ) -> Option<PeerMessage> {
    let resolve = self.options().resolvers.place_in_queue_resolver.clone()?;
    let place = match resolve(username.to_owned(), endpoint, filename.clone()).await {
      Ok(place) => place?,
      Err(error) => {
        tracing::warn!(%filename, %error, "Failed to resolve place in queue");
        return None;
      }
    };
    Some(PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse {
      filename: filename.clone(),
      place,
    }))
  }

  pub(super) async fn send_browse_response(
    &self,
    conn: &MessageConn,
    answering: &mut Answering,
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
        answering
          .send(conn, &[PeerMessage::BrowseResponse(response)])
          .await?;
      }
      BrowseReply::Encoded(frame) => {
        answering.send_encoded(conn, frame).await?;
        tracing::debug!(code = PeerCode::BrowseResponse.code(), "peer message sent");
      }
    }
    tracing::debug!("Share contents sent");
    Ok(())
  }

  pub(super) async fn send_user_info(
    &self,
    conn: &MessageConn,
    answering: &mut Answering,
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
    answering
      .send(conn, &[PeerMessage::InfoResponse(info)])
      .await?;
    tracing::debug!("User info sent");
    Ok(())
  }

  pub(super) async fn send_folder_contents(
    &self,
    conn: &MessageConn,
    answering: &mut Answering,
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
    let folder = PeerMessage::FolderContentsResponse(FolderContentsResponse {
      token: request.token,
      directory_name: request.directory_name,
      directories,
    });
    answering.send(conn, &[folder]).await?;
    tracing::debug!(token = request.token, "Folder contents sent");
    Ok(())
  }
}
