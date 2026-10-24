//! Messages on a `P` connection between two clients (spec 01 section 6). The layouts are the same
//! in both directions.

use crate::codes::PeerCode;
use crate::compressed::{self, FolderContentsResponse};
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::{LIMIT_USER_INFO_PICTURE_BYTES, Limits};
use crate::log::{self, KeyFields};
use crate::model::{BrowseResponse, SearchResponse, TransferDirection, UserInfo};
use crate::wire::{Reader, RemoteFilename, Writer};

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

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum PeerMessage {
  BrowseRequest,
  BrowseResponse(BrowseResponse),
  /// obsolete, still answered
  SearchRequest {
    token: u32,
    query: String,
  },
  SearchResponse(SearchResponse),
  InfoRequest,
  InfoResponse(UserInfo),
  FolderContentsRequest {
    token: u32,
    /// the remote directory path exactly as it appeared in a browse or search
    directory_name: RemoteFilename,
  },
  FolderContentsResponse(FolderContentsResponse),
  TransferRequest(TransferRequest),
  TransferResponse(TransferResponse),
  /// obsolete
  UploadPlacehold {
    filename: RemoteFilename,
  },
  /// ask the peer to queue a file for us
  QueueDownload {
    filename: RemoteFilename,
  },
  PlaceInQueueResponse {
    filename: RemoteFilename,
    /// 1-based
    place: u32,
  },
  UploadFailed {
    filename: RemoteFilename,
  },
  UploadDenied {
    filename: RemoteFilename,
    reason: String,
  },
  PlaceInQueueRequest {
    filename: RemoteFilename,
  },
  /// deprecated, empty; Soulseek NS sends it before uploading
  UploadQueueNotification,
}

impl PeerMessage {
  pub fn code(&self) -> PeerCode {
    match self {
      Self::BrowseRequest => PeerCode::BrowseRequest,
      Self::BrowseResponse(_) => PeerCode::BrowseResponse,
      Self::SearchRequest { .. } => PeerCode::SearchRequest,
      Self::SearchResponse(_) => PeerCode::SearchResponse,
      Self::InfoRequest => PeerCode::InfoRequest,
      Self::InfoResponse(_) => PeerCode::InfoResponse,
      Self::FolderContentsRequest { .. } => PeerCode::FolderContentsRequest,
      Self::FolderContentsResponse(_) => PeerCode::FolderContentsResponse,
      Self::TransferRequest(_) => PeerCode::TransferRequest,
      Self::TransferResponse(_) => PeerCode::TransferResponse,
      Self::UploadPlacehold { .. } => PeerCode::UploadPlacehold,
      Self::QueueDownload { .. } => PeerCode::QueueDownload,
      Self::PlaceInQueueResponse { .. } => PeerCode::PlaceInQueueResponse,
      Self::UploadFailed { .. } => PeerCode::UploadFailed,
      Self::UploadDenied { .. } => PeerCode::UploadDenied,
      Self::PlaceInQueueRequest { .. } => PeerCode::PlaceInQueueRequest,
      Self::UploadQueueNotification => PeerCode::UploadQueueNotification,
    }
  }

