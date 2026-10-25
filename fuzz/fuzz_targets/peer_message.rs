#![no_main]

use libfuzzer_sys::fuzz_target;
use reseek_fuzz::{LIMITS, check_peer};
use soulseek_proto::{PeerMessage, peek_search_response};

fuzz_target!(|data: &[u8]| {
  // any result is fine, this only checks it does not panic
  let _ = peek_search_response(data, &LIMITS);
  check_peer(PeerMessage::decode_with(data, &LIMITS));
});
