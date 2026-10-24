//! Search requests between peers and the compressed search response (spec 01 sections 6.2 to 6.4).

use std::io::Read;

use flate2::bufread::ZlibDecoder;

use super::browse::{File, read_files};
use super::{read_compressed, write_compressed, zlib_error};
use crate::codec::{Wire, wire_struct};
use crate::codes::PeerCode;
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::{LIMIT_STRING_BYTES, Limits};
use crate::log::{self, KeyFields};
use crate::wire::{Reader, Writer};

wire_struct! {
  /// Peer code 8, spec 01 section 6.2, obsolete, still answered
  pub struct SearchRequest {
    pub token: u32,
    pub query: String,
  }
}

/// Peer code 9, spec 01 section 6.3
///
/// The `SearchResponse` body
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct SearchResponse {
  /// the responder's username
  pub username: String,
  pub token: u32,
  #[cfg_attr(test, proptest(strategy = "crate::proptests::small_vec::<File>()"))]
  pub files: Vec<File>,
  pub has_free_upload_slot: bool,
  /// bytes/second
  pub upload_speed: u32,
  pub queue_length: u32,
  /// files shared only with selected users, not downloadable
  #[cfg_attr(test, proptest(strategy = "crate::proptests::small_vec::<File>()"))]
  pub locked_files: Vec<File>,
}

impl Wire for SearchResponse {
  const MIN_BYTES: usize = 0;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    read_compressed(r, |r| {
      let username = r.string()?;
      let token = r.u32()?;
      let files = read_files(r)?;
      let has_free_upload_slot = r.bool()?;
      let upload_speed = r.u32()?;
      let queue_length = r.u32()?;
      let mut locked_files = Vec::new();
      // `>= 4` rather than "has more" so a stray trailing byte is not a read error
      if r.remaining() >= 4 {
        // always 0, discarded
        r.u32()?;
        if r.remaining() >= 4 {
          locked_files = read_files(r)?;
        }
      }
      Ok(Self {
        username,
        token,
        files,
        has_free_upload_slot,
        upload_speed,
        queue_length,
        locked_files,
      })
    })
  }

  fn write(&self, w: &mut Writer) {
    write_compressed(w, |w| {
      w.string(&self.username);
      w.u32(self.token);
      self.files.write(w);
      w.bool(self.has_free_upload_slot);
      w.u32(self.upload_speed);
      w.u32(self.queue_length);
      w.u32(0);
      self.locked_files.write(w);
    });
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
    fields.token = Some(self.token);
    fields.count = Some(self.files.len() + self.locked_files.len());
  }
}

/// Inflates only the `username` and `token` that start a `SearchResponse` frame, so a response
/// whose token is not a live search can be dropped before the rest is inflated
pub fn peek_search_response(bytes: &[u8], limits: &Limits) -> Result<(String, u32), DecodeError> {
  let frame = frame::exact(FrameKind::Peer, bytes, limits)?;
  if frame.code != PeerCode::SearchResponse.code() {
    return Err(DecodeError::InvalidValue {
      what: "search response code",
      value: frame.code,
    });
  }
  peek_prefix(frame.payload, limits).inspect_err(|error| {
    log::rejected(
      "peer",
      frame.code,
      Some(PeerCode::SearchResponse.name()),
      error,
    )
  })
}

