//! Primitive encodings (spec 01 section 1): little-endian integers, length-prefixed strings with
//! the utf-8 to iso-8859-1 read fallback, reversed ipv4 addresses.

use std::fmt;
use std::net::Ipv4Addr;

use crate::error::{DecodeError, FieldError};
use crate::frame::FrameKind;
use crate::limits::{
  LIMIT_DESCRIPTION_BYTES, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_STRING_BYTES, Limits,
};

/// A path as a peer sent it. The bytes are what gets echoed back to that peer; `as_str` is for
/// display and is iso-8859-1 decoded (`is_lossy`) when the bytes are not utf-8.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RemoteFilename {
  bytes: Vec<u8>,
  display: String,
  lossy: bool,
}

impl RemoteFilename {
  pub fn from_bytes(bytes: Vec<u8>) -> Self {
    let (display, lossy) = decode_text(&bytes);
    Self {
      bytes,
      display,
      lossy,
    }
  }

  pub fn as_bytes(&self) -> &[u8] {
    &self.bytes
  }

  pub fn as_str(&self) -> &str {
    &self.display
  }

  /// the bytes were not utf-8 and were decoded as iso-8859-1
  pub fn is_lossy(&self) -> bool {
    self.lossy
  }
}

impl From<String> for RemoteFilename {
  fn from(display: String) -> Self {
    Self {
      bytes: display.as_bytes().to_vec(),
      display,
      lossy: false,
    }
  }
}

impl From<&str> for RemoteFilename {
  fn from(display: &str) -> Self {
    Self::from(display.to_owned())
  }
}

impl fmt::Debug for RemoteFilename {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(&self.display, f)
  }
}

impl fmt::Display for RemoteFilename {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.display)
  }
}

/// A password or password hash carried by a message. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
  pub fn new(secret: impl Into<String>) -> Self {
    Self(secret.into())
  }

  pub fn expose(&self) -> &str {
    &self.0
  }
}

impl fmt::Debug for Secret {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Secret(..)")
  }
}

/// strict utf-8, else iso-8859-1 (every byte is the code point of the same value); the flag says
/// the fallback was used
fn decode_text(bytes: &[u8]) -> (String, bool) {
  match std::str::from_utf8(bytes) {
    Ok(text) => (text.to_owned(), false),
    Err(_) => {
      tracing::trace!(
        len = bytes.len(),
        "string is not utf-8, decoded as iso-8859-1"
      );
      (bytes.iter().map(|&b| char::from(b)).collect(), true)
    }
  }
}

/// Reads one message. It carries the message name for errors, the limits and what is left of
/// the per-response file and directory allowance, so the counts of nested lists add up against
/// one limit.
pub(crate) struct Reader<'a> {
  buf: &'a [u8],
  limits: &'a Limits,
  message: &'static str,
  files_left: usize,
  directories_left: usize,
}

impl<'a> Reader<'a> {
  pub(crate) fn new(buf: &'a [u8], limits: &'a Limits, message: &'static str) -> Self {
    Self {
      buf,
      limits,
      message,
      files_left: limits.files_per_response,
      directories_left: LIMIT_DIRECTORIES_PER_RESPONSE,
    }
  }

