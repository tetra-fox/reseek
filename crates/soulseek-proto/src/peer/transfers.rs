//! Transfer negotiation: requests, queueing, queue places and refusals (spec 01 section 6.7).

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::TransferDirection;
use crate::wire::{Reader, RemoteFilename, Writer};

/// Peer code 40, spec 01 section 6.7
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

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let direction = TransferDirection::read(r)?;
    let token = r.u32()?;
    let filename = r.filename()?;
    // a download request from an older client may carry a stray size of 0; it is ignored
    let file_size = match direction {
      TransferDirection::Upload if r.remaining() >= 8 => Some(r.u64()?),
      _ => None,
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

/// Peer code 41, spec 01 section 6.7
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum TransferResponse {
  Allowed {
    token: u32,
    /// present in the deprecated reply to a download request, absent in the reply to an upload
    /// announcement
    file_size: Option<u64>,
  },
  Denied {
    token: u32,
    /// `"Queued"` means accepted into the queue; anything else is display text
    reason: String,
  },
}

impl TransferResponse {
  pub fn token(&self) -> u32 {
    match self {
      Self::Allowed { token, .. } | Self::Denied { token, .. } => *token,
    }
  }
}

impl Wire for TransferResponse {
  const MIN_BYTES: usize = 5;

  /// `allowed` is true only for exactly 1, and a denial must carry a reason
  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let token = r.u32()?;
    Ok(if r.bool_exact()? {
      Self::Allowed {
        token,
        file_size: if r.remaining() >= 8 {
          Some(r.u64()?)
        } else {
          None
        },
      }
    } else {
      Self::Denied {
        token,
        reason: r.string()?,
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
      Self::Denied { token, reason } => {
        w.u32(*token);
        w.bool(false);
        w.string(reason);
      }
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.token = Some(self.token());
  }
}

wire_struct! {
  /// Peer code 42, spec 01 section 6.7, obsolete
  pub struct UploadPlacehold {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  /// Peer code 43, spec 01 section 6.7
  pub struct QueueDownload {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  /// Peer code 44, spec 01 section 6.7
  pub struct PlaceInQueueResponse {
    pub filename: RemoteFilename,
    /// 1-based
    pub place: u32,
  }
}

wire_struct! {
  /// Peer code 46, spec 01 section 6.7
  pub struct UploadFailed {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  /// Peer code 50, spec 01 section 6.7
  pub struct UploadDenied {
    pub filename: RemoteFilename,
    pub reason: String,
  }
}

wire_struct! {
  /// Peer code 51, spec 01 section 6.7
  pub struct PlaceInQueueRequest {
    pub filename: RemoteFilename,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::peer::PeerMessage;
  use crate::testing::{decodes, p, vectors};

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
      PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, reason: "Queued".into() })
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
      => Ok(PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, reason: "Cancelled".into() }));
    transfer_response_denied_needs_a_reason: PeerMessage, p().u32(5).u8(0).frame(41)
      => Err(DecodeError::Truncated { needed: 4, remaining: 0 });
  }

  #[test]
  fn queue_download_latin1_filename_is_echoed_with_its_bytes() {
    let bytes = p().bytes_string(b"caf\xe9.mp3").frame(43);
    let PeerMessage::QueueDownload(QueueDownload { filename }) =
      PeerMessage::decode(&bytes).unwrap()
    else {
      panic!("not a queue download");
    };
    assert_eq!(filename.as_str(), "caf\u{e9}.mp3");
    assert!(filename.is_lossy());
    let echo = PeerMessage::PlaceInQueueRequest(PlaceInQueueRequest { filename }).encode();
    assert_eq!(echo, p().bytes_string(b"caf\xe9.mp3").frame(51));
  }
}
