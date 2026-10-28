#![no_main]

use api_types::relay::RelayEnvelope;
use libfuzzer_sys::fuzz_target;

// a relay socket frame that reads as a message writes back to JSON that reads as the same one
fuzz_target!(|data: &[u8]| {
  let Ok(envelope) = serde_json::from_slice::<RelayEnvelope>(data) else {
    return;
  };
  let written = serde_json::to_vec(&envelope).expect("a read message writes");
  let again: RelayEnvelope = serde_json::from_slice(&written).expect("a written message reads");
  assert_eq!(again, envelope);
});
