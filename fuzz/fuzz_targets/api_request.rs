#![no_main]

use api_types::analysis::SetAnalysisOverride;
use api_types::blocking::{Block, BlockRequest};
use api_types::chat::{MessageText, NotifySetting};
use api_types::files::FileTokenRequest;
use api_types::live::LiveFrame;
use api_types::options::{FilePatch, FileReplace, FileValidate, OptionsPatch};
use api_types::rooms::JoinRoom;
use api_types::search::SearchRequest;
use api_types::session::LoginRequest;
use api_types::tokens::{CreateToken, UpdateToken};
use api_types::transfers::{
  BulkRequest, EnqueueDownloadBatchRequest, QueueDownloadRequest, TransferStates,
};
use api_types::users::{GroupAssignment, UserTags};
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
  match kind % 22 {
    0 => round_trip::<OptionsPatch>(rest),
    1 => round_trip::<FilePatch>(rest),
    2 => round_trip::<FileReplace>(rest),
    3 => round_trip::<FileValidate>(rest),
    4 => round_trip::<TransferStates>(rest),
    5 => round_trip::<LoginRequest>(rest),
    6 => round_trip::<CreateToken>(rest),
    7 => round_trip::<UpdateToken>(rest),
    8 => round_trip::<SearchRequest>(rest),
    9 => round_trip::<Vec<Option<QueueDownloadRequest>>>(rest),
    10 => round_trip::<EnqueueDownloadBatchRequest>(rest),
    11 => round_trip::<BulkRequest>(rest),
    12 => round_trip::<BlockRequest>(rest),
    13 => round_trip::<Vec<Block>>(rest),
    14 => round_trip::<JoinRoom>(rest),
    15 => round_trip::<MessageText>(rest),
    16 => round_trip::<NotifySetting>(rest),
    17 => round_trip::<UserTags>(rest),
    18 => round_trip::<GroupAssignment>(rest),
    19 => round_trip::<FileTokenRequest>(rest),
    20 => round_trip::<SetAnalysisOverride>(rest),
    _ => round_trip::<LiveFrame>(rest),
  }
});