  /// the complete frame, length prefix included; browse, search and folder responses are
  /// compressed
  pub fn encode(&self) -> Vec<u8> {
    let mut w = Writer::frame(self.code().code());
    match self {
      Self::BrowseRequest | Self::InfoRequest | Self::UploadQueueNotification => {}
      Self::BrowseResponse(response) => compressed::write_browse_response(&mut w, response),
      Self::SearchRequest { token, query } => {
        w.u32(*token);
        w.string(query);
      }
      Self::SearchResponse(response) => compressed::write_search_response(&mut w, response),
      Self::InfoResponse(info) => {
        w.string(&info.description);
        w.bool(info.picture.is_some());
        if let Some(picture) = &info.picture {
          w.count(picture.len());
          w.bytes(picture);
        }
        w.u32(info.upload_slots);
        w.u32(info.queue_length);
        w.bool(info.has_free_upload_slot);
        if let Some(upload_permitted) = info.upload_permitted {
          w.u32(upload_permitted);
        }
      }
      Self::FolderContentsRequest {
        token,
        directory_name,
      } => {
        w.u32(*token);
        w.filename(directory_name);
      }
      Self::FolderContentsResponse(response) => {
        compressed::write_folder_contents_response(&mut w, response)
      }
      Self::TransferRequest(request) => {
        w.u32(request.direction as u32);
        w.u32(request.token);
        w.filename(&request.filename);
        if let (TransferDirection::Upload, Some(file_size)) = (request.direction, request.file_size)
        {
          w.u64(file_size);
        }
      }
      Self::TransferResponse(TransferResponse::Allowed { token, file_size }) => {
        w.u32(*token);
        w.bool(true);
        if let Some(file_size) = file_size {
          w.u64(*file_size);
        }
      }
      Self::TransferResponse(TransferResponse::Denied { token, reason }) => {
        w.u32(*token);
        w.bool(false);
        w.string(reason);
      }
      Self::UploadPlacehold { filename }
      | Self::QueueDownload { filename }
      | Self::UploadFailed { filename }
      | Self::PlaceInQueueRequest { filename } => w.filename(filename),
      Self::PlaceInQueueResponse { filename, place } => {
        w.filename(filename);
        w.u32(*place);
      }
      Self::UploadDenied { filename, reason } => {
        w.filename(filename);
        w.string(reason);
      }
    }
    w.finish()
  }

  /// decodes one complete frame with the default limits
  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    Self::decode_with(bytes, &Limits::DEFAULT)
  }

  /// decodes one complete frame with configured limits
  pub fn decode_with(bytes: &[u8], limits: &Limits) -> Result<Self, DecodeError> {
    let frame = frame::exact(FrameKind::Peer, bytes, limits)?;
    let code = PeerCode::from_code(frame.code);
    let message = match code {
      Some(code) => Self::read(code, frame.payload, limits),
      None => Err(DecodeError::UnknownCode { code: frame.code }),
    }
    .inspect_err(|error| log::rejected("peer", frame.code, code.map(PeerCode::name), error))?;
    log::decoded(
      "peer",
      frame.code,
      message.code().name(),
      message.key_fields(),
    );
    Ok(message)
  }

  fn read(code: PeerCode, payload: &[u8], limits: &Limits) -> Result<Self, DecodeError> {
    let r = &mut Reader::new(payload);
    Ok(match code {
      PeerCode::BrowseRequest => Self::BrowseRequest,
      PeerCode::BrowseResponse => {
        Self::BrowseResponse(compressed::read_browse_response(payload, limits)?)
      }
      PeerCode::SearchRequest => Self::SearchRequest {
        token: r.u32()?,
        query: r.string()?,
      },
      PeerCode::SearchResponse => {
        Self::SearchResponse(compressed::read_search_response(payload, limits)?)
      }
      PeerCode::InfoRequest => Self::InfoRequest,
      PeerCode::InfoResponse => Self::InfoResponse(read_user_info(r)?),
      PeerCode::FolderContentsRequest => Self::FolderContentsRequest {
        token: r.u32()?,
        directory_name: r.filename()?,
      },
      PeerCode::FolderContentsResponse => {
        Self::FolderContentsResponse(compressed::read_folder_contents_response(payload, limits)?)
      }
      PeerCode::TransferRequest => {
        let direction = TransferDirection::from_u32(r.u32()?)?;
        let token = r.u32()?;
        let filename = r.filename()?;
        // a download request from an older client may carry a stray size of 0; it is ignored
        let file_size = match direction {
          TransferDirection::Upload if r.remaining() >= 8 => Some(r.u64()?),
          _ => None,
        };
        Self::TransferRequest(TransferRequest {
          direction,
          token,
          filename,
          file_size,
        })
      }
      PeerCode::TransferResponse => {
        let token = r.u32()?;
        Self::TransferResponse(if r.bool_exact()? {
          TransferResponse::Allowed {
            token,
            file_size: if r.remaining() >= 8 {
              Some(r.u64()?)
            } else {
              None
            },
          }
        } else {
          TransferResponse::Denied {
            token,
            reason: r.string()?,
          }
        })
      }
      PeerCode::UploadPlacehold => Self::UploadPlacehold {
        filename: r.filename()?,
      },
      PeerCode::QueueDownload => Self::QueueDownload {
        filename: r.filename()?,
      },
      PeerCode::PlaceInQueueResponse => Self::PlaceInQueueResponse {
        filename: r.filename()?,
        place: r.u32()?,
      },
      PeerCode::UploadFailed => Self::UploadFailed {
        filename: r.filename()?,
      },
      PeerCode::UploadDenied => Self::UploadDenied {
        filename: r.filename()?,
        reason: r.string()?,
      },
      PeerCode::PlaceInQueueRequest => Self::PlaceInQueueRequest {
        filename: r.filename()?,
      },
      PeerCode::UploadQueueNotification => Self::UploadQueueNotification,
      // named by the protocol reference without a layout
      PeerCode::PrivateMessage
      | PeerCode::PrivateRoomInvitation
      | PeerCode::CancelledQueuedTransfer
      | PeerCode::SendConnectToken
      | PeerCode::MoveDownloadToTop
      | PeerCode::ExactFileSearchRequest
      | PeerCode::QueuedDownloads
      | PeerCode::IndirectFileSearchRequest => {
        return Err(DecodeError::UnknownCode { code: code.code() });
      }
    })
  }

  fn key_fields(&self) -> KeyFields<'_> {
    match self {
      Self::BrowseResponse(response) => {
        KeyFields::count(response.directories.len() + response.locked_directories.len())
      }
      Self::SearchRequest { token, .. } => KeyFields::token(*token),
      Self::SearchResponse(response) => KeyFields::username(&response.username)
        .with_token(response.token)
        .with_count(response.files.len() + response.locked_files.len()),
      Self::FolderContentsRequest {
        token,
        directory_name,
      } => KeyFields::filename(directory_name.as_str()).with_token(*token),
      Self::FolderContentsResponse(response) => {
        KeyFields::filename(response.directory_name.as_str())
          .with_token(response.token)
          .with_count(response.directories.len())
      }
      Self::TransferRequest(request) => {
        KeyFields::filename(request.filename.as_str()).with_token(request.token)
      }
      Self::TransferResponse(response) => KeyFields::token(response.token()),
      Self::UploadPlacehold { filename }
      | Self::QueueDownload { filename }
      | Self::PlaceInQueueResponse { filename, .. }
      | Self::UploadFailed { filename }
      | Self::UploadDenied { filename, .. }
      | Self::PlaceInQueueRequest { filename } => KeyFields::filename(filename.as_str()),
      Self::BrowseRequest
      | Self::InfoRequest
      | Self::InfoResponse(_)
      | Self::UploadQueueNotification => KeyFields::default(),
    }
  }
}

