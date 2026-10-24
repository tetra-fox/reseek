//! Messages on a `P` connection between two clients (spec 01 section 6), the same layout in both
//! directions. Browse, search and folder responses are one zlib stream after the code; the
//! `File` and `Directory` layouts they share are here too.

use std::io::Read;

use flate2::Compression;
use flate2::bufread::{ZlibDecoder, ZlibEncoder};

use crate::codec::{Wire, family, wire_struct};
use crate::codes::PeerCode;
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::{
  LIMIT_ATTRIBUTES_PER_FILE, LIMIT_STRING_BYTES, LIMIT_USER_INFO_PICTURE_BYTES, Limits,
};
use crate::log::{self, KeyFields};
use crate::model::{FileAttributeType, TransferDirection};
use crate::wire::{Reader, RemoteFilename, Writer};

family! {
  pub enum PeerMessage {
    code: PeerCode,
    frame: Peer,
    label: "peer",
    quiet: [],
    messages: {
      BrowseRequest,
      BrowseResponse(BrowseResponse),
      /// obsolete, still answered
      SearchRequest(SearchRequest),
      SearchResponse(SearchResponse),
      InfoRequest,
      InfoResponse(UserInfo),
      FolderContentsRequest(FolderContentsRequest),
      FolderContentsResponse(FolderContentsResponse),
      TransferRequest(TransferRequest),
      TransferResponse(TransferResponse),
      /// obsolete
      UploadPlacehold(UploadPlacehold),
      /// ask the peer to queue a file for us
      QueueDownload(QueueDownload),
      PlaceInQueueResponse(PlaceInQueueResponse),
      UploadFailed(UploadFailed),
      UploadDenied(UploadDenied),
      PlaceInQueueRequest(PlaceInQueueRequest),
      /// deprecated, empty; Soulseek NS sends it before uploading
      UploadQueueNotification,
    }
  }
}

impl PeerMessage {
  /// decodes one complete frame with configured limits
  pub fn decode_with(bytes: &[u8], limits: &Limits) -> Result<Self, DecodeError> {
    Self::decode_limited(bytes, limits)
  }
}

fn compress(body: &[u8]) -> Vec<u8> {
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

/// inflates the rest of the payload and reads the body from it, with a fresh file and directory
/// allowance
fn read_compressed<T>(
  r: &mut Reader,
  read: impl FnOnce(&mut Reader) -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
  let limits = r.limits();
  let body = inflate(r.rest(), limits.inflated_bytes)?;
  read(&mut Reader::new(&body, limits))
}

fn write_compressed(w: &mut Writer, write: impl FnOnce(&mut Writer)) {
  let mut body = Writer::body();
  write(&mut body);
  w.bytes(&compress(&body.into_body()));
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

wire_struct! {
  pub struct FileAttribute {
    pub attribute_type: FileAttributeType,
    pub value: u32,
  }
}

/// A shared file. In search responses `filename` is the full remote path, in browse and folder
/// responses the bare name inside its directory.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct File {
  /// written as 1 by the daemon, kept as received otherwise
  pub code: u8,
  pub filename: RemoteFilename,
  #[cfg_attr(test, proptest(strategy = "crate::proptests::file_size()"))]
  pub size: u64,
  /// as the sharer computed it, often blank; derive the extension from the filename instead
  pub extension: String,
  /// in wire order; this list, not the typed getters, is what re-encodes
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<FileAttribute>()")
  )]
  pub attributes: Vec<FileAttribute>,
}

impl File {
  pub fn attribute_count(&self) -> usize {
    self.attributes.len()
  }

  /// the last attribute of this type wins when a list repeats one
  fn attribute(&self, attribute_type: FileAttributeType) -> Option<u32> {
    self
      .attributes
      .iter()
      .rev()
      .find(|a| a.attribute_type == attribute_type)
      .map(|a| a.value)
  }

  pub fn bit_rate(&self) -> Option<u32> {
    self.attribute(FileAttributeType::BitRate)
  }

  /// seconds
  pub fn length(&self) -> Option<u32> {
    self.attribute(FileAttributeType::Duration)
  }

  pub fn is_variable_bit_rate(&self) -> Option<bool> {
    self
      .attribute(FileAttributeType::VariableBitRate)
      .map(|v| v != 0)
  }

  pub fn sample_rate(&self) -> Option<u32> {
    self.attribute(FileAttributeType::SampleRate)
  }

