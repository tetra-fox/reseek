//! Shared helpers for the byte-exact tests in each family module, and the checks that run over
//! all of their vectors. A vector is built from wire primitives with [`P`], not with the crate's
//! own writer, and checked both ways: `encode` must produce exactly those bytes and `decode`
//! must return the message.

use std::fmt::Debug;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::Path;

use flate2::Compression;
use flate2::bufread::{ZlibDecoder, ZlibEncoder};

use crate::codes::{DistributedCode, InitCode, PeerCode, ServerCode};
use crate::error::DecodeError;
use crate::{DistributedMessage, InitMessage, PeerMessage, ServerRequest, ServerResponse};

/// the address the server messages with an `ip4` field carry in their vectors
pub(crate) const IP: [u8; 4] = [192, 168, 1, 20];

pub(crate) fn ip() -> Ipv4Addr {
  Ipv4Addr::from(IP)
}

/// payload builder over wire primitives
#[derive(Clone, Default)]
pub(crate) struct P(pub(crate) Vec<u8>);

pub(crate) fn p() -> P {
  P::default()
}

impl P {
  pub(crate) fn u8(mut self, value: u8) -> Self {
    self.0.push(value);
    self
  }

  pub(crate) fn u16(mut self, value: u16) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  pub(crate) fn u32(mut self, value: u32) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  pub(crate) fn i32(mut self, value: i32) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  pub(crate) fn u64(mut self, value: u64) -> Self {
    self.0.extend(value.to_le_bytes());
    self
  }

  pub(crate) fn raw(mut self, bytes: &[u8]) -> Self {
    self.0.extend(bytes);
    self
  }

  pub(crate) fn bytes_string(self, bytes: &[u8]) -> Self {
    self.u32(bytes.len() as u32).raw(bytes)
  }

  pub(crate) fn s(self, text: &str) -> Self {
    self.bytes_string(text.as_bytes())
  }

  /// the wire holds the octets in reverse order
  pub(crate) fn ip(self, [a, b, c, d]: [u8; 4]) -> Self {
    self.raw(&[d, c, b, a])
  }

  /// a frame with a 4-byte code
  pub(crate) fn frame(self, code: u32) -> Vec<u8> {
    let mut frame = (self.0.len() as u32 + 4).to_le_bytes().to_vec();
    frame.extend(code.to_le_bytes());
    frame.extend(self.0);
    frame
  }

  /// a frame with a 1-byte code
  pub(crate) fn frame_u8(self, code: u8) -> Vec<u8> {
    let mut frame = (self.0.len() as u32 + 1).to_le_bytes().to_vec();
    frame.push(code);
    frame.extend(self.0);
    frame
  }

  /// a peer frame whose payload is this body, zlib-compressed
  pub(crate) fn zframe(self, code: u32) -> Vec<u8> {
    p().raw(&zlib(&self.0)).frame(code)
  }
}

pub(crate) fn zlib(body: &[u8]) -> Vec<u8> {
  let mut out = Vec::new();
  ZlibEncoder::new(body, Compression::default())
    .read_to_end(&mut out)
    .unwrap();
  out
}

/// `None` for the rejection vectors whose payload is not a valid stream
fn inflate(compressed: &[u8]) -> Option<Vec<u8>> {
  let mut out = Vec::new();
  ZlibDecoder::new(compressed).read_to_end(&mut out).ok()?;
  Some(out)
}

/// the first `n` bytes of a frame, for truncation tests
pub(crate) fn first(mut bytes: Vec<u8>, n: usize) -> Vec<u8> {
  bytes.truncate(n);
  bytes
}

/// What the vector macros need from a family enum
pub(crate) trait Family: Clone + Debug + PartialEq + Sized {
  const FUZZ_TARGET: &'static str;
  /// the selector byte of the `frame` fuzz target
  const FRAME_KIND: u8;

  fn decode(bytes: &[u8]) -> Result<Self, DecodeError>;
  fn encode(&self) -> Vec<u8>;
}