fn read_user_info(r: &mut Reader) -> Result<UserInfo, DecodeError> {
  let description = r.description()?;
  let picture = if r.bool()? {
    let len = r.u32()? as usize;
    if len > LIMIT_USER_INFO_PICTURE_BYTES {
      return Err(DecodeError::LimitExceeded {
        what: "user info picture bytes",
        value: len as u64,
        limit: LIMIT_USER_INFO_PICTURE_BYTES as u64,
      });
    }
    Some(r.bytes(len)?.to_vec())
  } else {
    None
  };
  Ok(UserInfo {
    description,
    picture,
    upload_slots: r.u32()?,
    queue_length: r.u32()?,
    has_free_upload_slot: r.bool()?,
    // Museek+ writes the slot flag as a u32, leaving 3 stray bytes that must not be read as this
    upload_permitted: if r.remaining() >= 4 {
      Some(r.u32()?)
    } else {
      None
    },
  })
}

/// Inflates only the `username` and `token` that start a `SearchResponse` frame, so a response
/// whose token is not a live search can be dropped before the rest is inflated
pub fn peek_search_response(bytes: &[u8], limits: &Limits) -> Result<(String, u32), DecodeError> {
  let frame = frame::exact(FrameKind::Peer, bytes, limits)?;
  if frame.code != PeerCode::SearchResponse.code() {
    return Err(DecodeError::InvalidValue {
      what: "search response code",
      value: frame.code,
    });
  }
  compressed::peek_search_response(frame.payload).inspect_err(|error| {
    log::rejected(
      "peer",
      frame.code,
      Some(PeerCode::SearchResponse.name()),
      error,
    )
  })
}
