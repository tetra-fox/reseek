//! What peers say about the files we download from them: the offer to start sending one, a
//! refusal or a failure, our place in their queue. A refusal or failure ends the download
//! wherever it is. The offer of a file a user sends unasked is held while the application
//! decides whether to take it. Also asking a peer for our place in its queue, and failing the
//! waits of a download whose peer sent one of these messages in a form we cannot read.

use std::net::SocketAddr;

use soulseek_proto::peer::{
  PlaceInQueueRequest, PlaceInQueueResponse, TransferRequest, TransferResponse, UploadDenied,
  UploadFailed,
};
use soulseek_proto::{DecodeError, PeerCode, PeerMessage, RemoteFilename, TransferDirection};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::client::{Client, failed, non_blank};
use crate::conn::MessageConn;
use crate::error::{ClientError, MessageErrorKind};
use crate::events::Event;
use crate::peers::write_peer;
use super::RemoteFailure;
use crate::waiter::WaitKey;

impl Client {
  /// An uploader is about to send us a file. A download of that file from that user takes the
  /// offer when it waits for one. Any other file is one the user sends unasked: the application
  /// decides whether to take it, and a file it does not take is refused, which is also how a
  /// canceled download tells the uploader to drop it.
  pub(crate) async fn upload_offered(
    &self,
    conn: &MessageConn,
    username: &str,
    endpoint: SocketAddr,
    request: TransferRequest,
  ) -> Result<(), ClientError> {
    let token = request.token;
    if self.transfers().has_download(username, &request.filename) {
      let key = offer_key(username, &request.filename);
      if let Err(request) = self.waiter().offer(&key, request) {
        tracing::debug!(filename = %request.filename, token, "upload offer for a download not waiting for one, ignored");
      }
      return Ok(());
    }
    let filename = request.filename.clone();
    if self.receive_offer(username, endpoint, request).await {
      self.expire_offer(conn.clone(), username, filename, token);
      return Ok(());
    }
    refuse_offer(conn, token).await
  }

  /// Asks the application whether to take a file `username` offers unasked, holding the offer
  /// meanwhile; true when it started a download of the file, which takes the offer
  async fn receive_offer(
    &self,
    username: &str,
    endpoint: SocketAddr,
    offer: TransferRequest,
  ) -> bool {
    let filename = offer.filename.clone();
    let token = offer.token;
    let Some(receive) = self.options().resolvers.receive_offer.clone() else {
      tracing::debug!(username, %filename, token, "file offered unasked refused; nothing receives such files");
      return false;
    };
    let Some(size) = offer.file_size else {
      tracing::debug!(username, %filename, token, "file offered unasked without a size refused");
      return false;
    };
    if !self.transfers().hold_offer(username, offer) {
      tracing::debug!(username, %filename, token, "file offered unasked again while its offer is held, refused");
      return false;
    }
    match receive(username.to_owned(), endpoint, filename.clone(), size).await {
      Ok(()) => {
        tracing::debug!(username, %filename, token, size, "file offered unasked accepted");
        return true;
      }
      Err(ClientError::TransferRejected(reason)) => {
        tracing::debug!(username, %filename, token, reason, "file offered unasked refused");
      }
      Err(error) => {
        tracing::warn!(username, %filename, token, %error, "file offered unasked not received");
      }
    }
    self.transfers().release_offer(username, &filename, token);
    false
  }

  /// Refuses the offer of `filename` with `token` from `username` once the unasked offer timeout has
  /// passed, unless a download took it by then
  fn expire_offer(&self, conn: MessageConn, username: &str, filename: RemoteFilename, token: u32) {
    let held = self.options().unasked_offer_timeout;
    let client = self.downgrade();
    let lifetime = self.cancellation().clone();
    let username = username.to_owned();
    tokio::spawn(
      async move {
        tokio::select! {
          () = lifetime.cancelled() => return,
          () = tokio::time::sleep(held) => {}
        }
        let Some(client) = client.upgrade() else {
          return;
        };
        if !client
          .transfers()
          .release_offer(&username, &filename, token)
        {
          return;
        }
        tracing::debug!(username, %filename, token, "file offered unasked not taken in time, refused");
        if let Err(error) = refuse_offer(&conn, token).await {
          tracing::debug!(%error, "refusal of an offer not taken in time not sent");
        }
      }
      .in_current_span(),
    );
  }

