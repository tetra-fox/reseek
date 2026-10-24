//! The zlib-compressed peer payloads (spec 01 sections 2.2, 6.1, 6.3, 6.6) and the `File` and
//! `Directory` layouts they share (6.8).

use std::io::Read;

use flate2::Compression;
use flate2::bufread::{ZlibDecoder, ZlibEncoder};

use crate::error::DecodeError;
use crate::limits::{
  LIMIT_ATTRIBUTES_PER_FILE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_STRING_BYTES, Limits,
};
use crate::model::{
  BrowseResponse, Directory, File, FileAttribute, FileAttributeType, SearchResponse,
};
use crate::wire::{Reader, RemoteFilename, Writer};

/// u8 code, string filename, u64 size, string extension, u32 attribute count
const MIN_FILE_BYTES: usize = 21;
/// string name, u32 file count
const MIN_DIRECTORY_BYTES: usize = 8;
const ATTRIBUTE_BYTES: usize = 8;

/// The body of `Peer.FolderContentsResponse` (37)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct FolderContentsResponse {
  pub token: u32,
  /// echoed from the request
  pub directory_name: RemoteFilename,
  /// the requested directory, and in some clients each subdirectory as a further entry
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<Directory>()")
  )]
  pub directories: Vec<Directory>,
}

pub(crate) fn compress(body: &[u8]) -> Vec<u8> {
  let mut compressed = Vec::new();
  ZlibEncoder::new(body, Compression::default())
    .read_to_end(&mut compressed)
    .expect("compressing from a slice into a vec does not fail");
  compressed
}

/// Inflates the whole payload, failing once it grows past `limit` bytes or when the stream is
/// corrupt or ends before its end marker. Bytes after the end marker are ignored.
pub(crate) fn inflate(compressed: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
  let mut inflated = Vec::new();
  ZlibDecoder::new(compressed)
    .take(limit as u64 + 1)
    .read_to_end(&mut inflated)
    .map_err(zlib_error)?;
  if inflated.len() > limit {
    return Err(DecodeError::InflatedTooLarge { limit });
  }
  Ok(inflated)
}

fn zlib_error(error: std::io::Error) -> DecodeError {
  // flate2 reports a stream that ends early as UnexpectedEof and anything else as InvalidInput
  match error.kind() {
    std::io::ErrorKind::UnexpectedEof => DecodeError::TruncatedZlib,
    _ => DecodeError::InvalidZlib,
  }
}

/// Remaining file and directory allowance for one response, so the limits apply to the whole
/// message rather than to each list
struct Budget {
  files: usize,
  directories: usize,
}

impl Budget {
  fn new(limits: &Limits) -> Self {
    Self {
      files: limits.files_per_response,
      directories: LIMIT_DIRECTORIES_PER_RESPONSE,
    }
  }

  fn take_files(&mut self, count: usize, limits: &Limits) -> Result<(), DecodeError> {
    self.files = self
      .files
      .checked_sub(count)
      .ok_or(DecodeError::LimitExceeded {
        what: "files in one response",
        value: (limits.files_per_response - self.files + count) as u64,
        limit: limits.files_per_response as u64,
      })?;
    Ok(())
  }

  fn take_directories(&mut self, count: usize) -> Result<(), DecodeError> {
    self.directories = self
      .directories
      .checked_sub(count)
      .ok_or(DecodeError::LimitExceeded {
        what: "directories in one response",
        value: (LIMIT_DIRECTORIES_PER_RESPONSE - self.directories + count) as u64,
        limit: LIMIT_DIRECTORIES_PER_RESPONSE as u64,
      })?;
    Ok(())
  }
}

/// Soulseek NS writes sizes over 2 GiB sign-extended from 32 bits; those are taken as the low
/// 32 bits
fn fix_file_size(size: u64) -> u64 {
  if size >> 32 == 0xffff_ffff {
    size & 0xffff_ffff
  } else {
    size
  }
}

