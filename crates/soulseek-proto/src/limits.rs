//! Hostile-input limits. Each one is enforced where the decoder reads the field it bounds;
//! exceeding one is a [`DecodeError`](crate::DecodeError). The bound on a peer message's frame
//! length by its code, [`Limits::peer_message_bytes`], is for a reader to skip a frame from its
//! header without reading it in.

use crate::codes::PeerCode;
use crate::frame::FrameKind;

/// frame `len` on the server connection
pub const LIMIT_SERVER_MESSAGE_BYTES: u32 = 64 << 20;
/// frame `len` of a browse response, the one peer message a large share makes large; the
/// default of [`Limits::browse_message_bytes`]
pub const LIMIT_BROWSE_MESSAGE_BYTES: u32 = 256 << 20;
/// frame `len` of a peer message made of a few integers and strings, which is every one but the
/// compressed responses and the user info
pub const LIMIT_PEER_SMALL_MESSAGE_BYTES: u32 = 256 << 10;
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
  /// frame `len` of a browse response
  pub browse_message_bytes: u32,
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
    browse_message_bytes: LIMIT_BROWSE_MESSAGE_BYTES,
    inflated_browse_bytes: LIMIT_INFLATED_BROWSE_BYTES,
    inflated_folder_bytes: LIMIT_INFLATED_FOLDER_BYTES,
    inflated_search_bytes: LIMIT_INFLATED_SEARCH_BYTES,
    files_per_response: LIMIT_FILES_PER_RESPONSE,
  };

  /// largest frame `len` accepted on a connection of this kind
  pub fn frame_bytes(&self, kind: FrameKind) -> u32 {
    match kind {
      FrameKind::Server => LIMIT_SERVER_MESSAGE_BYTES,
      // the longest any peer message may be; one longer than its own code allows is skipped
      FrameKind::Peer => [
        PeerCode::BrowseResponse,
        PeerCode::FolderContentsResponse,
        PeerCode::SearchResponse,
        PeerCode::InfoResponse,
      ]
      .into_iter()
      .map(|code| self.peer_message_bytes(code))
      .fold(LIMIT_PEER_SMALL_MESSAGE_BYTES, u32::max),
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

  /// The longest frame `len` a peer message of `code` may have: the browse limit for a browse
  /// response, room for the inflate limit of the other compressed ones, room for the longest
  /// description and picture in user info, and the small-message limit for the rest
  pub fn peer_message_bytes(&self, code: PeerCode) -> u32 {
    let payload = match code {
      PeerCode::BrowseResponse => return self.browse_message_bytes,
      PeerCode::FolderContentsResponse => zlib_bound(self.inflated_folder_bytes),
      PeerCode::SearchResponse => zlib_bound(self.inflated_search_bytes),
      // the description and picture lengths, the picture flag, the slots and queue, the free
      // slot flag and the upload permission
      PeerCode::InfoResponse => {
        (4 + 4 + 1 + 4 + 4 + 1 + 4)
          + LIMIT_DESCRIPTION_BYTES as u64
          + LIMIT_USER_INFO_PICTURE_BYTES as u64
      }
      _ => return LIMIT_PEER_SMALL_MESSAGE_BYTES,
    };
    // a frame length has 32 bits, so a bound past them lets every length through
    u32::try_from(FrameKind::Peer.code_width() as u64 + payload).unwrap_or(u32::MAX)
  }
}

/// the most a zlib stream of `inflated` bytes takes, by zlib's `compressBound`
fn zlib_bound(inflated: usize) -> u64 {
  let n = inflated as u64;
  n + (n >> 12) + (n >> 14) + (n >> 25) + 13
}

impl Default for Limits {
  fn default() -> Self {
    Self::DEFAULT
  }
}

#[cfg(test)]
mod tests {
  use std::io::Read;

  use flate2::Compression;
  use flate2::bufread::ZlibEncoder;

  use super::*;

  #[test]
  fn an_incompressible_response_at_its_inflate_limit_fits_its_frame_bound() {
    let limits = Limits {
      inflated_search_bytes: 200_000,
      ..Limits::DEFAULT
    };
    // xorshift output, which deflate cannot shrink
    let mut state = 0x2545_f491_u32;
    let body: Vec<u8> = (0..limits.inflated_search_bytes)
      .map(|_| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as u8
      })
      .collect();
    let mut compressed = Vec::new();
    ZlibEncoder::new(body.as_slice(), Compression::default())
      .read_to_end(&mut compressed)
      .unwrap();
    assert!(compressed.len() > body.len());
    let len = 4 + compressed.len() as u32;
    assert!(len <= limits.peer_message_bytes(PeerCode::SearchResponse));
  }

  #[test]
  fn the_peer_frame_bound_is_the_largest_message_bound() {
    let limits = Limits {
      browse_message_bytes: 1024,
      ..Limits::DEFAULT
    };
    assert_eq!(limits.peer_message_bytes(PeerCode::BrowseResponse), 1024);
    assert_eq!(
      limits.frame_bytes(FrameKind::Peer),
      limits.peer_message_bytes(PeerCode::FolderContentsResponse)
    );
    assert_eq!(
      limits.peer_message_bytes(PeerCode::TransferRequest),
      LIMIT_PEER_SMALL_MESSAGE_BYTES
    );
    assert_eq!(
      Limits::DEFAULT.frame_bytes(FrameKind::Peer),
      LIMIT_BROWSE_MESSAGE_BYTES
    );
  }
}
