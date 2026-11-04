//! Hostile-input limits. Each one is enforced where the decoder reads the field it bounds;
//! exceeding one is a [`DecodeError`](crate::DecodeError).

use crate::codes::PeerCode;
use crate::frame::FrameKind;

/// frame `len` on the server connection
pub const LIMIT_SERVER_MESSAGE_BYTES: u32 = 64 << 20;
/// frame `len` on peer connections; the default of [`Limits::peer_message_bytes`]
pub const LIMIT_PEER_MESSAGE_BYTES: u32 = 256 << 20;
/// frame `len` on distributed connections
pub const LIMIT_DISTRIBUTED_MESSAGE_BYTES: u32 = 64 << 10;
/// the first frame on an inbound socket
pub const LIMIT_INIT_MESSAGE_BYTES: u32 = 4 << 10;
/// decompressed size of a browse response; the default of [`Limits::inflated_browse_bytes`]
pub const LIMIT_INFLATED_BROWSE_BYTES: usize = 1 << 30;
/// decompressed size of a folder contents response; the default of
/// [`Limits::inflated_folder_bytes`]
pub const LIMIT_INFLATED_FOLDER_BYTES: usize = 128 << 20;
/// decompressed size of a search response, which any peer that saw the search can send; the
/// default of [`Limits::inflated_search_bytes`]
pub const LIMIT_INFLATED_SEARCH_BYTES: usize = 16 << 20;
/// any single string, chat messages included, except the two below
pub const LIMIT_STRING_BYTES: usize = 64 << 10;
/// the user-info description and the login greeting
pub const LIMIT_DESCRIPTION_BYTES: usize = 256 << 10;
/// `InfoResponse` picture length
pub const LIMIT_USER_INFO_PICTURE_BYTES: usize = 16 << 20;
/// total files in one browse, search or folder response; the default of
/// [`Limits::files_per_response`]
pub const LIMIT_FILES_PER_RESPONSE: usize = 10_000_000;
/// total directories in one browse or folder response
pub const LIMIT_DIRECTORIES_PER_RESPONSE: usize = 1_000_000;
/// `n_attributes` of one file
pub const LIMIT_ATTRIBUTES_PER_FILE: usize = 64;

/// The limits an operator can configure. Everything else is a constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
  /// frame `len` on peer connections
  pub peer_message_bytes: u32,
  /// decompressed size of a browse response
  pub inflated_browse_bytes: usize,
  /// decompressed size of a folder contents response
  pub inflated_folder_bytes: usize,
  /// decompressed size of a search response
  pub inflated_search_bytes: usize,
  /// total files in one browse, search or folder response
  pub files_per_response: usize,
}

impl Limits {
  /// the `LIMIT_*` constant of each field
  pub const DEFAULT: Self = Self {
    peer_message_bytes: LIMIT_PEER_MESSAGE_BYTES,
    inflated_browse_bytes: LIMIT_INFLATED_BROWSE_BYTES,
    inflated_folder_bytes: LIMIT_INFLATED_FOLDER_BYTES,
    inflated_search_bytes: LIMIT_INFLATED_SEARCH_BYTES,
    files_per_response: LIMIT_FILES_PER_RESPONSE,
  };

  /// largest frame `len` accepted on a connection of this kind
  pub fn frame_bytes(&self, kind: FrameKind) -> u32 {
    match kind {
      FrameKind::Server => LIMIT_SERVER_MESSAGE_BYTES,
      FrameKind::Peer => self.peer_message_bytes,
      FrameKind::Distributed => LIMIT_DISTRIBUTED_MESSAGE_BYTES,
      FrameKind::Init => LIMIT_INIT_MESSAGE_BYTES,
    }
  }

  /// the decompressed size a peer message of `code` may reach, `None` for one not compressed
  pub fn inflated_bytes(&self, code: PeerCode) -> Option<usize> {
    match code {
      PeerCode::BrowseResponse => Some(self.inflated_browse_bytes),
      PeerCode::FolderContentsResponse => Some(self.inflated_folder_bytes),
      PeerCode::SearchResponse => Some(self.inflated_search_bytes),
      _ => None,
    }
  }
}

impl Default for Limits {
  fn default() -> Self {
    Self::DEFAULT
  }
}