fn peek_prefix(compressed: &[u8], limits: &Limits) -> Result<(String, u32), DecodeError> {
  let mut decoder = ZlibDecoder::new(compressed);
  let mut prefix = Vec::new();
  (&mut decoder)
    .take(4)
    .read_to_end(&mut prefix)
    .map_err(zlib_error)?;
  let len = Reader::new(&prefix, limits).u32()?;
  if len as usize > LIMIT_STRING_BYTES {
    return Err(DecodeError::StringTooLong {
      len,
      limit: LIMIT_STRING_BYTES,
    });
  }
  decoder
    .take(u64::from(len) + 4)
    .read_to_end(&mut prefix)
    .map_err(zlib_error)?;
  let mut r = Reader::new(&prefix, limits);
  Ok((r.string()?, r.u32()?))
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::peer::PeerMessage;
  use crate::peer::browse::tests::{flac, flac_bytes, mp3, mp3_bytes};
  use crate::testing::{decodes, p, vectors};

  vectors! {
    search_request: PeerMessage::SearchRequest(SearchRequest { token: 3, query: "a".into() }) => p().u32(3).s("a").frame(8);
    search_response:
      PeerMessage::SearchResponse(SearchResponse {
        username: "bob".into(),
        token: 3,
        files: vec![mp3("@@share\\artist\\01 intro.mp3")],
        has_free_upload_slot: true,
        upload_speed: 51_200,
        queue_length: 4,
        locked_files: vec![flac("@@share\\locked\\x.flac")],
      })
      => flac_bytes(
        mp3_bytes(p().s("bob").u32(3).u32(1), "@@share\\artist\\01 intro.mp3").u8(1).u32(51_200).u32(4).u32(0).u32(1),
        "@@share\\locked\\x.flac",
      ).zframe(9);
  }

  decodes! {
    search_response_legacy_without_trailer: PeerMessage, p().s("bob").u32(3).u32(0).u8(0).u32(10).u32(2).zframe(9)
      => Ok(PeerMessage::SearchResponse(SearchResponse {
        username: "bob".into(), token: 3, files: vec![], has_free_upload_slot: false, upload_speed: 10, queue_length: 2,
        locked_files: vec![],
      }));
    search_response_eight_byte_queue_length: PeerMessage, p().s("bob").u32(3).u32(0).u8(1).u32(10).u64(2).zframe(9)
      => Ok(PeerMessage::SearchResponse(SearchResponse {
        username: "bob".into(), token: 3, files: vec![], has_free_upload_slot: true, upload_speed: 10, queue_length: 2,
        locked_files: vec![],
      }));
    search_response_ignores_a_stray_trailing_byte: PeerMessage, p().s("bob").u32(3).u32(0).u8(1).u32(10).u32(2).u8(0).zframe(9)
      => Ok(PeerMessage::SearchResponse(SearchResponse {
        username: "bob".into(), token: 3, files: vec![], has_free_upload_slot: true, upload_speed: 10, queue_length: 2,
        locked_files: vec![],
      }));
    search_response_rejects_too_many_attributes: PeerMessage,
      p().s("bob").u32(3).u32(1).u8(1).s("f").u64(1).s("").u32(65).raw(&[0; 65 * 8]).zframe(9)
      => Err(DecodeError::LimitExceeded { what: "attributes of one file", value: 65, limit: 64 });
    search_response_rejects_a_file_count_past_the_body: PeerMessage, p().s("bob").u32(3).u32(1_000_000).zframe(9)
      => Err(DecodeError::CountTooLarge { what: "files", count: 1_000_000, remaining: 0 });
  }

  #[test]
  fn search_response_peek_stops_before_the_files() {
    // the file list is garbage, but the prefix is enough to drop an unknown token
    let frame = p()
      .s("bob")
      .u32(42)
      .u32(u32::MAX)
      .raw(&[0xff; 16])
      .zframe(9);
    assert_eq!(
      peek_search_response(&frame, &Limits::DEFAULT),
      Ok(("bob".into(), 42))
    );
    assert!(PeerMessage::decode(&frame).is_err());
    let short = p().s("bob").zframe(9);
    assert_eq!(
      peek_search_response(&short, &Limits::DEFAULT),
      Err(DecodeError::Truncated {
        needed: 4,
        remaining: 0
      })
    );
    let wrong_code = p().s("bob").u32(42).zframe(5);
    assert!(peek_search_response(&wrong_code, &Limits::DEFAULT).is_err());
  }

  #[test]
  fn search_response_honours_configured_limits() {
    let response = PeerMessage::SearchResponse(SearchResponse {
      username: "bob".into(),
      token: 1,
      files: vec![mp3("a.mp3"), mp3("b.mp3")],
      has_free_upload_slot: true,
      upload_speed: 0,
      queue_length: 0,
      locked_files: vec![mp3("c.mp3")],
    });
    let frame = response.encode();
    let files = Limits {
      files_per_response: 2,
      ..Limits::DEFAULT
    };
    assert_eq!(
      PeerMessage::decode_with(&frame, &files),
      Err(DecodeError::LimitExceeded {
        what: "files in one response",
        value: 3,
        limit: 2
      })
    );
    let inflated = Limits {
      inflated_bytes: 16,
      ..Limits::DEFAULT
    };
    assert_eq!(
      PeerMessage::decode_with(&frame, &inflated),
      Err(DecodeError::InflatedTooLarge { limit: 16 })
    );
    let frame_len = Limits {
      peer_message_bytes: 16,
      ..Limits::DEFAULT
    };
    assert!(matches!(
      PeerMessage::decode_with(&frame, &frame_len),
      Err(DecodeError::FrameTooLarge { limit: 16, .. })
    ));
    assert_eq!(
      PeerMessage::decode_with(&frame, &Limits::DEFAULT),
      Ok(response)
    );
  }
}
