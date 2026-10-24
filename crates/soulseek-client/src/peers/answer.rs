//! How the client answers what a peer asks of it on a message connection: shares, user info
//! and folder contents come from the application's resolvers, a download the peer wants is
//! queued through the application, and a file a peer offers to upload is taken by the download
//! waiting for it or refused. A resolver that fails yields an empty answer, no answer or a
//! generic refusal; its error never reaches the peer.

use std::net::SocketAddr;

use soulseek_proto::peer::{
  BrowseResponse, FolderContentsRequest, FolderContentsResponse, PlaceInQueueResponse,
  TransferRequest, TransferResponse, UploadDenied, UserInfo,
};
use soulseek_proto::{PeerCode, PeerMessage, RemoteFilename};

use crate::client::Client;
use crate::conn::MessageConn;
use crate::error::ClientError;
use crate::resolvers::BrowseReply;
use crate::waiter::WaitKey;

impl Client {
  /// A peer asks to queue a download of `filename`; it is told its place in the queue, or why
  /// the application refused
  pub(super) async fn queue_requested(
    &self,
    conn: &MessageConn,
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
          .try_send_place_in_queue(conn, username, endpoint, &filename)
          .await;
      }
      Err(reason) => {
        write_peer(
          conn,
          PeerMessage::UploadDenied(UploadDenied { filename, reason }),
        )
        .await?;
      }
    }
    Ok(())
  }

  /// An uploader is about to send us a file. A download waiting for it takes the request;
  /// anything else is refused.
  pub(super) async fn upload_offered(
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
  pub(super) async fn download_requested(
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
  pub(super) async fn try_send_place_in_queue(
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

  pub(super) async fn send_browse_response(
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

  pub(super) async fn send_user_info(
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

  pub(super) async fn send_folder_contents(
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
async fn write_peer(conn: &MessageConn, message: PeerMessage) -> Result<(), ClientError> {
  let code = message.code();
  conn.write(message.encode()).await?;
  tracing::debug!(code = code.code(), name = code.name(), "peer message sent");
  Ok(())
}
