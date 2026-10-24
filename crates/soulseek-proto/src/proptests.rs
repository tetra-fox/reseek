//! Property tests for every decoder, and the strategies the `Arbitrary` derives refer to.
//! Generated messages stay inside what a decoder can produce, so `decode(encode(m)) == m` is
//! exact.

use proptest::arbitrary::{Arbitrary, any};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::strategy::BoxedStrategy;

use crate::codes::{DistributedCode, InitCode, PeerCode, ServerCode};
use crate::distributed::{DistributedMessage, Embedded};
use crate::error::FieldError;
use crate::frame::{self, FrameKind};
use crate::init::InitMessage;
use crate::limits::Limits;
use crate::model::{FileAttributeType, TransferDirection};
use crate::peer::{self, PeerMessage, TransferRequest, peek_search_response};
use crate::server::{ServerRequest, ServerResponse};
use crate::testing::zlib as compress;
use crate::wire::{RemoteFilename, Secret, Writer};

pub(crate) fn small_vec<T: Arbitrary>() -> impl Strategy<Value = Vec<T>> {
  vec(any::<T>(), 0..4)
}

/// sizes that the Soulseek NS fix-up would rewrite on read are left out
pub(crate) fn file_size() -> impl Strategy<Value = u64> {
  any::<u64>().prop_filter("sign-extended 32-bit size", |size| {
    size >> 32 != 0xffff_ffff
  })
}

pub(crate) fn peer_username() -> impl Strategy<Value = String> {
  "[a-zA-Z0-9_ .-]{1,30}".prop_filter("reserved name", |name| name != "server")
}

/// an unpadded embedded message with inner code 0 and two zero payload bytes would read back as
/// padded, so the inner code is never 0
pub(crate) fn distributed_embedded() -> impl Strategy<Value = Embedded> {
  (1..=u8::MAX, vec(any::<u8>(), 0..32)).prop_map(|(distributed_code, payload)| Embedded {
    distributed_code,
    payload,
  })
}

impl Arbitrary for RemoteFilename {
  type Parameters = ();
  type Strategy = BoxedStrategy<Self>;

  fn arbitrary_with((): ()) -> Self::Strategy {
    prop_oneof![
      any::<String>().prop_map(RemoteFilename::from),
      vec(any::<u8>(), 0..48).prop_map(RemoteFilename::from_bytes),
    ]
    .boxed()
  }
}

impl Arbitrary for Secret {
  type Parameters = ();
  type Strategy = BoxedStrategy<Self>;

  fn arbitrary_with((): ()) -> Self::Strategy {
    any::<String>().prop_map(Secret::new).boxed()
  }
}

impl Arbitrary for FileAttributeType {
  type Parameters = ();
  type Strategy = BoxedStrategy<Self>;

  fn arbitrary_with((): ()) -> Self::Strategy {
    prop_oneof![
      Just(Self::BitRate),
      Just(Self::Duration),
      Just(Self::VariableBitRate),
      Just(Self::SampleRate),
      Just(Self::BitDepth),
      Just(Self::Unknown(3)),
      (6..=u32::MAX).prop_map(Self::Unknown),
    ]
    .boxed()
  }
}

impl Arbitrary for TransferRequest {
  type Parameters = ();
  type Strategy = BoxedStrategy<Self>;

  /// a download request never carries a size
  fn arbitrary_with((): ()) -> Self::Strategy {
    (
      any::<TransferDirection>(),
      any::<u32>(),
      any::<RemoteFilename>(),
      any::<Option<u64>>(),
    )
      .prop_map(|(direction, token, filename, file_size)| Self {
        direction,
        token,
        filename,
        file_size: match direction {
          TransferDirection::Upload => file_size,
          TransferDirection::Download => None,
        },
      })
      .boxed()
  }
}

/// `[u32 len][code][payload]` for a known code of a connection kind
fn framed(
  codes: Vec<u32>,
  kind: FrameKind,
  payload: impl Strategy<Value = Vec<u8>>,
) -> impl Strategy<Value = Vec<u8>> {
  (proptest::sample::select(codes), payload).prop_map(move |(code, payload)| {
    let mut w = Writer::frame(kind, code);
    w.bytes(&payload);
    w.finish()
  })
}

fn codes<T: Copy>(all: &[T], code: fn(T) -> u32) -> Vec<u32> {
  all.iter().map(|&c| code(c)).collect()
}

fn server_frames() -> impl Strategy<Value = Vec<u8>> {
  framed(
    codes(ServerCode::ALL, ServerCode::code),
    FrameKind::Server,
    vec(any::<u8>(), 0..128),
  )
}

fn peer_frames() -> impl Strategy<Value = Vec<u8>> {
  framed(
    codes(PeerCode::ALL, PeerCode::code),
    FrameKind::Peer,
    vec(any::<u8>(), 0..128),
  )
}

/// browse, search and folder responses with a valid zlib stream around arbitrary bodies
fn compressed_peer_frames() -> impl Strategy<Value = Vec<u8>> {
  framed(
    vec![5, 9, 37],
    FrameKind::Peer,
    vec(any::<u8>(), 0..256).prop_map(|body| compress(&body)),
  )
}

fn distributed_frames() -> impl Strategy<Value = Vec<u8>> {
  framed(
    codes(DistributedCode::ALL, DistributedCode::code),
    FrameKind::Distributed,
    vec(any::<u8>(), 0..96),
  )
}