fn read_file(r: &mut Reader) -> Result<File, DecodeError> {
  let code = r.u8()?;
  let filename = r.filename()?;
  let size = fix_file_size(r.u64()?);
  let extension = r.string()?;
  let count = r.count(ATTRIBUTE_BYTES, "file attributes")?;
  if count > LIMIT_ATTRIBUTES_PER_FILE {
    return Err(DecodeError::LimitExceeded {
      what: "attributes of one file",
      value: count as u64,
      limit: LIMIT_ATTRIBUTES_PER_FILE as u64,
    });
  }
  let attributes = (0..count)
    .map(|_| {
      Ok(FileAttribute {
        attribute_type: FileAttributeType::from_u32(r.u32()?),
        value: r.u32()?,
      })
    })
    .collect::<Result<_, DecodeError>>()?;
  Ok(File {
    code,
    filename,
    size,
    extension,
    attributes,
  })
}

fn write_file(w: &mut Writer, file: &File) {
  w.u8(file.code);
  w.filename(&file.filename);
  w.u64(file.size);
  w.string(&file.extension);
  w.count(file.attributes.len());
  for attribute in &file.attributes {
    w.u32(attribute.attribute_type.to_u32());
    w.u32(attribute.value);
  }
}

fn read_files(
  r: &mut Reader,
  budget: &mut Budget,
  limits: &Limits,
) -> Result<Vec<File>, DecodeError> {
  let count = r.count(MIN_FILE_BYTES, "files")?;
  budget.take_files(count, limits)?;
  (0..count).map(|_| read_file(r)).collect()
}

fn write_files(w: &mut Writer, files: &[File]) {
  w.count(files.len());
  for file in files {
    write_file(w, file);
  }
}

fn read_directories(
  r: &mut Reader,
  budget: &mut Budget,
  limits: &Limits,
) -> Result<Vec<Directory>, DecodeError> {
  let count = r.count(MIN_DIRECTORY_BYTES, "directories")?;
  budget.take_directories(count)?;
  (0..count)
    .map(|_| {
      Ok(Directory {
        name: r.filename()?,
        files: read_files(r, budget, limits)?,
      })
    })
    .collect()
}

fn write_directories(w: &mut Writer, directories: &[Directory]) {
  w.count(directories.len());
  for directory in directories {
    w.filename(&directory.name);
    write_files(w, &directory.files);
  }
}

pub(crate) fn read_browse_response(
  compressed: &[u8],
  limits: &Limits,
) -> Result<BrowseResponse, DecodeError> {
  let body = inflate(compressed, limits.inflated_bytes)?;
  let mut r = Reader::new(&body);
  let mut budget = Budget::new(limits);
  let directories = read_directories(&mut r, &mut budget, limits)?;
  let mut locked_directories = Vec::new();
  if r.has_more() {
    // always 0, discarded
    r.u32()?;
    if r.has_more() {
      locked_directories = read_directories(&mut r, &mut budget, limits)?;
    }
  }
  Ok(BrowseResponse {
    directories,
    locked_directories,
  })
}

pub(crate) fn write_browse_response(w: &mut Writer, response: &BrowseResponse) {
  let mut body = Writer::body();
  write_directories(&mut body, &response.directories);
  body.u32(0);
  write_directories(&mut body, &response.locked_directories);
  w.bytes(&compress(&body.into_body()));
}

pub(crate) fn read_search_response(
  compressed: &[u8],
  limits: &Limits,
) -> Result<SearchResponse, DecodeError> {
  let body = inflate(compressed, limits.inflated_bytes)?;
  let mut r = Reader::new(&body);
  let mut budget = Budget::new(limits);
  let username = r.string()?;
  let token = r.u32()?;
  let files = read_files(&mut r, &mut budget, limits)?;
  let has_free_upload_slot = r.bool()?;
  let upload_speed = r.u32()?;
  let queue_length = r.u32()?;
  let mut locked_files = Vec::new();
  // `>= 4` rather than "has more" so a stray trailing byte is not a read error
  if r.remaining() >= 4 {
    // always 0, discarded
    r.u32()?;
    if r.remaining() >= 4 {
      locked_files = read_files(&mut r, &mut budget, limits)?;
    }
  }
  Ok(SearchResponse {
    username,
    token,
    files,
    has_free_upload_slot,
    upload_speed,
    queue_length,
    locked_files,
  })
}

