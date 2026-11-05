//! Helpers shared by the fuzz targets

pub mod config;

use std::io::Read;

use flate2::Compression;
use flate2::bufread::ZlibEncoder;
use soulseek_proto::{DecodeError, Limits, PeerMessage};

/// inflate caps a fuzz input can reach: libFuzzer's inputs are at most 4 KiB by default, and a
/// raw zlib stream of 4 KiB inflates to about 4 MiB (deflate tops out near 1032:1), so 1 MiB is
/// within reach of the raw-stream mode
pub const LIMITS: Limits = Limits {
  inflated_browse_bytes: 1 << 20,
  inflated_folder_bytes: 1 << 20,
  inflated_search_bytes: 1 << 20,
  ..Limits::DEFAULT
};

/// Builds a compressed peer frame from fuzz input: the low bit of `mode` picks whether `rest` is
/// a body to compress (0) or a raw zlib stream (1)
pub fn compressed_frame(code: u32, mode: u8, rest: &[u8]) -> Vec<u8> {
  let payload = if mode & 1 == 0 {
    let mut compressed = Vec::new();
    ZlibEncoder::new(rest, Compression::default())
      .read_to_end(&mut compressed)
      .unwrap();
    compressed
  } else {
    rest.to_vec()
  };
  let mut frame = (payload.len() as u32 + 4).to_le_bytes().to_vec();
  frame.extend(code.to_le_bytes());
  frame.extend(payload);
  frame
}

/// a decoded message must encode to something that decodes back to it
pub fn check_peer(decoded: Result<PeerMessage, DecodeError>) {
  if let Ok(message) = decoded {
    assert_eq!(
      PeerMessage::decode_with(&message.encode(), &LIMITS),
      Ok(message)
    );
  }
}