  pub(crate) fn limits(&self) -> &'a Limits {
    self.limits
  }

  pub(crate) fn message(&self) -> &'static str {
    self.message
  }

  /// the error for `field` of the message being read
  pub(crate) fn fail(&self, field: &'static str, error: FieldError) -> DecodeError {
    DecodeError::Field {
      message: self.message,
      field,
      error,
    }
  }

  pub(crate) fn take_files(
    &mut self,
    count: usize,
    field: &'static str,
  ) -> Result<(), DecodeError> {
    let limit = self.limits.files_per_response;
    let Some(left) = self.files_left.checked_sub(count) else {
      return Err(self.fail(
        field,
        FieldError::LimitExceeded {
          value: (limit - self.files_left + count) as u64,
          limit: limit as u64,
        },
      ));
    };
    self.files_left = left;
    Ok(())
  }

  pub(crate) fn take_directories(
    &mut self,
    count: usize,
    field: &'static str,
  ) -> Result<(), DecodeError> {
    let limit = LIMIT_DIRECTORIES_PER_RESPONSE;
    let Some(left) = self.directories_left.checked_sub(count) else {
      return Err(self.fail(
        field,
        FieldError::LimitExceeded {
          value: (limit - self.directories_left + count) as u64,
          limit: limit as u64,
        },
      ));
    };
    self.directories_left = left;
    Ok(())
  }

  pub(crate) fn remaining(&self) -> usize {
    self.buf.len()
  }

  pub(crate) fn has_more(&self) -> bool {
    !self.buf.is_empty()
  }

  pub(crate) fn bytes(&mut self, n: usize, field: &'static str) -> Result<&'a [u8], DecodeError> {
    if n > self.buf.len() {
      return Err(self.fail(
        field,
        FieldError::Truncated {
          needed: n,
          remaining: self.buf.len(),
        },
      ));
    }
    let (head, tail) = self.buf.split_at(n);
    self.buf = tail;
    Ok(head)
  }

  /// the next `n` bytes, or fewer if fewer remain, without consuming them
  pub(crate) fn peek(&self, n: usize) -> &'a [u8] {
    &self.buf[..n.min(self.buf.len())]
  }

  /// everything up to the end of the message
  pub(crate) fn rest(&mut self) -> &'a [u8] {
    std::mem::take(&mut self.buf)
  }

  fn array<const N: usize>(&mut self, field: &'static str) -> Result<[u8; N], DecodeError> {
    let mut array = [0; N];
    array.copy_from_slice(self.bytes(N, field)?);
    Ok(array)
  }

  pub(crate) fn u8(&mut self, field: &'static str) -> Result<u8, DecodeError> {
    Ok(self.array::<1>(field)?[0])
  }

  pub(crate) fn u16(&mut self, field: &'static str) -> Result<u16, DecodeError> {
    self.array(field).map(u16::from_le_bytes)
  }

  pub(crate) fn u32(&mut self, field: &'static str) -> Result<u32, DecodeError> {
    self.array(field).map(u32::from_le_bytes)
  }

  pub(crate) fn i32(&mut self, field: &'static str) -> Result<i32, DecodeError> {
    self.array(field).map(i32::from_le_bytes)
  }

  pub(crate) fn u64(&mut self, field: &'static str) -> Result<u64, DecodeError> {
    self.array(field).map(u64::from_le_bytes)
  }

  /// any non-zero byte is true
  pub(crate) fn bool(&mut self, field: &'static str) -> Result<bool, DecodeError> {
    Ok(self.u8(field)? != 0)
  }

  /// only 1 is true; `Login.success`, `Login.is_supporter`, `TransferResponse.allowed` and
  /// `PrivateMessage.is_new` read this way
  pub(crate) fn bool_exact(&mut self, field: &'static str) -> Result<bool, DecodeError> {
    Ok(self.u8(field)? == 1)
  }

  /// the wire holds the octets reversed, so the little-endian value is the address as a
  /// big-endian number
  pub(crate) fn ip4(&mut self, field: &'static str) -> Result<Ipv4Addr, DecodeError> {
    self.u32(field).map(Ipv4Addr::from)
  }

  fn raw_string(&mut self, limit: usize, field: &'static str) -> Result<&'a [u8], DecodeError> {
    let len = self.u32(field)?;
    if len as usize > limit {
      return Err(self.fail(field, FieldError::StringTooLong { len, limit }));
    }
    self.bytes(len as usize, field)
  }

  pub(crate) fn string(&mut self, field: &'static str) -> Result<String, DecodeError> {
    self
      .raw_string(LIMIT_STRING_BYTES, field)
      .map(|bytes| decode_text(bytes).0)
  }

  /// the user-info description and the login greeting, which have a larger limit
  pub(crate) fn description(&mut self, field: &'static str) -> Result<String, DecodeError> {
    self
      .raw_string(LIMIT_DESCRIPTION_BYTES, field)
      .map(|bytes| decode_text(bytes).0)
  }

  pub(crate) fn filename(&mut self, field: &'static str) -> Result<RemoteFilename, DecodeError> {
    self
      .raw_string(LIMIT_STRING_BYTES, field)
      .map(|bytes| RemoteFilename::from_bytes(bytes.to_vec()))
  }

  /// a list count, rejected unless `count` elements of at least `min_size` bytes each fit in
  /// what remains
  pub(crate) fn count(
    &mut self,
    min_size: usize,
    field: &'static str,
  ) -> Result<usize, DecodeError> {
    let count = self.u32(field)?;
    let remaining = self.remaining();
    if u64::from(count) * min_size as u64 > remaining as u64 {
      return Err(self.fail(field, FieldError::CountTooLarge { count, remaining }));
    }
    Ok(count as usize)
  }
}

pub(crate) struct Writer {
  buf: Vec<u8>,
}

impl Writer {
  /// a frame with the code width of `kind`; `finish` fills in the length prefix
  pub(crate) fn frame(kind: FrameKind, code: u32) -> Self {
    let mut writer = Self { buf: vec![0; 4] };
    match kind.code_width() {
      1 => writer.u8(code as u8),
      _ => writer.u32(code),
    }
    writer
  }

  /// a bare payload with no length prefix, for compressed bodies
  pub(crate) fn body() -> Self {
    Self { buf: Vec::new() }
  }

  pub(crate) fn finish(mut self) -> Vec<u8> {
    let len = u32::try_from(self.buf.len() - 4).expect("frame longer than u32::MAX bytes");
    self.buf[..4].copy_from_slice(&len.to_le_bytes());
    self.buf
  }

  pub(crate) fn into_body(self) -> Vec<u8> {
    self.buf
  }

