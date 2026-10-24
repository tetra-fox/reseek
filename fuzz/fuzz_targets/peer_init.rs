#![no_main]

use libfuzzer_sys::fuzz_target;
use soulseek_proto::InitMessage;
use soulseek_proto::init::{FileOffset, FileTransferInit};

fuzz_target!(|data: &[u8]| {
  if let Ok(message) = InitMessage::decode(data) {
    assert_eq!(InitMessage::decode(&message.encode()), Ok(message));
  }
  // any result is fine, this only checks it does not panic
  let _ = FileTransferInit::decode(data);
  let _ = FileOffset::decode(data);
});
