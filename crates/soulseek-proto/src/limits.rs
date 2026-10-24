//! Hostile-input limits (spec 01 section 9). Each one is enforced where the decoder reads the field
//! it bounds; exceeding one is a [`DecodeError`](crate::DecodeError).

use crate::frame::FrameKind;

/// frame `len` on the server connection
pub const LIMIT_SERVER_MESSAGE_BYTES: u32 = 64 << 20;
/// frame `len` on peer connections; the default of [`Limits::peer_message_bytes`]
pub const LIMIT_PEER_MESSAGE_BYTES: u32 = 256 << 20;
/// frame `len` on distributed connections
pub const LIMIT_DISTRIBUTED_MESSAGE_BYTES: u32 = 64 << 10;
/// the first frame on an inbound socket
pub const LIMIT_INIT_MESSAGE_BYTES: u32 = 4 << 10;
/// decompressed size of a browse, search or folder response; the default of
/// [`Limits::inflated_bytes`]
pub const LIMIT_INFLATED_BYTES: usize = 1 << 30;
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

/// The limits the spec marks configurable. Everything else is a constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
  pub peer_message_bytes: u32,
  pub inflated_bytes: usize,
  pub files_per_response: usize,
}

impl Limits {
  pub const DEFAULT: Self = Self {
    peer_message_bytes: LIMIT_PEER_MESSAGE_BYTES,
    inflated_bytes: LIMIT_INFLATED_BYTES,
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
}

impl Default for Limits {
  fn default() -> Self {
    Self::DEFAULT
  }
}
