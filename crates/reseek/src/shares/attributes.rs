//! What a shared file says about itself to the network: its extension and, for audio and
//! video, its bit rate, length and sample format, read from its headers only.
//!
//! Lossy files send bit rate, length and whether the bit rate varies; lossless files send
//! length, sample rate and bit depth, in that order, which is the order other clients send
//! them in. A file whose headers do not read sends no attributes.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use api_types::search::{FileAttribute, FileAttributeType};
use lofty::config::ParseOptions;
use lofty::file::FileType;
use lofty::prelude::*;
use lofty::probe::Probe;
use tracing::debug;

/// The extensions whose headers are read.
const PROBED: &[&str] = &[
  "aa", "aax", "aac", "aiff", "ape", "dsf", "flac", "m4a", "m4b", "m4p", "mp3", "mpc", "mpp",
  "ogg", "oga", "wav", "wma", "wv", "webm", "mkv", "ogv", "avi", "wmv", "asf", "mp4", "m4v", "mpg",
  "mpe", "mpv", "m2v",
];

/// The text after the last `.` of a file name, lower case, empty without one.
pub fn extension(name: &str) -> String {
  name
    .rsplit_once('.')
    .map(|(_, extension)| extension.to_lowercase())
    .unwrap_or_default()
}

/// The attributes of the file at `path` with the extension `extension`.
pub fn read(path: &Path, extension: &str) -> Vec<FileAttribute> {
  if !PROBED.contains(&extension) {
    return Vec::new();
  }
  match probe(path) {
    Ok(attributes) => attributes,
    Err(e) => {
      debug!(
        file = %path.display(),
        error = %e,
        "Failed to read metadata from file; the file is an unsupported format or may be corrupt"
      );
      Vec::new()
    }
  }
}

fn probe(path: &Path) -> Result<Vec<FileAttribute>, lofty::error::FileParseError> {
  let options = ParseOptions::new().read_tags(false).read_cover_art(false);
  let file = Probe::open(path)?.options(options).read()?;
  let properties = file.properties();
  let length = u32::try_from(properties.duration().as_secs()).unwrap_or(u32::MAX);
  let attribute = |attribute_type, value| FileAttribute {
    attribute_type,
    value,
  };
  let bit_depth = u32::from(properties.bit_depth().unwrap_or(0));
  if bit_depth > 0 {
    return Ok(vec![
      attribute(FileAttributeType::Length, length),
      attribute(
        FileAttributeType::SampleRate,
        properties.sample_rate().unwrap_or(0),
      ),
      attribute(FileAttributeType::BitDepth, bit_depth),
    ]);
  }
  let variable = match file.file_type() {
    FileType::Mpeg => mpeg_is_variable(path),
    // AAC is always reported as varying, and so is everything in an Ogg container
    FileType::Aac | FileType::Mp4 | FileType::Vorbis | FileType::Opus | FileType::Speex => true,
    _ => false,
  };
  Ok(vec![
    attribute(
      FileAttributeType::BitRate,
      properties.audio_bitrate().unwrap_or(0),
    ),
    attribute(FileAttributeType::Length, length),
    attribute(FileAttributeType::VariableBitRate, u32::from(variable)),
  ])
}

/// How far into an MPEG file the first frame is looked for, after any ID3v2 tag.
const FRAME_SEARCH: usize = 64 * 1024;

/// Whether an MPEG audio file's first frame carries a Xing or VBRI header with a size, which
/// marks a stream whose bit rate varies. An `Info` header marks a constant one.
fn mpeg_is_variable(path: &Path) -> bool {
  let Ok(mut file) = File::open(path) else {
    return false;
  };
  let mut header = [0u8; 10];
  let skip = match file.read_exact(&mut header) {
    Ok(()) if &header[..3] == b"ID3" => {
      let size = header[6..10]
        .iter()
        .fold(0u64, |size, b| (size << 7) | u64::from(b & 0x7f));
      let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };
      10 + size + footer
    }
    Ok(()) => 0,
    Err(_) => return false,
  };
  if file.seek(SeekFrom::Start(skip)).is_err() {
    return false;
  }
  let mut window = Vec::with_capacity(FRAME_SEARCH);
  if file
    .take(FRAME_SEARCH as u64)
    .read_to_end(&mut window)
    .is_err()
  {
    return false;
  }
  frame_is_variable(&window)
}

