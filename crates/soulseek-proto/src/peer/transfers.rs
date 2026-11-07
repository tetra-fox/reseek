//! Transfer negotiation: requests, queueing, queue places and refusals.

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::TransferDirection;
use crate::wire::{Reader, RemoteFilename, Writer};

/// Peer code 40: a request to download a file, or an uploader announcing it is about to send one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferRequest {
  pub direction: TransferDirection,
  /// chosen by the sender
  pub token: u32,
  pub filename: RemoteFilename,
  /// only carried by an upload announcement (`direction == Upload`), and missing from very old
  /// clients there; never written or read for a download request
  pub file_size: Option<u64>,
}

impl Wire for TransferRequest {
  const MIN_BYTES: usize = 12;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    let direction = TransferDirection::read(r, "direction")?;
    let token = r.u32("token")?;
    let filename = r.filename("filename")?;
    // a download request from an older client may carry a stray size of 0; it is ignored
    let file_size = match direction {
      TransferDirection::Upload => r.optional_fixed("file_size")?,
      TransferDirection::Download => None,
    };
    Ok(Self {
      direction,
      token,
      filename,
      file_size,
    })
  }

  fn write(&self, w: &mut Writer) {
    self.direction.write(w);
    w.u32(self.token);
    w.filename(&self.filename);
    if let (TransferDirection::Upload, Some(file_size)) = (self.direction, self.file_size) {
      w.u64(file_size);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.filename = Some(self.filename.as_str());
    fields.token = Some(self.token);
  }
}

/// Peer code 41: the answer to a `TransferRequest`, allowing the transfer or refusing it with a
/// message.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum TransferResponse {
  /// the transfer may start
  Allowed {
    token: u32,
    /// present in the deprecated reply to a download request, absent in the reply to an upload
    /// announcement
    file_size: Option<u64>,
  },
  /// the transfer does not start now
  Denied {
    token: u32,
    /// `"Queued"` means accepted into the queue; anything else is display text
    message: String,
  },
}

impl TransferResponse {
  /// the token of the `TransferRequest` this answers
  pub fn token(&self) -> u32 {
    match self {
      Self::Allowed { token, .. } | Self::Denied { token, .. } => *token,
    }
  }
}

impl Wire for TransferResponse {
  const MIN_BYTES: usize = 5;

  /// `allowed` is true only for exactly 1, and a denial must carry a message
  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    let token = r.u32("token")?;
    Ok(if r.bool_exact("allowed")? {
      Self::Allowed {
        token,
        file_size: r.optional_fixed("file_size")?,
      }
    } else {
      Self::Denied {
        token,
        message: r.string("message")?,
      }
    })
  }

  fn write(&self, w: &mut Writer) {
    match self {
      Self::Allowed { token, file_size } => {
        w.u32(*token);
        w.bool(true);
        if let Some(file_size) = file_size {
          w.u64(*file_size);
        }
      }
      Self::Denied { token, message } => {
        w.u32(*token);
        w.bool(false);
        w.string(message);
      }
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.token = Some(self.token());
  }
}