  pub fn bit_depth(&self) -> Option<u32> {
    self.attribute(FileAttributeType::BitDepth)
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

impl Wire for File {
  const MIN_BYTES: usize = 21;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let code = r.u8()?;
    let filename = r.filename()?;
    let size = fix_file_size(r.u64()?);
    let extension = r.string()?;
    let count = r.count(FileAttribute::MIN_BYTES, "attributes")?;
    if count > LIMIT_ATTRIBUTES_PER_FILE {
      return Err(DecodeError::LimitExceeded {
        what: "attributes of one file",
        value: count as u64,
        limit: LIMIT_ATTRIBUTES_PER_FILE as u64,
      });
    }
    let attributes = (0..count)
      .map(|_| FileAttribute::read(r))
      .collect::<Result<_, _>>()?;
    Ok(Self {
      code,
      filename,
      size,
      extension,
      attributes,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u8(self.code);
    w.filename(&self.filename);
    w.u64(self.size);
    w.string(&self.extension);
    self.attributes.write(w);
  }
}

/// a file list, counted against the response's file limit
fn read_files(r: &mut Reader) -> Result<Vec<File>, DecodeError> {
  let count = r.count(File::MIN_BYTES, "files")?;
  r.take_files(count)?;
  (0..count).map(|_| File::read(r)).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Directory {
  /// the full remote path with the peer's separator
  pub name: RemoteFilename,
  #[cfg_attr(test, proptest(strategy = "crate::proptests::small_vec::<File>()"))]
  pub files: Vec<File>,
}

impl Directory {
  pub fn file_count(&self) -> usize {
    self.files.len()
  }
}

impl Wire for Directory {
  const MIN_BYTES: usize = 8;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      name: r.filename()?,
      files: read_files(r)?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.filename(&self.name);
    self.files.write(w);
  }
}

/// a directory list, counted against the response's directory limit
fn read_directories(r: &mut Reader) -> Result<Vec<Directory>, DecodeError> {
  let count = r.count(Directory::MIN_BYTES, "directories")?;
  r.take_directories(count)?;
  (0..count).map(|_| Directory::read(r)).collect()
}

/// The `BrowseResponse` body
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct BrowseResponse {
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<Directory>()")
  )]
  pub directories: Vec<Directory>,
  /// shown but not downloadable
  #[cfg_attr(
    test,
    proptest(strategy = "crate::proptests::small_vec::<Directory>()")
  )]
  pub locked_directories: Vec<Directory>,
}

impl Wire for BrowseResponse {
  const MIN_BYTES: usize = 0;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    read_compressed(r, |r| {
      let directories = read_directories(r)?;
      let mut locked_directories = Vec::new();
      if r.has_more() {
        // always 0, discarded
        r.u32()?;
        if r.has_more() {
          locked_directories = read_directories(r)?;
        }
      }
      Ok(Self {
        directories,
        locked_directories,
      })
    })
  }

  fn write(&self, w: &mut Writer) {
    write_compressed(w, |w| {
      self.directories.write(w);
      w.u32(0);
      self.locked_directories.write(w);
    });
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.count = Some(self.directories.len() + self.locked_directories.len());
  }
}

wire_struct! {
  /// obsolete, still answered
  pub struct SearchRequest {
    pub token: u32,
    pub query: String,
  }
}

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

/// The `InfoResponse` body
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserInfo {
  pub description: String,
  /// raw image file bytes
  pub picture: Option<Vec<u8>>,
  pub upload_slots: u32,
  pub queue_length: u32,
  pub has_free_upload_slot: bool,
  /// who may upload to this user: 0 no one, 1 everyone, 2 users in their list, 3 permitted
  /// users; absent from SoulseekQt
  pub upload_permitted: Option<u32>,
}

impl UserInfo {
  pub fn has_picture(&self) -> bool {
    self.picture.is_some()
  }
}