macro_rules! impl_family {
  ($($ty:ty => $target:literal, $kind:literal;)*) => {
    $(
      impl Family for $ty {
        const FUZZ_TARGET: &'static str = $target;
        const FRAME_KIND: u8 = $kind;

        fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
          <$ty>::decode(bytes)
        }

        fn encode(&self) -> Vec<u8> {
          <$ty>::encode(self)
        }
      }
    )*
  };
}

impl_family! {
  ServerRequest => "server_message", 0;
  ServerResponse => "server_message", 0;
  PeerMessage => "peer_message", 1;
  DistributedMessage => "distributed_message", 2;
  InitMessage => "peer_init", 3;
}

pub(crate) struct Vector {
  pub(crate) name: &'static str,
  pub(crate) target: &'static str,
  pub(crate) frame_kind: u8,
  pub(crate) bytes: Vec<u8>,
  /// the bytes are exactly what the encoder writes
  pub(crate) canonical: bool,
}

impl Vector {
  pub(crate) fn canonical<M: Family>(name: &'static str, _message: &M, bytes: Vec<u8>) -> Self {
    Self {
      name,
      target: M::FUZZ_TARGET,
      frame_kind: M::FRAME_KIND,
      bytes,
      canonical: true,
    }
  }

  pub(crate) fn decode_only<M: Family>(name: &'static str, bytes: Vec<u8>) -> Self {
    Self {
      name,
      target: M::FUZZ_TARGET,
      frame_kind: M::FRAME_KIND,
      bytes,
      canonical: false,
    }
  }

  /// the message code from the frame header
  fn code(&self) -> u32 {
    match self.frame_kind {
      0 | 1 => u32::from_le_bytes(self.bytes[4..8].try_into().unwrap()),
      _ => u32::from(self.bytes[4]),
    }
  }
}

pub(crate) fn check<M: Family>(message: M, bytes: &[u8]) {
  assert_eq!(M::decode(bytes), Ok(message.clone()), "decode");
  assert_eq!(message.encode(), bytes, "encode");
}

/// `name: message => bytes;` becomes a test that the two convert both ways, and an entry in
/// the module's `vectors()`
macro_rules! vectors {
  ($($name:ident: $message:expr => $bytes:expr;)*) => {
    $(
      #[test]
      fn $name() {
        $crate::testing::check($message, &$bytes);
      }
    )*

    pub(crate) fn vectors() -> Vec<$crate::testing::Vector> {
      vec![$($crate::testing::Vector::canonical(stringify!($name), &$message, $bytes),)*]
    }
  };
}
pub(crate) use vectors;

/// `name: Family, bytes => expected;` for input that is accepted but is not what the encoder
/// writes, or that must be rejected
macro_rules! decodes {
  ($($name:ident: $ty:ty, $bytes:expr => $expected:expr;)*) => {
    $(
      #[test]
      fn $name() {
        assert_eq!(<$ty as $crate::testing::Family>::decode(&$bytes), $expected);
      }
    )*

    pub(crate) fn decode_vectors() -> Vec<$crate::testing::Vector> {
      vec![$($crate::testing::Vector::decode_only::<$ty>(stringify!($name), $bytes),)*]
    }
  };
}
pub(crate) use decodes;

