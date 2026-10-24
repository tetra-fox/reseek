//! Framing (§01 §2): `[u32 len][code][payload]`, where the code is 4 bytes on server and peer
//! connections and 1 byte on distributed connections and in the init frame.

use crate::error::DecodeError;
use crate::limits::Limits;

/// Which code width and length limit apply to a frame. A transfer (`F`) socket carries only its
/// init frame; the raw token and offset after it are in [`crate::init`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameKind {
  Server,
  Peer,
  Distributed,
  /// the first frame on an inbound `P`, `F` or `D` socket
  Init,
}

impl FrameKind {
  pub fn code_width(self) -> usize {
    match self {
      Self::Server | Self::Peer => 4,
      Self::Distributed | Self::Init => 1,
    }
  }
}

/// `len` counts the code and the payload, not the 4-byte prefix
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
  pub len: u32,
  pub code: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
  pub code: u32,
  pub payload: &'a [u8],
}

/// Reads the length and code as soon as they are buffered, before the payload arrives (the
/// browse progress hook needs them). `Ok(None)` means more bytes are needed. A length below the
/// code width or above the limit is an error and the connection should be closed.
pub fn header(
  kind: FrameKind,
  buf: &[u8],
  limits: &Limits,
) -> Result<Option<FrameHeader>, DecodeError> {
  let Some(prefix) = buf.first_chunk::<4>() else {
    return Ok(None);
  };
  let len = u32::from_le_bytes(*prefix);
  let code_width = kind.code_width();
  if (len as usize) < code_width {
    tracing::warn!(?kind, len, "frame shorter than its code, rejected");
    return Err(DecodeError::FrameTooShort { len, code_width });
  }
  let limit = limits.frame_bytes(kind);
  if len > limit {
    tracing::warn!(?kind, len, limit, "frame over the length limit, rejected");
    return Err(DecodeError::FrameTooLarge { len, limit });
  }
  let Some(code) = buf.get(4..4 + code_width) else {
    return Ok(None);
  };
  let code = code
    .iter()
    .rev()
    .fold(0, |code, &byte| code << 8 | u32::from(byte));
  Ok(Some(FrameHeader { len, code }))
}

/// Splits the first complete frame off `buf`, returning it with the number of bytes it took.
/// `Ok(None)` means more bytes are needed.
pub fn split<'a>(
  kind: FrameKind,
  buf: &'a [u8],
  limits: &Limits,
) -> Result<Option<(Frame<'a>, usize)>, DecodeError> {
  let Some(FrameHeader { len, code }) = header(kind, buf, limits)? else {
    return Ok(None);
  };
  let consumed = 4 + len as usize;
  let Some(frame) = buf.get(4 + kind.code_width()..consumed) else {
    return Ok(None);
  };
  tracing::trace!(?kind, len, code, "frame read");
  Ok(Some((
    Frame {
      code,
      payload: frame,
    },
    consumed,
  )))
}

/// the single frame that must make up all of `buf`, for the message decoders
pub(crate) fn exact<'a>(
  kind: FrameKind,
  buf: &'a [u8],
  limits: &Limits,
) -> Result<Frame<'a>, DecodeError> {
  let error = match split(kind, buf, limits)? {
    Some((frame, consumed)) if consumed == buf.len() => return Ok(frame),
    Some((_, consumed)) => DecodeError::TrailingBytes {
      extra: buf.len() - consumed,
    },
    None => {
      let needed = match buf.first_chunk::<4>() {
        Some(prefix) => 4 + u32::from_le_bytes(*prefix) as usize,
        None => 4 + kind.code_width(),
      };
      DecodeError::IncompleteFrame {
        needed,
        available: buf.len(),
      }
    }
  };
  tracing::warn!(?kind, error = %error, "frame rejected");
  Err(error)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::limits::{LIMIT_DISTRIBUTED_MESSAGE_BYTES, LIMIT_SERVER_MESSAGE_BYTES};

  const LIMITS: Limits = Limits::DEFAULT;

  #[test]
  fn splits_back_to_back_frames() {
    let buf = [5, 0, 0, 0, 32, 0, 0, 0, 7, 1, 0, 0, 0, 4];
    let (frame, consumed) = split(FrameKind::Server, &buf, &LIMITS).unwrap().unwrap();
    assert_eq!(
      frame,
      Frame {
        code: 32,
        payload: &[7]
      }
    );
    assert_eq!(consumed, 9);
    let (frame, consumed) = split(FrameKind::Distributed, &buf[9..], &LIMITS)
      .unwrap()
      .unwrap();
    assert_eq!(
      frame,
      Frame {
        code: 4,
        payload: &[]
      }
    );
    assert_eq!(consumed, 5);
  }

  #[test]
  fn partial_frames_need_more() {
    let buf = [5, 0, 0, 0, 32, 0, 0, 0, 7];
    for end in 0..buf.len() {
      assert_eq!(
        split(FrameKind::Server, &buf[..end], &LIMITS),
        Ok(None),
        "prefix {end}"
      );
    }
  }

  #[test]
  fn header_is_available_before_the_payload() {
    let buf = [100, 0, 0, 0, 5, 0, 0, 0];
    assert_eq!(
      header(FrameKind::Peer, &buf, &LIMITS),
      Ok(Some(FrameHeader { len: 100, code: 5 }))
    );
    assert_eq!(split(FrameKind::Peer, &buf, &LIMITS), Ok(None));
  }

  #[test]
  fn empty_payload_is_valid_and_shorter_than_the_code_is_not() {
    assert!(
      split(FrameKind::Server, &[4, 0, 0, 0, 32, 0, 0, 0], &LIMITS)
        .unwrap()
        .is_some()
    );
    assert_eq!(
      split(FrameKind::Server, &[3, 0, 0, 0, 32, 0, 0], &LIMITS),
      Err(DecodeError::FrameTooShort {
        len: 3,
        code_width: 4
      })
    );
    assert_eq!(
      split(FrameKind::Distributed, &[0, 0, 0, 0], &LIMITS),
      Err(DecodeError::FrameTooShort {
        len: 0,
        code_width: 1
      })
    );
  }

  #[test]
  fn length_over_the_limit_is_rejected_from_the_prefix_alone() {
    let len = LIMIT_SERVER_MESSAGE_BYTES + 1;
    assert_eq!(
      split(FrameKind::Server, &len.to_le_bytes(), &LIMITS),
      Err(DecodeError::FrameTooLarge {
        len,
        limit: LIMIT_SERVER_MESSAGE_BYTES
      })
    );
    let len = LIMIT_DISTRIBUTED_MESSAGE_BYTES + 1;
    assert!(matches!(
      split(FrameKind::Distributed, &len.to_le_bytes(), &LIMITS),
      Err(DecodeError::FrameTooLarge { .. })
    ));
    let small = Limits {
      peer_message_bytes: 16,
      ..Limits::DEFAULT
    };
    assert!(matches!(
      split(FrameKind::Peer, &17u32.to_le_bytes(), &small),
      Err(DecodeError::FrameTooLarge { len: 17, limit: 16 })
    ));
  }
}
