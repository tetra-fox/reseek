//! Helpers shared by the fuzz targets

use std::io::Read;

use flate2::Compression;
use flate2::bufread::ZlibEncoder;
use soulseek_proto::{DecodeError, Limits, PeerMessage};

/// keeps a fuzz run's memory bounded while still exercising the inflate limit
pub const LIMITS: Limits = Limits {
  inflated_bytes: 16 << 20,
  ..Limits::DEFAULT
};

/// Builds a compressed peer frame from fuzz input: the first byte's low bit picks whether the
/// rest is a body to compress (0) or a raw zlib stream (1)
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