fn all_vectors() -> Vec<Vector> {
  use crate::server::{
    distributed, interests, messaging, obsolete, peers, private_rooms, privileges, rooms, search,
    session, users,
  };
  use crate::{init, peer};
  [
    crate::server::tests::decode_vectors(),
    session::tests::vectors(),
    session::tests::decode_vectors(),
    users::tests::vectors(),
    users::tests::decode_vectors(),
    peers::tests::vectors(),
    peers::tests::decode_vectors(),
    rooms::tests::vectors(),
    rooms::tests::decode_vectors(),
    private_rooms::tests::vectors(),
    messaging::tests::vectors(),
    messaging::tests::decode_vectors(),
    search::tests::vectors(),
    distributed::tests::vectors(),
    privileges::tests::vectors(),
    interests::tests::vectors(),
    interests::tests::decode_vectors(),
    obsolete::tests::vectors(),
    peer::browse::tests::vectors(),
    peer::browse::tests::decode_vectors(),
    peer::search::tests::vectors(),
    peer::search::tests::decode_vectors(),
    peer::info::tests::vectors(),
    peer::info::tests::decode_vectors(),
    peer::transfers::tests::vectors(),
    peer::transfers::tests::decode_vectors(),
    crate::distributed::tests::vectors(),
    crate::distributed::tests::decode_vectors(),
    init::tests::vectors(),
    init::tests::decode_vectors(),
  ]
  .into_iter()
  .flatten()
  .collect()
}

#[test]
fn every_code_with_a_layout_has_a_round_trip_vector() {
  let vectors = all_vectors();
  let has = |target: &str, code: u32| {
    vectors
      .iter()
      .any(|v| v.canonical && v.target == target && v.code() == code)
  };
  let mut missing = Vec::new();
  for code in ServerCode::ALL {
    if !has("server_message", code.code()) {
      missing.push(format!("server {}", code.name()));
    }
  }
  // named by the protocol reference without a layout
  let no_layout = [1, 10, 14, 33, 34, 47, 48, 49];
  for code in PeerCode::ALL {
    if !no_layout.contains(&code.code()) && !has("peer_message", code.code()) {
      missing.push(format!("peer {}", code.name()));
    }
  }
  for code in DistributedCode::ALL {
    // a placeholder of the original library, not a wire code
    if *code != DistributedCode::Unknown && !has("distributed_message", code.code()) {
      missing.push(format!("distributed {}", code.name()));
    }
  }
  for code in InitCode::ALL {
    if !has("peer_init", code.code()) {
      missing.push(format!("init {}", code.name()));
    }
  }
  assert!(
    missing.is_empty(),
    "codes without a round-trip vector: {missing:#?}"
  );
}

/// names repeat across families (`search_request` is a peer and a distributed message), so
/// corpus files are keyed by target and name
#[test]
fn vector_names_are_unique_per_target() {
  let mut names: Vec<_> = all_vectors().iter().map(|v| (v.target, v.name)).collect();
  names.sort();
  let before = names.len();
  names.dedup();
  assert_eq!(names.len(), before);
}

/// Writes every vector into `fuzz/corpus/<target>/` as seeds. Run through `just fuzz-seed`.
#[test]
#[ignore = "writes the fuzz seed corpus; run by `just fuzz-seed`"]
fn write_fuzz_corpus() {
  let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus");
  let write = |target: &str, name: &str, bytes: &[u8]| {
    let dir = corpus.join(target);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
  };
  for vector in all_vectors() {
    write(vector.target, vector.name, &vector.bytes);
    // the frame target takes a frame kind byte, then a stream of frames
    let frame_name = format!("{}_{}", vector.target, vector.name);
    write(
      "frame",
      &frame_name,
      &[&[vector.frame_kind][..], &vector.bytes].concat(),
    );
    // the compressed targets take a mode byte: bit 0 = raw zlib stream rather than a body to
    // compress, bit 1 = folder contents rather than browse
    let compressed = match (vector.target, vector.code()) {
      ("peer_message", 9) => Some(("compressed_search_response", 0)),
      ("peer_message", 5) => Some(("compressed_browse_response", 0)),
      ("peer_message", 37) => Some(("compressed_browse_response", 2)),
      _ => None,
    };
    if let Some((target, mode)) = compressed {
      let stream = &vector.bytes[8..];
      write(
        target,
        &format!("{}_zlib", vector.name),
        &[&[mode | 1][..], stream].concat(),
      );
      if let Some(body) = inflate(stream) {
        write(
          target,
          &format!("{}_body", vector.name),
          &[&[mode][..], &body].concat(),
        );
      }
    }
  }
}