  /// The uploader refused a file we asked for: the download ends rejected with its reason
  pub(crate) fn download_denied(&self, username: &str, denied: UploadDenied) {
    tracing::debug!(filename = %denied.filename, reason = denied.reason, "download denied by the uploader");
    let error = ClientError::TransferRejected(denied.reason.clone());
    if self.end_download(username, &denied.filename, error) {
      self.emit(Event::DownloadDenied {
        username: username.to_owned(),
        filename: denied.filename,
        message: denied.reason,
      });
    }
  }

  /// The uploader reports that sending a file to us failed: the download ends with that
  pub(crate) fn download_failed(&self, username: &str, failed: UploadFailed) {
    tracing::debug!(filename = %failed.filename, "download reported as failed by the uploader");
    let error =
      ClientError::TransferReportedFailed("Download reported as failed by remote client".into());
    if self.end_download(username, &failed.filename, error) {
      self.emit(Event::DownloadFailed {
        username: username.to_owned(),
        filename: failed.filename,
      });
    }
  }

  /// Ends the download of `filename` from `username` with `error`, at whatever stage it is,
  /// unless it has not asked for the file yet; false when the report was set aside for that
  fn end_download(&self, username: &str, filename: &RemoteFilename, error: ClientError) -> bool {
    match self
      .transfers()
      .fail_download(username, filename, error.clone())
    {
      RemoteFailure::Ended => {
        self.waiter().fault(&offer_key(username, filename), error);
      }
      RemoteFailure::NotAsked => {
        tracing::debug!(%filename, %error, "refusal or failure for a download that has not asked yet, ignored");
        return false;
      }
      RemoteFailure::Untracked => tracing::debug!(%filename, "no such download"),
    }
    true
  }

  /// Our place in the uploader's queue: the answer to our question, or news the uploader sent
  /// on its own, which is raised as an event
  pub(crate) fn place_in_queue_received(&self, username: &str, response: PlaceInQueueResponse) {
    let key = WaitKey::PeerFile {
      code: PeerCode::PlaceInQueueResponse,
      username: username.to_owned(),
      filename: response.filename.clone(),
    };
    if let Err(response) = self.waiter().offer(&key, response) {
      tracing::debug!(filename = %response.filename, place = response.place, "place in queue received");
      self.emit(Event::PlaceInQueueReceived {
        username: username.to_owned(),
        filename: response.filename,
        place: response.place,
      });
    }
  }

  /// Asks `username` for our place in its queue for `filename`, which we must be downloading.
  /// Peers count from 1; some say 0 while the file is being sent.
  pub async fn get_download_place_in_queue(
    &self,
    username: &str,
    filename: impl Into<RemoteFilename>,
    cancel: &CancellationToken,
  ) -> Result<u32, ClientError> {
    let filename = filename.into();
    non_blank("username", username)?;
    non_blank("filename", filename.as_str())?;
    let verb = format!("get the place in queue of {filename} from {username}");
    self.require_logged_in(&verb)?;
    if !self.transfers().has_download(username, &filename) {
      return Err(ClientError::TransferNotFound {
        username: username.to_owned(),
        filename: filename.to_string(),
      });
    }
    let answer = self.waiter().wait_from_first_poll::<PlaceInQueueResponse>(
      WaitKey::PeerFile {
        code: PeerCode::PlaceInQueueResponse,
        username: username.to_owned(),
        filename: filename.clone(),
      },
      Some(self.waiter().default_timeout()),
      cancel.clone(),
    );
    let ask = async {
      let endpoint = self.get_user_endpoint(username).await?;
      let conn = self
        .message_connection(username, Some(endpoint), None, cancel)
        .await?;
      write_peer(
        &conn,
        PeerMessage::PlaceInQueueRequest(PlaceInQueueRequest { filename }),
      )
      .await?;
      Ok(answer.await?.place)
    };
    ask.await.map_err(|error| failed(&verb, error))
  }

  /// A transfer message from `username` that does not decode fails the wait its fields name, as
  /// far as they can be read, so the download waiting for it ends at once
  pub(crate) fn fault_unparseable(&self, username: &str, frame: &[u8], error: &DecodeError) {
    let Some((key, name)) = unparseable_key(username, frame) else {
      return;
    };
    // like a refusal or failure that decodes, one for a download that has not asked yet is
    // about an earlier download of the file
    if let ("UploadDenied" | "UploadFailed", WaitKey::PeerFile { filename, .. }) = (name, &key)
      && !self.transfers().has_asked(username, filename)
    {
      return tracing::debug!(name, %filename, "unparseable refusal or failure for a download that has not asked yet, ignored");
    }
    let error = ClientError::Message {
      kind: MessageErrorKind::Read,
      message: format!("peer sent an unparseable {name}"),
      source: Some(std::sync::Arc::new(error.clone())),
    };
    if self.waiter().fault(&key, error) {
      tracing::debug!(?key, "wait failed by an unparseable message");
    }
  }
}