pub(crate) fn write_search_response(w: &mut Writer, response: &SearchResponse) {
  let mut body = Writer::body();
  body.string(&response.username);
  body.u32(response.token);
  write_files(&mut body, &response.files);
  body.bool(response.has_free_upload_slot);
  body.u32(response.upload_speed);
  body.u32(response.queue_length);
  body.u32(0);
  write_files(&mut body, &response.locked_files);
  w.bytes(&compress(&body.into_body()));
}

pub(crate) fn read_folder_contents_response(
  compressed: &[u8],
  limits: &Limits,
) -> Result<FolderContentsResponse, DecodeError> {
  let body = inflate(compressed, limits.inflated_bytes)?;
  let mut r = Reader::new(&body);
  let mut budget = Budget::new(limits);
  Ok(FolderContentsResponse {
    token: r.u32()?,
    directory_name: r.filename()?,
    directories: read_directories(&mut r, &mut budget, limits)?,
  })
}

pub(crate) fn write_folder_contents_response(w: &mut Writer, response: &FolderContentsResponse) {
  let mut body = Writer::body();
  body.u32(response.token);
  body.filename(&response.directory_name);
  write_directories(&mut body, &response.directories);
  w.bytes(&compress(&body.into_body()));
}

/// Inflates only the `username` and `token` that start a search response body, so a response
/// for a token that is not a live search can be dropped without inflating the rest
pub(crate) fn peek_search_response(compressed: &[u8]) -> Result<(String, u32), DecodeError> {
  let mut decoder = ZlibDecoder::new(compressed);
  let mut prefix = Vec::new();
  (&mut decoder)
    .take(4)
    .read_to_end(&mut prefix)
    .map_err(zlib_error)?;
  let len = Reader::new(&prefix).u32()?;
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
  let mut r = Reader::new(&prefix);
  Ok((r.string()?, r.u32()?))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ns_sign_extended_sizes_are_fixed() {
    assert_eq!(fix_file_size(0xffff_ffff_ffff_ffff), 4_294_967_295);
    assert_eq!(fix_file_size(0xffff_ffff_b9ab_cde9), 0xb9ab_cde9);
    assert_eq!(fix_file_size(0xffff_fffe_0000_0000), 0xffff_fffe_0000_0000);
    assert_eq!(fix_file_size(1 << 63), 1 << 63);
  }

  #[test]
  fn inflate_round_trips_and_honours_the_limit() {
    let body = vec![7; 1000];
    let compressed = compress(&body);
    assert_eq!(compressed[0], 0x78, "zlib header, not raw deflate or gzip");
    assert_eq!(inflate(&compressed, 1000), Ok(body));
    assert_eq!(
      inflate(&compressed, 999),
      Err(DecodeError::InflatedTooLarge { limit: 999 })
    );
  }

  #[test]
  fn truncated_and_corrupt_streams_fail() {
    let compressed = compress(b"hello hello hello");
    assert_eq!(
      inflate(&compressed[..compressed.len() - 1], 1 << 20),
      Err(DecodeError::TruncatedZlib)
    );
    assert_eq!(
      inflate(&compressed[..2], 1 << 20),
      Err(DecodeError::TruncatedZlib)
    );
    assert_eq!(inflate(&[], 1 << 20), Err(DecodeError::TruncatedZlib));
    assert_eq!(
      inflate(b"not zlib at all", 1 << 20),
      Err(DecodeError::InvalidZlib)
    );
    let mut bad_checksum = compressed.clone();
    *bad_checksum.last_mut().unwrap() ^= 1;
    assert_eq!(
      inflate(&bad_checksum, 1 << 20),
      Err(DecodeError::InvalidZlib)
    );
  }

  #[test]
  fn zip_bomb_stops_at_the_limit() {
    let compressed = compress(&vec![0; 64 << 20]);
    assert!(compressed.len() < 100 << 10);
    assert_eq!(
      inflate(&compressed, 1 << 20),
      Err(DecodeError::InflatedTooLarge { limit: 1 << 20 })
    );
  }
}