fn init_frames() -> impl Strategy<Value = Vec<u8>> {
  framed(
    codes(InitCode::ALL, InitCode::code),
    FrameKind::Init,
    vec(any::<u8>(), 0..64),
  )
}

/// any value a decoder returns must survive another encode and decode unchanged
macro_rules! assert_canonical {
  ($ty:ty, $bytes:expr) => {
    if let Ok(message) = <$ty>::decode($bytes) {
      prop_assert_eq!(<$ty>::decode(&message.encode()), Ok(message));
    }
  };
}

proptest! {
  #[test]
  fn server_request_round_trips(message in any::<ServerRequest>()) {
    prop_assert_eq!(ServerRequest::decode(&message.encode()), Ok(message));
  }

  #[test]
  fn server_response_round_trips(message in any::<ServerResponse>()) {
    prop_assert_eq!(ServerResponse::decode(&message.encode()), Ok(message));
  }

  #[test]
  fn peer_message_round_trips(message in any::<PeerMessage>()) {
    prop_assert_eq!(PeerMessage::decode(&message.encode()), Ok(message));
  }

  #[test]
  fn init_message_round_trips(message in any::<InitMessage>()) {
    prop_assert_eq!(InitMessage::decode(&message.encode()), Ok(message));
  }

  #[test]
  fn distributed_message_round_trips(message in any::<DistributedMessage>()) {
    prop_assert_eq!(DistributedMessage::decode(&message.encode()), Ok(message));
  }

  #[test]
  fn arbitrary_bytes_never_panic(bytes in vec(any::<u8>(), 0..512)) {
    assert_canonical!(ServerRequest, &bytes);
    assert_canonical!(ServerResponse, &bytes);
    assert_canonical!(PeerMessage, &bytes);
    assert_canonical!(InitMessage, &bytes);
    assert_canonical!(DistributedMessage, &bytes);
    // any result is fine, this only checks it does not panic
    let _ = peek_search_response(&bytes, &Limits::DEFAULT);
  }

  #[test]
  fn server_frames_decode_or_fail(bytes in server_frames()) {
    assert_canonical!(ServerRequest, &bytes);
    assert_canonical!(ServerResponse, &bytes);
  }

  #[test]
  fn peer_frames_decode_or_fail(bytes in prop_oneof![peer_frames(), compressed_peer_frames()]) {
    assert_canonical!(PeerMessage, &bytes);
    // any result is fine, this only checks it does not panic
    let _ = peek_search_response(&bytes, &Limits::DEFAULT);
  }

  #[test]
  fn distributed_frames_decode_or_fail(bytes in distributed_frames()) {
    assert_canonical!(DistributedMessage, &bytes);
  }

  #[test]
  fn init_frames_decode_or_fail(bytes in init_frames()) {
    assert_canonical!(InitMessage, &bytes);
  }

  #[test]
  fn split_never_claims_more_than_the_input(bytes in vec(any::<u8>(), 0..64), kind in prop_oneof![
    Just(FrameKind::Server), Just(FrameKind::Peer), Just(FrameKind::Distributed), Just(FrameKind::Init),
  ]) {
    if let Ok(Some((frame, consumed))) = frame::split(kind, &bytes, &Limits::DEFAULT) {
      prop_assert!(consumed <= bytes.len());
      let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
      prop_assert_eq!(consumed, 4 + len);
      prop_assert_eq!(frame.payload.len(), len - kind.code_width());
    }
  }

  #[test]
  fn inflate_never_exceeds_the_limit(body in vec(any::<u8>(), 0..2048), limit in 0usize..1024) {
    match peer::inflate(&compress(&body), limit) {
      Ok(inflated) => {
        prop_assert!(inflated.len() <= limit);
        prop_assert_eq!(inflated, body);
      }
      Err(error) => {
        prop_assert!(body.len() > limit);
        prop_assert_eq!(error, FieldError::InflatedTooLarge { limit });
      }
    }
  }

  #[test]
  fn hostile_compressed_counts_are_bounded(
    code in proptest::sample::select(vec![5u32, 9, 37]),
    counts in vec(any::<u32>(), 1..6),
    tail in vec(any::<u8>(), 0..64),
  ) {
    // a body of nothing but large counts must fail fast, never allocate per count
    let mut body = Writer::body();
    if code == 9 {
      body.string("peer");
    }
    for count in &counts {
      body.u32(*count);
    }
    body.bytes(&tail);
    let mut w = Writer::frame(FrameKind::Peer, code);
    w.bytes(&compress(&body.into_body()));
    let frame = w.finish();
    let limits = Limits { files_per_response: 4, inflated_bytes: 4096, ..Limits::DEFAULT };
    if let Ok(message) = PeerMessage::decode_with(&frame, &limits) {
      let files = match &message {
        PeerMessage::SearchResponse(r) => r.files.len() + r.locked_files.len(),
        PeerMessage::BrowseResponse(r) => r.directories.iter().chain(&r.locked_directories).map(|d| d.files.len()).sum(),
        PeerMessage::FolderContentsResponse(r) => r.directories.iter().map(|d| d.files.len()).sum(),
        _ => unreachable!("only compressed codes are generated"),
      };
      prop_assert!(files <= 4);
    }
  }
}
