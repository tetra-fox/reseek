#![no_main]

use api_types::live::LiveFrame;
use api_types::options::{FilePatch, FileReplace, FileValidate, OptionsPatch};
use api_types::transfers::TransferStates;
use libfuzzer_sys::fuzz_target;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A body that reads as `T` must write back to JSON that reads as the same value.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(data: &[u8]) {
  let Ok(value) = serde_json::from_slice::<T>(data) else {
    return;
  };
  let written = serde_json::to_vec(&value).expect("a read value writes");
  let again: T = serde_json::from_slice(&written).expect("a written value reads");
  assert_eq!(again, value);
}

// first byte: which request body or frame the rest is read as
fuzz_target!(|data: &[u8]| {
  let Some((&kind, rest)) = data.split_first() else {
    return;
  };
  match kind % 6 {
    0 => round_trip::<OptionsPatch>(rest),
    1 => round_trip::<FilePatch>(rest),
    2 => round_trip::<FileReplace>(rest),
    3 => round_trip::<FileValidate>(rest),
    4 => round_trip::<TransferStates>(rest),
    _ => round_trip::<LiveFrame>(rest),
  }
});