  pub(crate) fn bytes(&mut self, bytes: &[u8]) {
    self.buf.extend_from_slice(bytes);
  }

  pub(crate) fn u8(&mut self, value: u8) {
    self.buf.push(value);
  }

  pub(crate) fn u16(&mut self, value: u16) {
    self.bytes(&value.to_le_bytes());
  }

  pub(crate) fn u32(&mut self, value: u32) {
    self.bytes(&value.to_le_bytes());
  }

  pub(crate) fn i32(&mut self, value: i32) {
    self.bytes(&value.to_le_bytes());
  }

  pub(crate) fn u64(&mut self, value: u64) {
    self.bytes(&value.to_le_bytes());
  }

  pub(crate) fn bool(&mut self, value: bool) {
    self.u8(u8::from(value));
  }

  pub(crate) fn ip4(&mut self, ip: Ipv4Addr) {
    self.u32(u32::from(ip));
  }

  pub(crate) fn count(&mut self, count: usize) {
    self.u32(u32::try_from(count).expect("list longer than u32::MAX elements"));
  }

  fn raw_string(&mut self, bytes: &[u8]) {
    self.count(bytes.len());
    self.bytes(bytes);
  }

  pub(crate) fn string(&mut self, value: &str) {
    self.raw_string(value.as_bytes());
  }

  pub(crate) fn filename(&mut self, filename: &RemoteFilename) {
    self.raw_string(filename.as_bytes());
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::field;

  #[test]
  fn invalid_utf8_falls_back_to_latin1_and_keeps_bytes() {
    let bytes = vec![b'c', 0xe9, b'.', b'm', b'p', b'3'];
    let name = RemoteFilename::from_bytes(bytes.clone());
    assert_eq!(name.as_str(), "c\u{e9}.mp3");
    assert!(name.is_lossy());
    assert_eq!(name.as_bytes(), bytes);

    let mut writer = Writer::body();
    writer.filename(&name);
    let mut expected = 6u32.to_le_bytes().to_vec();
    expected.extend_from_slice(&bytes);
    assert_eq!(writer.into_body(), expected);
  }

  #[test]
  fn valid_utf8_is_not_lossy() {
    let name = RemoteFilename::from_bytes("caf\u{e9}".as_bytes().to_vec());
    assert_eq!(name.as_str(), "caf\u{e9}");
    assert!(!name.is_lossy());
  }

  #[test]
  fn ip4_is_reversed_on_the_wire() {
    let mut reader = Reader::new(&[4, 3, 2, 1], &Limits::DEFAULT, "Test");
    assert_eq!(reader.ip4("ip").unwrap(), Ipv4Addr::new(1, 2, 3, 4));
    let mut writer = Writer::body();
    writer.ip4(Ipv4Addr::new(1, 2, 3, 4));
    assert_eq!(writer.into_body(), [4, 3, 2, 1]);
  }

  #[test]
  fn string_longer_than_remaining_is_truncated() {
    let mut reader = Reader::new(&[5, 0, 0, 0, b'a'], &Limits::DEFAULT, "Test");
    assert_eq!(
      reader.string("name"),
      Err(field(
        "Test",
        "name",
        FieldError::Truncated {
          needed: 5,
          remaining: 1
        }
      ))
    );
  }

  #[test]
  fn string_over_limit_is_rejected_before_reading() {
    let len = (LIMIT_STRING_BYTES as u32) + 1;
    let prefix = len.to_le_bytes();
    let mut reader = Reader::new(&prefix, &Limits::DEFAULT, "Test");
    assert_eq!(
      reader.string("name"),
      Err(field(
        "Test",
        "name",
        FieldError::StringTooLong {
          len,
          limit: LIMIT_STRING_BYTES
        }
      ))
    );
  }

  #[test]
  fn count_must_fit_in_remaining_bytes() {
    // 2 strings need at least 8 bytes, only 7 remain
    let mut buf = 2u32.to_le_bytes().to_vec();
    buf.extend_from_slice(&[0; 7]);
    let mut reader = Reader::new(&buf, &Limits::DEFAULT, "Test");
    assert_eq!(
      reader.count(4, "strings"),
      Err(field(
        "Test",
        "strings",
        FieldError::CountTooLarge {
          count: 2,
          remaining: 7
        }
      ))
    );
    // a count with bit 31 set is a protocol error, not zero iterations
    let max = u32::MAX.to_le_bytes();
    let mut reader = Reader::new(&max, &Limits::DEFAULT, "Test");
    assert!(matches!(
      reader.count(1, "x"),
      Err(DecodeError::Field {
        error: FieldError::CountTooLarge { .. },
        ..
      })
    ));
  }

  #[test]
  fn bool_reads_differ_for_values_above_one() {
    let mut reader = Reader::new(&[2, 2], &Limits::DEFAULT, "Test");
    assert!(reader.bool("a").unwrap());
    assert!(!reader.bool_exact("b").unwrap());
  }
}
