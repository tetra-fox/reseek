//! Messages on a `P` connection between two clients, the same layout in both directions. Each
//! message lives in the file of its topic; this file holds the catalog and the zlib handling that
//! browse, search and folder responses share, since each of them is one zlib stream after the code.

pub(crate) mod browse;
pub(crate) mod info;
pub(crate) mod search;
pub(crate) mod transfers;

pub use browse::*;
pub use info::*;
pub use search::*;
pub use transfers::*;

use std::io::Read;

use flate2::Compression;
use flate2::bufread::{ZlibDecoder, ZlibEncoder};

use crate::codec::family;
use crate::codes::PeerCode;
use crate::error::{DecodeError, FieldError};
use crate::limits::Limits;
use crate::wire::{Reader, Writer};

family! {
  /// Every message two clients exchange on a `P` connection
  pub enum PeerMessage {
    code: PeerCode,
    frame: Peer,
    label: "peer",
    quiet: [],
    messages: {
      /// asks for the peer's whole share listing
      BrowseRequest,
      BrowseResponse(BrowseResponse),
      /// obsolete, still answered
      SearchRequest(SearchRequest),
      SearchResponse(SearchResponse),
      /// asks for the peer's user info
      InfoRequest,
      InfoResponse(UserInfo),
      FolderContentsRequest(FolderContentsRequest),
      FolderContentsResponse(FolderContentsResponse),
      TransferRequest(TransferRequest),
      TransferResponse(TransferResponse),
      /// obsolete
      UploadPlacehold(UploadPlacehold),
      /// asks the peer to queue a file for us
      QueueDownload(QueueDownload),
      PlaceInQueueResponse(PlaceInQueueResponse),
      UploadFailed(UploadFailed),
      UploadDenied(UploadDenied),
      PlaceInQueueRequest(PlaceInQueueRequest),
      /// deprecated, empty; Soulseek NS sends it before uploading
      UploadQueueNotification,
    }
  }
}

impl PeerMessage {
  /// decodes one complete frame with configured limits
  pub fn decode_with(bytes: &[u8], limits: &Limits) -> Result<Self, DecodeError> {
    Self::decode_limited(bytes, limits)
  }
}

fn compress(body: &[u8]) -> Vec<u8> {
  let mut compressed = Vec::new();
  ZlibEncoder::new(body, Compression::default())
    .read_to_end(&mut compressed)
    .expect("compressing from a slice into a vec does not fail");
  compressed
}

/// Inflates the whole payload, failing once it grows past `limit` bytes or when the stream is
/// corrupt or ends before its end marker. Bytes after the end marker are ignored.
pub(crate) fn inflate(compressed: &[u8], limit: usize) -> Result<Vec<u8>, FieldError> {
  let mut inflated = Vec::new();
  ZlibDecoder::new(compressed)
    // one byte past the limit shows it was exceeded; with no effective limit the take saturates
    .take((limit as u64).saturating_add(1))
    .read_to_end(&mut inflated)
    .map_err(zlib_error)?;
  if inflated.len() > limit {
    return Err(FieldError::InflatedTooLarge { limit });
  }
  Ok(inflated)
}

fn zlib_error(error: std::io::Error) -> FieldError {
  // flate2 reports a stream that ends early as UnexpectedEof and anything else as InvalidInput
  match error.kind() {
    std::io::ErrorKind::UnexpectedEof => FieldError::TruncatedZlib,
    _ => FieldError::InvalidZlib,
  }
}

/// inflates the rest of the payload and reads the body from it, with a fresh file and directory
/// allowance
fn read_compressed<T>(
  r: &mut Reader,
  read: impl FnOnce(&mut Reader) -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
  let limits = r.limits();
  let body = inflate(r.rest(), limits.inflated_bytes).map_err(|error| r.fail("body", error))?;
  read(&mut Reader::new(&body, limits, r.message()))
}

fn write_compressed(w: &mut Writer, write: impl FnOnce(&mut Writer)) {
  let mut body = Writer::body();
  write(&mut body);
  w.bytes(&compress(&body.into_body()));
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::testing::p;

  #[test]
  fn codes_without_a_layout_decode_as_unknown() {
    for code in [1, 10, 14, 33, 34, 47, 48, 49] {
      assert_eq!(
        PeerMessage::decode(&p().u32(1).frame(code)),
        Err(DecodeError::UnknownCode { code })
      );
    }
  }

  #[test]
  fn inflate_rejects_truncated_and_corrupt_streams() {
    let compressed = compress(b"hello hello hello");
    assert_eq!(
      inflate(&compressed[..compressed.len() - 1], 1 << 20),
      Err(FieldError::TruncatedZlib)
    );
    assert_eq!(
      inflate(&compressed[..2], 1 << 20),
      Err(FieldError::TruncatedZlib)
    );
    assert_eq!(inflate(&[], 1 << 20), Err(FieldError::TruncatedZlib));
    assert_eq!(
      inflate(b"not zlib at all", 1 << 20),
      Err(FieldError::InvalidZlib)
    );
    let mut bad_checksum = compressed.clone();
    *bad_checksum.last_mut().unwrap() ^= 1;
    assert_eq!(
      inflate(&bad_checksum, 1 << 20),
      Err(FieldError::InvalidZlib)
    );
  }

  #[test]
  fn inflate_stops_a_zip_bomb_at_the_limit() {
    let compressed = compress(&vec![0; 64 << 20]);
    assert!(compressed.len() < 100 << 10);
    assert_eq!(
      inflate(&compressed, 1 << 20),
      Err(FieldError::InflatedTooLarge { limit: 1 << 20 })
    );
  }
}