wire_struct! {
  /// Peer code 42: names a file. Obsolete: no client sends it.
  pub struct UploadPlacehold {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  /// Peer code 43: asks the peer to queue a file for us to download.
  pub struct QueueDownload {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  /// Peer code 44: our place in the peer's upload queue for a file, in reply to
  /// `PlaceInQueueRequest`.
  pub struct PlaceInQueueResponse {
    pub filename: RemoteFilename,
    /// 1-based
    pub place: u32,
  }
}

wire_struct! {
  /// Peer code 46: an upload of a file to us stopped before it completed.
  pub struct UploadFailed {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  /// Peer code 50: the peer refuses or cancels an upload of a file to us, with the reason.
  pub struct UploadDenied {
    pub filename: RemoteFilename,
    pub reason: String,
  }
}

wire_struct! {
  /// Peer code 51: asks for our place in the peer's upload queue for a file.
  pub struct PlaceInQueueRequest {
    pub filename: RemoteFilename,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::error::FieldError;
  use crate::peer::PeerMessage;
  use crate::testing::{decodes, field, p, vectors};

  vectors! {
    transfer_request_download:
      PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Download, token: 5, filename: "@@share\\a.mp3".into(), file_size: None,
      })
      => p().u32(0).u32(5).s("@@share\\a.mp3").frame(40);
    transfer_request_upload:
      PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Upload, token: 5, filename: "@@share\\a.mp3".into(), file_size: Some(5_242_880),
      })
      => p().u32(1).u32(5).s("@@share\\a.mp3").u64(5_242_880).frame(40);
    transfer_response_allowed_with_size:
      PeerMessage::TransferResponse(TransferResponse::Allowed { token: 5, file_size: Some(5_242_880) })
      => p().u32(5).u8(1).u64(5_242_880).frame(41);
    transfer_response_allowed:
      PeerMessage::TransferResponse(TransferResponse::Allowed { token: 5, file_size: None }) => p().u32(5).u8(1).frame(41);
    transfer_response_denied:
      PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, message: "Queued".into() })
      => p().u32(5).u8(0).s("Queued").frame(41);
    upload_placehold: PeerMessage::UploadPlacehold(UploadPlacehold { filename: "a.mp3".into() }) => p().s("a.mp3").frame(42);
    queue_download:
      PeerMessage::QueueDownload(QueueDownload { filename: "@@share\\a.mp3".into() }) => p().s("@@share\\a.mp3").frame(43);
    queue_download_latin1:
      PeerMessage::QueueDownload(QueueDownload { filename: RemoteFilename::from_bytes(b"caf\xe9\\\xe0.mp3".to_vec()) })
      => p().bytes_string(b"caf\xe9\\\xe0.mp3").frame(43);
    place_in_queue_response:
      PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse { filename: "a.mp3".into(), place: 3 })
      => p().s("a.mp3").u32(3).frame(44);
    upload_failed: PeerMessage::UploadFailed(UploadFailed { filename: "a.mp3".into() }) => p().s("a.mp3").frame(46);
    upload_denied:
      PeerMessage::UploadDenied(UploadDenied { filename: "a.mp3".into(), reason: "File not shared.".into() })
      => p().s("a.mp3").s("File not shared.").frame(50);
    place_in_queue_request:
      PeerMessage::PlaceInQueueRequest(PlaceInQueueRequest { filename: "a.mp3".into() }) => p().s("a.mp3").frame(51);
    upload_queue_notification: PeerMessage::UploadQueueNotification => p().frame(52);
  }

  decodes! {
    transfer_request_download_ignores_a_stray_size: PeerMessage, p().u32(0).u32(5).s("a.mp3").u64(0).frame(40)
      => Ok(PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Download, token: 5, filename: "a.mp3".into(), file_size: None,
      }));
    transfer_request_upload_without_size: PeerMessage, p().u32(1).u32(5).s("a.mp3").frame(40)
      => Ok(PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Upload, token: 5, filename: "a.mp3".into(), file_size: None,
      }));
    transfer_response_allowed_must_be_exactly_one: PeerMessage, p().u32(5).u8(2).s("Cancelled").frame(41)
      => Ok(PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, message: "Cancelled".into() }));
    transfer_response_denied_needs_a_message: PeerMessage, p().u32(5).u8(0).frame(41)
      => Err(field("TransferResponse", "message", FieldError::Truncated { needed: 4, remaining: 0 }));
  }

  #[test]
  fn queue_download_latin1_filename_reads_as_its_text_and_is_lossy() {
    let bytes = p().bytes_string(b"caf\xe9.mp3").frame(43);
    let PeerMessage::QueueDownload(QueueDownload { filename }) =
      PeerMessage::decode(&bytes).unwrap()
    else {
      panic!("not a queue download");
    };
    assert_eq!(filename.as_str(), "caf\u{e9}.mp3");
    assert!(filename.is_lossy());
  }
}
