#![no_main]

use libfuzzer_sys::fuzz_target;
use reseek_fuzz::{LIMITS, check_peer, compressed_frame};
use soulseek_proto::{PeerMessage, peek_search_response};

// first byte: bit 0 picks a body to compress (0) or a raw zlib stream (1)
fuzz_target!(|data: &[u8]| {
  let Some((&mode, rest)) = data.split_first() else {
    return;
  };
  let frame = compressed_frame(9, mode, rest);
  // any result is fine, this only checks it does not panic
  let _ = peek_search_response(&frame, &LIMITS);
  check_peer(PeerMessage::decode_with(&frame, &LIMITS));
});
