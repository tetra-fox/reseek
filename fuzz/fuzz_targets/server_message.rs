#![no_main]

use libfuzzer_sys::fuzz_target;
use soulseek_proto::{DistributedMessage, ServerRequest, ServerResponse};

fuzz_target!(|data: &[u8]| {
  if let Ok(message) = ServerRequest::decode(data) {
    assert_eq!(ServerRequest::decode(&message.encode()), Ok(message));
  }
  if let Ok(message) = ServerResponse::decode(data) {
    if let ServerResponse::EmbeddedMessage(embedded) = &message {
      // any result is fine, this only checks it does not panic
      let _ = DistributedMessage::decode(&embedded.inner_frame());
    }
    assert_eq!(ServerResponse::decode(&message.encode()), Ok(message));
  }
});
