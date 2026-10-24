//! Browsing a peer's shares and one of its folders (spec 01 sections 6.1, 6.6), and the `File` and
//! `Directory` layouts every compressed response shares (6.8).

use super::{read_compressed, write_compressed};
use crate::codec::{Wire, wire_struct};
use crate::error::{DecodeError, FieldError};
use crate::limits::LIMIT_ATTRIBUTES_PER_FILE;
use crate::log::KeyFields;
use crate::model::FileAttributeType;
use crate::wire::{Reader, RemoteFilename, Writer};

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

impl Wire for File {
  const MIN_BYTES: usize = 21;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    let code = r.u8("code")?;
    let filename = r.filename("filename")?;
    let size = fix_file_size(r.u64("size")?);
    let extension = r.string("extension")?;
    let count = r.count(FileAttribute::MIN_BYTES, "attributes")?;
    if count > LIMIT_ATTRIBUTES_PER_FILE {
      return Err(r.fail(
        "attributes",
        FieldError::LimitExceeded {
          value: count as u64,
          limit: LIMIT_ATTRIBUTES_PER_FILE as u64,
        },
      ));
    }
    let attributes = (0..count)
      .map(|_| FileAttribute::read(r, "attributes"))
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

/// Soulseek NS writes sizes over 2 GiB sign-extended from 32 bits; those are taken as the low
/// 32 bits
fn fix_file_size(size: u64) -> u64 {
  if size >> 32 == 0xffff_ffff {
    size & 0xffff_ffff
  } else {
    size
  }
}

/// a file list, counted against the response's file limit
pub(super) fn read_files(r: &mut Reader, field: &'static str) -> Result<Vec<File>, DecodeError> {
  let count = r.count(File::MIN_BYTES, field)?;
  r.take_files(count, field)?;
  (0..count).map(|_| File::read(r, field)).collect()
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

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      name: r.filename("name")?,
      files: read_files(r, "files")?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.filename(&self.name);
    self.files.write(w);
  }
}

/// a directory list, counted against the response's directory limit
fn read_directories(r: &mut Reader, field: &'static str) -> Result<Vec<Directory>, DecodeError> {
  let count = r.count(Directory::MIN_BYTES, field)?;
  r.take_directories(count, field)?;
  (0..count).map(|_| Directory::read(r, field)).collect()
}

/// Peer code 5, spec 01 section 6.1
///
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

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    read_compressed(r, |r| {
      let directories = read_directories(r, "directories")?;
      let mut locked_directories = Vec::new();
      // the unknown u32 is always 0 and discarded
      if r.optional::<u32>("unknown")?.is_some() && r.has_more() {
        locked_directories = read_directories(r, "locked_directories")?;
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
  /// Peer code 36, spec 01 section 6.6
  pub struct FolderContentsRequest {
    pub token: u32,
    /// the remote directory path exactly as it appeared in a browse or search
    pub directory_name: RemoteFilename,
  }
}

/// Peer code 37, spec 01 section 6.6
///
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

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    read_compressed(r, |r| {
      Ok(Self {
        token: r.u32("token")?,
        directory_name: r.filename("directory_name")?,
        directories: read_directories(r, "directories")?,
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

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::limits::LIMIT_DIRECTORIES_PER_RESPONSE;
  use crate::peer::PeerMessage;
  use crate::testing::{P, decodes, field, p, vectors};

  fn attr(attribute_type: FileAttributeType, value: u32) -> FileAttribute {
    FileAttribute {
      attribute_type,
      value,
    }
  }

  pub(crate) fn mp3(filename: &str) -> File {
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

  pub(crate) fn mp3_bytes(b: P, filename: &str) -> P {
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

  pub(crate) fn flac(filename: &str) -> File {
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

  pub(crate) fn flac_bytes(b: P, filename: &str) -> P {
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
    browse_response_rejects_a_stream_that_is_not_zlib: PeerMessage, p().raw(b"plain").frame(5)
      => Err(field("BrowseResponse", "body", FieldError::InvalidZlib));
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
  fn browse_response_directory_limit_counts_every_list() {
    let mut body = p().u32(LIMIT_DIRECTORIES_PER_RESPONSE as u32);
    for _ in 0..LIMIT_DIRECTORIES_PER_RESPONSE {
      body = body.s("").u32(0);
    }
    let frame = body.u32(0).u32(1).s("").u32(0).zframe(5);
    assert_eq!(
      PeerMessage::decode(&frame),
      Err(field(
        "BrowseResponse",
        "locked_directories",
        FieldError::LimitExceeded {
          value: LIMIT_DIRECTORIES_PER_RESPONSE as u64 + 1,
          limit: LIMIT_DIRECTORIES_PER_RESPONSE as u64,
        }
      ))
    );
  }
}