impl Wire for UserInfo {
  const MIN_BYTES: usize = 14;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let description = r.description()?;
    let picture = if r.bool()? {
      let len = r.u32()? as usize;
      if len > LIMIT_USER_INFO_PICTURE_BYTES {
        return Err(DecodeError::LimitExceeded {
          what: "user info picture bytes",
          value: len as u64,
          limit: LIMIT_USER_INFO_PICTURE_BYTES as u64,
        });
      }
      Some(r.bytes(len)?.to_vec())
    } else {
      None
    };
    Ok(Self {
      description,
      picture,
      upload_slots: r.u32()?,
      queue_length: r.u32()?,
      has_free_upload_slot: r.bool()?,
      // Museek+ writes the slot flag as a u32, leaving 3 stray bytes that must not be read as this
      upload_permitted: if r.remaining() >= 4 {
        Some(r.u32()?)
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.description);
    w.bool(self.picture.is_some());
    if let Some(picture) = &self.picture {
      w.count(picture.len());
      w.bytes(picture);
    }
    w.u32(self.upload_slots);
    w.u32(self.queue_length);
    w.bool(self.has_free_upload_slot);
    if let Some(upload_permitted) = self.upload_permitted {
      w.u32(upload_permitted);
    }
  }
}

wire_struct! {
  pub struct FolderContentsRequest {
    pub token: u32,
    /// the remote directory path exactly as it appeared in a browse or search
    pub directory_name: RemoteFilename,
  }
}

/// The `FolderContentsResponse` body
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

impl Wire for FolderContentsResponse {
  const MIN_BYTES: usize = 0;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    read_compressed(r, |r| {
      Ok(Self {
        token: r.u32()?,
        directory_name: r.filename()?,
        directories: read_directories(r)?,
      })
    })
  }

