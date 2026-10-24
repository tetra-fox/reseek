#![no_main]

use libfuzzer_sys::fuzz_target;
use soulseek_proto::Limits;
use soulseek_proto::frame::{self, FrameKind};

// first byte picks the frame kind, the rest is a stream of frames
fuzz_target!(|data: &[u8]| {
  let Some((&kind, mut buf)) = data.split_first() else {
    return;
  };
  let kind = match kind % 4 {
    0 => FrameKind::Server,
    1 => FrameKind::Peer,
    2 => FrameKind::Distributed,
    _ => FrameKind::Init,
  };
  // any result is fine, this only checks it does not panic
  let _ = frame::header(kind, buf, &Limits::DEFAULT);
  while let Ok(Some((frame, consumed))) = frame::split(kind, buf, &Limits::DEFAULT) {
    assert!(consumed <= buf.len());
    assert_eq!(frame.payload.len() + 4 + kind.code_width(), consumed);
    buf = &buf[consumed..];
  }
});