/// tells the sender of a file we do not take to drop it
async fn refuse_offer(conn: &MessageConn, token: u32) -> Result<(), ClientError> {
  write_peer(
    conn,
    PeerMessage::TransferResponse(TransferResponse::Denied {
      token,
      message: "Cancelled".into(),
    }),
  )
  .await
}

/// the wait a download holds for its uploader's offer
pub(super) fn offer_key(username: &str, filename: &RemoteFilename) -> WaitKey {
  WaitKey::PeerFile {
    code: PeerCode::TransferRequest,
    username: username.to_owned(),
    filename: filename.clone(),
  }
}

/// The wait a transfer message that did not decode was for, read from the fields at its start,
/// and the message's name; `None` when not even those can be read or no download waits for it
fn unparseable_key(username: &str, frame: &[u8]) -> Option<(WaitKey, &'static str)> {
  let code = u32_at(frame, 4)?;
  let payload = frame.get(8..)?;
  match PeerCode::from_code(code)? {
    PeerCode::TransferRequest => {
      // only an uploader's offer is waited for
      let direction = u32_at(payload, 0)?;
      let filename = string_at(payload, 8)?;
      (direction == TransferDirection::Upload as u32)
        .then(|| (offer_key(username, &filename), "TransferRequest"))
    }
    PeerCode::TransferResponse => Some((
      WaitKey::PeerToken {
        code: PeerCode::TransferResponse,
        username: username.to_owned(),
        token: u32_at(payload, 0)?,
      },
      "TransferResponse",
    )),
    PeerCode::UploadDenied => Some((offer_key(username, &string_at(payload, 0)?), "UploadDenied")),
    PeerCode::UploadFailed => Some((offer_key(username, &string_at(payload, 0)?), "UploadFailed")),
    _ => None,
  }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
  let bytes = bytes.get(at..at + 4)?;
  Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn string_at(bytes: &[u8], at: usize) -> Option<RemoteFilename> {
  let len = u32_at(bytes, at)? as usize;
  let text = bytes.get(at + 4..(at + 4).checked_add(len)?)?;
  Some(RemoteFilename::from_bytes(text.to_vec()))
}

#[cfg(test)]
mod tests {
  use super::*;

  /// a frame of `code` whose payload is `payload`
  fn frame(code: PeerCode, payload: &[u8]) -> Vec<u8> {
    let mut frame = ((payload.len() + 4) as u32).to_le_bytes().to_vec();
    frame.extend(code.code().to_le_bytes());
    frame.extend(payload);
    frame
  }

  fn string(text: &str) -> Vec<u8> {
    let mut bytes = (text.len() as u32).to_le_bytes().to_vec();
    bytes.extend(text.as_bytes());
    bytes
  }

  #[test]
  fn an_unparseable_offer_names_the_downloads_wait() {
    // an upload offer whose size is cut short
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend(9u32.to_le_bytes());
    payload.extend(string("a.mp3"));
    payload.extend([1, 2, 3]);
    let (key, name) = unparseable_key("bob", &frame(PeerCode::TransferRequest, &payload)).unwrap();
    assert_eq!(key, offer_key("bob", &"a.mp3".into()));
    assert_eq!(name, "TransferRequest");
    // a download request is never waited for
    payload[0] = 0;
    assert!(unparseable_key("bob", &frame(PeerCode::TransferRequest, &payload)).is_none());
  }

  #[test]
  fn an_unparseable_answer_or_refusal_names_its_wait() {
    let (key, _) =
      unparseable_key("bob", &frame(PeerCode::TransferResponse, &[7, 0, 0, 0, 9])).unwrap();
    assert_eq!(
      key,
      WaitKey::PeerToken {
        code: PeerCode::TransferResponse,
        username: "bob".into(),
        token: 7,
      }
    );
    let denied = frame(PeerCode::UploadDenied, &string("a.mp3"));
    assert_eq!(
      unparseable_key("bob", &denied).unwrap().0,
      offer_key("bob", &"a.mp3".into())
    );
    // a filename cut short names nothing
    assert!(unparseable_key("bob", &frame(PeerCode::UploadFailed, &[9, 0, 0, 0, b'a'])).is_none());
  }
}