  fn write(&self, w: &mut Writer) {
    write_compressed(w, |w| {
      w.u32(self.token);
      w.filename(&self.directory_name);
      self.directories.write(w);
    });
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.filename = Some(self.directory_name.as_str());
    fields.token = Some(self.token);
    fields.count = Some(self.directories.len());
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferRequest {
  pub direction: TransferDirection,
  /// chosen by the sender
  pub token: u32,
  pub filename: RemoteFilename,
  /// only carried by an upload announcement (`direction == Upload`), and missing from very old
  /// clients there; never written or read for a download request
  pub file_size: Option<u64>,
}

impl Wire for TransferRequest {
  const MIN_BYTES: usize = 12;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let direction = TransferDirection::read(r)?;
    let token = r.u32()?;
    let filename = r.filename()?;
    // a download request from an older client may carry a stray size of 0; it is ignored
    let file_size = match direction {
      TransferDirection::Upload if r.remaining() >= 8 => Some(r.u64()?),
      _ => None,
    };
    Ok(Self {
      direction,
      token,
      filename,
      file_size,
    })
  }

  fn write(&self, w: &mut Writer) {
    self.direction.write(w);
    w.u32(self.token);
    w.filename(&self.filename);
    if let (TransferDirection::Upload, Some(file_size)) = (self.direction, self.file_size) {
      w.u64(file_size);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.filename = Some(self.filename.as_str());
    fields.token = Some(self.token);
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum TransferResponse {
  Allowed {
    token: u32,
    /// present in the deprecated reply to a download request, absent in the reply to an upload
    /// announcement
    file_size: Option<u64>,
  },
  Denied {
    token: u32,
    /// `"Queued"` means accepted into the queue; anything else is display text
    reason: String,
  },
}

impl TransferResponse {
  pub fn token(&self) -> u32 {
    match self {
      Self::Allowed { token, .. } | Self::Denied { token, .. } => *token,
    }
  }
}

impl Wire for TransferResponse {
  const MIN_BYTES: usize = 5;

  /// `allowed` is true only for exactly 1, and a denial must carry a reason
  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let token = r.u32()?;
    Ok(if r.bool_exact()? {
      Self::Allowed {
        token,
        file_size: if r.remaining() >= 8 {
          Some(r.u64()?)
        } else {
          None
        },
      }
    } else {
      Self::Denied {
        token,
        reason: r.string()?,
      }
    })
  }

  fn write(&self, w: &mut Writer) {
    match self {
      Self::Allowed { token, file_size } => {
        w.u32(*token);
        w.bool(true);
        if let Some(file_size) = file_size {
          w.u64(*file_size);
        }
      }
      Self::Denied { token, reason } => {
        w.u32(*token);
        w.bool(false);
        w.string(reason);
      }
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.token = Some(self.token());
  }
}

wire_struct! {
  /// obsolete
  pub struct UploadPlacehold {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  pub struct QueueDownload {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  pub struct PlaceInQueueResponse {
    pub filename: RemoteFilename,
    /// 1-based
    pub place: u32,
  }
}

wire_struct! {
  pub struct UploadFailed {
    pub filename: RemoteFilename,
  }
}

wire_struct! {
  pub struct UploadDenied {
    pub filename: RemoteFilename,
    pub reason: String,
  }
}

wire_struct! {
  pub struct PlaceInQueueRequest {
    pub filename: RemoteFilename,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::limits::LIMIT_DIRECTORIES_PER_RESPONSE;
  use crate::testing::{P, decodes, p, vectors};

  fn attr(attribute_type: FileAttributeType, value: u32) -> FileAttribute {
    FileAttribute {
      attribute_type,
      value,
    }
  }

  fn mp3(filename: &str) -> File {
    File {
      code: 1,
      filename: filename.into(),
      size: 5_242_880,
      extension: "mp3".into(),
      attributes: vec![
        attr(FileAttributeType::BitRate, 320),
        attr(FileAttributeType::Duration, 245),
        attr(FileAttributeType::VariableBitRate, 0),
      ],
    }
  }

  fn mp3_bytes(b: P, filename: &str) -> P {
    b.u8(1)
      .s(filename)
      .u64(5_242_880)
      .s("mp3")
      .u32(3)
      .u32(0)
      .u32(320)
      .u32(1)
      .u32(245)
      .u32(2)
      .u32(0)
  }

  fn flac(filename: &str) -> File {
    File {
      code: 1,
      filename: filename.into(),
      size: 31_457_280,
      extension: String::new(),
      attributes: vec![
        attr(FileAttributeType::Duration, 245),
        attr(FileAttributeType::SampleRate, 44_100),
        attr(FileAttributeType::BitDepth, 16),
        attr(FileAttributeType::Unknown(3), 9),
      ],
    }
  }

  fn flac_bytes(b: P, filename: &str) -> P {
    b.u8(1)
      .s(filename)
      .u64(31_457_280)
      .s("")
      .u32(4)
      .u32(1)
      .u32(245)
      .u32(4)
      .u32(44_100)
      .u32(5)
      .u32(16)
      .u32(3)
      .u32(9)
  }

  vectors! {
    browse_request: PeerMessage::BrowseRequest => p().frame(4);
    browse_response:
      PeerMessage::BrowseResponse(BrowseResponse {
        directories: vec![
          Directory { name: "@@share\\artist\\album".into(), files: vec![mp3("01 intro.mp3"), flac("02 song.flac")] },
          Directory { name: "@@share\\empty".into(), files: vec![] },
        ],
        locked_directories: vec![Directory { name: "@@share\\friends".into(), files: vec![mp3("secret.mp3")] }],
      })
      => flac_bytes(mp3_bytes(p().u32(2).s("@@share\\artist\\album").u32(2), "01 intro.mp3"), "02 song.flac")
        .s("@@share\\empty").u32(0).u32(0).u32(1).s("@@share\\friends").u32(1).raw(&mp3_bytes(p(), "secret.mp3").0)
        .zframe(5);
    browse_response_empty:
      PeerMessage::BrowseResponse(BrowseResponse { directories: vec![], locked_directories: vec![] })
      => p().u32(0).u32(0).u32(0).zframe(5);
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
    info_request: PeerMessage::InfoRequest => p().frame(15);
    info_response:
      PeerMessage::InfoResponse(UserInfo {
        description: "hello".into(),
        picture: Some(vec![0x89, b'P', b'N', b'G']),
        upload_slots: 2,
        queue_length: 0,
        has_free_upload_slot: true,
        upload_permitted: Some(1),
      })
      => p().s("hello").u8(1).u32(4).raw(&[0x89, b'P', b'N', b'G']).u32(2).u32(0).u8(1).u32(1).frame(16);
    info_response_without_picture:
      PeerMessage::InfoResponse(UserInfo {
        description: String::new(),
        picture: None,
        upload_slots: 0,
        queue_length: 2_147_483_647,
        has_free_upload_slot: false,
        upload_permitted: None,
      })
      => p().s("").u8(0).u32(0).u32(2_147_483_647).u8(0).frame(16);
    folder_contents_request:
      PeerMessage::FolderContentsRequest(FolderContentsRequest { token: 8, directory_name: "@@share\\artist".into() })
      => p().u32(8).s("@@share\\artist").frame(36);
    folder_contents_response:
      PeerMessage::FolderContentsResponse(FolderContentsResponse {
        token: 8,
        directory_name: "@@share\\artist".into(),
        directories: vec![
          Directory { name: "@@share\\artist".into(), files: vec![mp3("a.mp3")] },
          Directory { name: "@@share\\artist\\cd2".into(), files: vec![] },
        ],
      })
      => mp3_bytes(p().u32(8).s("@@share\\artist").u32(2).s("@@share\\artist").u32(1), "a.mp3")
        .s("@@share\\artist\\cd2").u32(0).zframe(37);
    transfer_request_download:
      PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Download, token: 5, filename: "@@share\\a.mp3".into(), file_size: None,
      })
      => p().u32(0).u32(5).s("@@share\\a.mp3").frame(40);
    transfer_request_upload:
      PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Upload, token: 5, filename: "@@share\\a.mp3".into(), file_size: Some(5_242_880),
      })
      => p().u32(1).u32(5).s("@@share\\a.mp3").u64(5_242_880).frame(40);
    transfer_response_allowed_with_size:
      PeerMessage::TransferResponse(TransferResponse::Allowed { token: 5, file_size: Some(5_242_880) })
      => p().u32(5).u8(1).u64(5_242_880).frame(41);
    transfer_response_allowed:
      PeerMessage::TransferResponse(TransferResponse::Allowed { token: 5, file_size: None }) => p().u32(5).u8(1).frame(41);
    transfer_response_denied:
      PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, reason: "Queued".into() })
      => p().u32(5).u8(0).s("Queued").frame(41);
    upload_placehold: PeerMessage::UploadPlacehold(UploadPlacehold { filename: "a.mp3".into() }) => p().s("a.mp3").frame(42);
    queue_download:
      PeerMessage::QueueDownload(QueueDownload { filename: "@@share\\a.mp3".into() }) => p().s("@@share\\a.mp3").frame(43);
    queue_download_latin1:
      PeerMessage::QueueDownload(QueueDownload { filename: RemoteFilename::from_bytes(b"caf\xe9\\\xe0.mp3".to_vec()) })
      => p().bytes_string(b"caf\xe9\\\xe0.mp3").frame(43);
    place_in_queue_response:
      PeerMessage::PlaceInQueueResponse(PlaceInQueueResponse { filename: "a.mp3".into(), place: 3 })
      => p().s("a.mp3").u32(3).frame(44);
    upload_failed: PeerMessage::UploadFailed(UploadFailed { filename: "a.mp3".into() }) => p().s("a.mp3").frame(46);
    upload_denied:
      PeerMessage::UploadDenied(UploadDenied { filename: "a.mp3".into(), reason: "File not shared.".into() })
      => p().s("a.mp3").s("File not shared.").frame(50);
    place_in_queue_request:
      PeerMessage::PlaceInQueueRequest(PlaceInQueueRequest { filename: "a.mp3".into() }) => p().s("a.mp3").frame(51);
    upload_queue_notification: PeerMessage::UploadQueueNotification => p().frame(52);
  }

  decodes! {
    browse_response_without_trailer: PeerMessage, p().u32(0).zframe(5)
      => Ok(PeerMessage::BrowseResponse(BrowseResponse { directories: vec![], locked_directories: vec![] }));
    browse_response_fixes_ns_sign_extended_size: PeerMessage,
      p().u32(1).s("d").u32(1).u8(1).s("big.iso").u64(0xffff_ffff_ffff_ffff).s("iso").u32(0).u32(0).u32(0).zframe(5)
      => Ok(PeerMessage::BrowseResponse(BrowseResponse {
        directories: vec![Directory {
          name: "d".into(),
          files: vec![File { code: 1, filename: "big.iso".into(), size: 4_294_967_295, extension: "iso".into(), attributes: vec![] }],
        }],
        locked_directories: vec![],
      }));
    browse_response_rejects_a_stream_that_is_not_zlib: PeerMessage, p().raw(b"plain").frame(5) => Err(DecodeError::InvalidZlib);
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
    info_response_ignores_museek_stray_bytes: PeerMessage, p().s("d").u8(0).u32(1).u32(0).u8(1).raw(&[0, 0, 0]).frame(16)
      => Ok(PeerMessage::InfoResponse(UserInfo {
        description: "d".into(), picture: None, upload_slots: 1, queue_length: 0, has_free_upload_slot: true,
        upload_permitted: None,
      }));
    info_response_rejects_a_picture_over_the_limit: PeerMessage, p().s("").u8(1).u32(16 << 20 | 1).frame(16)
      => Err(DecodeError::LimitExceeded { what: "user info picture bytes", value: 16 << 20 | 1, limit: 16 << 20 });
    transfer_request_download_ignores_a_stray_size: PeerMessage, p().u32(0).u32(5).s("a.mp3").u64(0).frame(40)
      => Ok(PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Download, token: 5, filename: "a.mp3".into(), file_size: None,
      }));
    transfer_request_upload_without_size: PeerMessage, p().u32(1).u32(5).s("a.mp3").frame(40)
      => Ok(PeerMessage::TransferRequest(TransferRequest {
        direction: TransferDirection::Upload, token: 5, filename: "a.mp3".into(), file_size: None,
      }));
    transfer_response_allowed_must_be_exactly_one: PeerMessage, p().u32(5).u8(2).s("Cancelled").frame(41)
      => Ok(PeerMessage::TransferResponse(TransferResponse::Denied { token: 5, reason: "Cancelled".into() }));
    transfer_response_denied_needs_a_reason: PeerMessage, p().u32(5).u8(0).frame(41)
      => Err(DecodeError::Truncated { needed: 4, remaining: 0 });
  }

  #[test]
  fn file_last_attribute_of_a_type_wins() {
    let file = File {
      attributes: vec![
        attr(FileAttributeType::Duration, 10),
        attr(FileAttributeType::Unknown(3), 7),
        attr(FileAttributeType::SampleRate, 44_100),
        attr(FileAttributeType::Duration, 11),
      ],
      ..flac("a.flac")
    };
    assert_eq!(file.length(), Some(11));
    assert_eq!(file.sample_rate(), Some(44_100));
    assert_eq!(file.bit_rate(), None);
    assert_eq!(file.attribute_count(), 4);
  }

  #[test]
  fn file_ns_sign_extended_sizes_are_fixed() {
    assert_eq!(fix_file_size(0xffff_ffff_ffff_ffff), 4_294_967_295);
    assert_eq!(fix_file_size(0xffff_ffff_b9ab_cde9), 0xb9ab_cde9);
    assert_eq!(fix_file_size(0xffff_fffe_0000_0000), 0xffff_fffe_0000_0000);
    assert_eq!(fix_file_size(1 << 63), 1 << 63);
  }

  #[test]
  fn queue_download_latin1_filename_is_echoed_with_its_bytes() {
    let bytes = p().bytes_string(b"caf\xe9.mp3").frame(43);
    let PeerMessage::QueueDownload(QueueDownload { filename }) =
      PeerMessage::decode(&bytes).unwrap()
    else {
      panic!("not a queue download");
    };
    assert_eq!(filename.as_str(), "caf\u{e9}.mp3");
    assert!(filename.is_lossy());
    let echo = PeerMessage::PlaceInQueueRequest(PlaceInQueueRequest { filename }).encode();
    assert_eq!(echo, p().bytes_string(b"caf\xe9.mp3").frame(51));
  }

  #[test]
  fn codes_without_a_layout_decode_as_unknown() {
    for code in [1, 10, 14, 33, 34, 47, 48, 49] {
      assert_eq!(
        PeerMessage::decode(&p().u32(1).frame(code)),
        Err(DecodeError::UnknownCode { code })
      );
    }
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

  #[test]
  fn browse_response_directory_limit_counts_every_list() {
    let mut body = p().u32(LIMIT_DIRECTORIES_PER_RESPONSE as u32);
    for _ in 0..LIMIT_DIRECTORIES_PER_RESPONSE {
      body = body.s("").u32(0);
    }
    let frame = body.u32(0).u32(1).s("").u32(0).zframe(5);
    assert_eq!(
      PeerMessage::decode(&frame),
      Err(DecodeError::LimitExceeded {
        what: "directories in one response",
        value: LIMIT_DIRECTORIES_PER_RESPONSE as u64 + 1,
        limit: LIMIT_DIRECTORIES_PER_RESPONSE as u64,
      })
    );
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
  fn inflate_rejects_truncated_and_corrupt_streams() {
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
  fn inflate_stops_a_zip_bomb_at_the_limit() {
    let compressed = compress(&vec![0; 64 << 20]);
    assert!(compressed.len() < 100 << 10);
    assert_eq!(
      inflate(&compressed, 1 << 20),
      Err(DecodeError::InflatedTooLarge { limit: 1 << 20 })
    );
  }
}
