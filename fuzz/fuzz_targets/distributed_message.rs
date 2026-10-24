#![no_main]

use libfuzzer_sys::fuzz_target;
use soulseek_proto::DistributedMessage;

fuzz_target!(|data: &[u8]| {
  if let Ok(message) = DistributedMessage::decode(data) {
    if let DistributedMessage::EmbeddedMessage { embedded, .. } = &message {
      // any result is fine, this only checks it does not panic
      let _ = DistributedMessage::decode(&embedded.inner_frame());
    }
    assert_eq!(DistributedMessage::decode(&message.encode()), Ok(message));
  }
});