/// [`mpeg_is_variable`] on bytes starting after any ID3v2 tag.
fn frame_is_variable(bytes: &[u8]) -> bool {
  let Some(start) = (0..bytes.len().saturating_sub(4)).find(|&i| {
    let (a, b) = (bytes[i], bytes[i + 1]);
    // eleven set bits of frame sync, a known version and layer III
    a == 0xff && b & 0xe0 == 0xe0 && (b >> 3) & 0b11 != 0b01 && (b >> 1) & 0b11 == 0b01
  }) else {
    return false;
  };
  let frame = &bytes[start..];
  let mpeg1 = (frame[1] >> 3) & 0b11 == 0b11;
  let mono = frame[3] >> 6 == 0b11;
  let side_info = match (mpeg1, mono) {
    (true, true) => 17,
    (true, false) => 32,
    (false, true) => 9,
    (false, false) => 17,
  };
  let read_u32 = |at: usize| {
    frame
      .get(at..at + 4)
      .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
  };
  let xing = 4 + side_info;
  match frame.get(xing..xing + 4) {
    Some(b"Xing") => {
      let flags = read_u32(xing + 4).unwrap_or(0);
      let mut at = xing + 8;
      let frames = (flags & 1 != 0).then(|| {
        let value = read_u32(at).unwrap_or(0);
        at += 4;
        value
      });
      let size = (flags & 2 != 0).then(|| read_u32(at).unwrap_or(0));
      size.or(frames).is_some_and(|value| value > 0)
    }
    Some(b"Info") => false,
    _ => frame.get(36..40) == Some(b"VBRI") && read_u32(36 + 10).is_some_and(|size| size > 0),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// An MPEG-1 layer III stereo frame header followed by `tag` at the side-information offset.
  fn frame(tag: &[u8], flags: u32, value: u32) -> Vec<u8> {
    let mut bytes = vec![0u8, 0xff, 0xfb, 0x90, 0x00];
    bytes.extend(vec![0u8; 32]);
    bytes.extend(tag);
    bytes.extend(flags.to_be_bytes());
    bytes.extend(value.to_be_bytes());
    bytes.extend(vec![0u8; 64]);
    bytes
  }

  #[test]
  fn a_xing_header_with_a_size_marks_a_varying_bit_rate() {
    assert!(frame_is_variable(&frame(b"Xing", 1, 1000)));
    assert!(!frame_is_variable(&frame(b"Xing", 0, 0)));
    assert!(
      !frame_is_variable(&frame(b"Info", 1, 1000)),
      "Info is constant"
    );
    assert!(!frame_is_variable(&frame(b"none", 1, 1000)));
    let mut vbri = vec![0xff, 0xfb, 0x90, 0x00];
    vbri.extend(vec![0u8; 32]);
    vbri.extend(b"VBRI");
    vbri.extend([0, 1, 0, 0, 0, 50]);
    vbri.extend(5000u32.to_be_bytes());
    assert!(frame_is_variable(&vbri));
  }

  #[test]
  fn a_file_that_does_not_read_has_no_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.mp3");
    std::fs::write(&path, b"not audio at all").unwrap();
    assert!(read(&path, "mp3").is_empty());
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, b"x").unwrap();
    assert!(read(&text, "txt").is_empty());
  }

  #[test]
  fn a_wav_reads_as_lossless_in_its_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    std::fs::write(&path, wav(44_100, 16, 44_100 * 3)).unwrap();
    let read = read(&path, "wav");
    let kinds: Vec<_> = read.iter().map(|a| (a.attribute_type, a.value)).collect();
    assert_eq!(
      kinds,
      [
        (FileAttributeType::Length, 3),
        (FileAttributeType::SampleRate, 44_100),
        (FileAttributeType::BitDepth, 16)
      ]
    );
  }

  /// A mono PCM WAV file of `frames` silent samples.
  pub(crate) fn wav(rate: u32, bits: u16, frames: u32) -> Vec<u8> {
    let data = frames * u32::from(bits / 8);
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(bits / 8)).to_le_bytes());
    bytes.extend((bits / 8).to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(vec![0u8; data as usize]);
    bytes
  }
}
