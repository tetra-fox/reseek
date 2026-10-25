#![no_main]

use libfuzzer_sys::fuzz_target;
use reseek_fuzz::{LIMITS, check_peer, compressed_frame};
use soulseek_proto::PeerMessage;

// first byte: bit 0 picks a body to compress (0) or a raw zlib stream (1), bit 1 picks a
// folder contents response (1) over a browse response (0)
fuzz_target!(|data: &[u8]| {
  let Some((&mode, rest)) = data.split_first() else {
    return;
  };
  let code = if mode & 2 == 0 { 5 } else { 37 };
  check_peer(PeerMessage::decode_with(
    &compressed_frame(code, mode, rest),
    &LIMITS,
  ));
});
