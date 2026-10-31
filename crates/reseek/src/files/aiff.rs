//! AIFF and AIFF-C as plain PCM: where the samples are in the file, what they are, and turning
//! them into the little-endian samples a WAV holds. Nothing is decoded; what is compressed is
//! refused.

use std::io::{self, Read, Seek, SeekFrom};

/// The most chunks read looking for the format and the samples, so a file of tiny chunks cannot
/// keep a request reading headers.
const MAX_CHUNKS: u32 = 4096;

/// How the samples of a file are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stored {
  /// signed integers, most significant byte first
  Signed,
  /// signed integers, least significant byte first
  SignedLittle,
  /// unsigned integers, one byte each
  Unsigned,
  /// IEEE floats, most significant byte first
  Float,
}

/// Where a file's samples are and what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Aiff {
  pub stored: Stored,
  pub channels: u16,
  pub sample_rate: u32,
  /// the bytes one sample takes, what the bits per sample round up to
  pub width: u16,
  /// where the first sample is in the file
  pub data_at: u64,
  /// the sample frames the file really holds
  pub frames: u64,
}

/// What reading a file's header found out.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Header {
  /// not an AIFF file at all
  Other,
  /// plain samples
  Pcm(Aiff),
  /// an AIFF file that cannot be served, with the reason
  Refused(String),
}

/// Reads the header of the file `source`, of `size` bytes.
pub(super) fn read(source: &mut (impl Read + Seek), size: u64) -> io::Result<Header> {
  let mut form = [0; 12];
  source.seek(SeekFrom::Start(0))?;
  match source.read_exact(&mut form) {
    Ok(()) => {}
    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(Header::Other),
    Err(e) => return Err(e),
  }
  let compressed = match (&form[..4], &form[8..12]) {
    (b"FORM", b"AIFF") => false,
    (b"FORM", b"AIFC") => true,
    _ => return Ok(Header::Other),
  };
  let mut format = None;
  let mut samples = None;
  let mut at = 12u64;
  for _ in 0..MAX_CHUNKS {
    if format.is_some() && samples.is_some() {
      break;
    }
    let mut head = [0; 8];
    source.seek(SeekFrom::Start(at))?;
    if read_all(source, &mut head)? < head.len() {
      break;
    }
    let length = u64::from(u32::from_be_bytes([head[4], head[5], head[6], head[7]]));
    let body = at + 8;
    match &head[..4] {
      b"COMM" => {
        // the longest common chunk that matters: the fields and a compression type
        let mut fields = [0; 22];
        let read = read_all(source, &mut fields)?.min(length.min(22) as usize);
        match common(&fields[..read], compressed) {
          Ok(found) => format = Some(found),
          Err(reason) => return Ok(Header::Refused(reason)),
        }
      }
      b"SSND" => {
        let mut offset = [0; 4];
        if read_all(source, &mut offset)? < offset.len() {
          break;
        }
        let skipped = u64::from(u32::from_be_bytes(offset));
        // the offset and the block size come before the samples
        let start = body + 8 + skipped;
        let end = (body + length).min(size);
        samples = Some((start, end.saturating_sub(start)));
      }
      _ => {}
    }
    // a chunk of odd length is followed by a pad byte
    at = body + length + (length & 1);
  }
  let Some((stored, channels, stated_frames, sample_rate, width)) = format else {
    return Ok(Header::Refused("the file states no audio format".into()));
  };
  let Some((data_at, bytes)) = samples else {
    return Ok(Header::Refused("the file holds no samples".into()));
  };
  let block = u64::from(channels) * u64::from(width);
  Ok(Header::Pcm(Aiff {
    stored,
    channels,
    sample_rate,
    width,
    data_at,
    frames: stated_frames.min(bytes / block),
  }))
}

/// What a read gave, which is less than `into` holds only at the end of the file.
fn read_all(source: &mut impl Read, into: &mut [u8]) -> io::Result<usize> {
  let mut filled = 0;
  while filled < into.len() {
    match source.read(&mut into[filled..])? {
      0 => break,
      read => filled += read,
    }
  }
  Ok(filled)
}

/// How the samples are stored, the channels, the frames stated, the rate and the sample width.
type Common = (Stored, u16, u64, u32, u16);

/// The format in the start of a common chunk, or why it cannot be served.
fn common(fields: &[u8], compressed: bool) -> Result<Common, String> {
  let too_short = || "the audio format is cut short".to_string();
  if fields.len() < 18 {
    return Err(too_short());
  }
  let channels = u16::from_be_bytes([fields[0], fields[1]]);
  let frames = u32::from_be_bytes([fields[2], fields[3], fields[4], fields[5]]);
  let bits = u16::from_be_bytes([fields[6], fields[7]]);
  let rate: &[u8; 10] = fields[8..18].try_into().map_err(|_| too_short())?;
  let sample_rate = extended(rate).ok_or("the sample rate is not usable")?;
  if channels == 0 {
    return Err("the file states no channels".to_string());
  }
  let compression = if compressed {
    *fields
      .get(18..22)
      .and_then(|c| <&[u8; 4]>::try_from(c).ok())
      .ok_or_else(too_short)?
  } else {
    *b"NONE"
  };
  let stored = stored_as(&compression, bits)?;
  // a sample of 12 bits fills two bytes from the top, as it does in a WAV
  let width = bits.div_ceil(8);
  Ok((stored, channels, u64::from(frames), sample_rate, width))
}

/// What the compression type `kind` means for samples of `bits`, or why it cannot be served.
fn stored_as(kind: &[u8; 4], bits: u16) -> Result<Stored, String> {
  let unusable = || format!("{bits}-bit samples cannot be served");
  match kind {
    b"NONE" | b"twos" if (1..=32).contains(&bits) => Ok(Stored::Signed),
    b"sowt" if (1..=32).contains(&bits) => Ok(Stored::SignedLittle),
    b"in24" if bits == 24 => Ok(Stored::Signed),
    b"in32" if bits == 32 => Ok(Stored::Signed),
    b"raw " if bits == 8 => Ok(Stored::Unsigned),
    b"fl32" | b"FL32" if bits == 32 => Ok(Stored::Float),
    b"fl64" | b"FL64" if bits == 64 => Ok(Stored::Float),
    b"NONE" | b"twos" | b"sowt" | b"in24" | b"in32" | b"raw " | b"fl32" | b"FL32" | b"fl64"
    | b"FL64" => Err(unusable()),
    other => Err(format!(
      "the audio is compressed as {}",
      String::from_utf8_lossy(other).trim()
    )),
  }
}

/// An 80-bit extended float as a whole number of samples per second; `None` for what is not a
/// usable rate.
fn extended(bytes: &[u8; 10]) -> Option<u32> {
  let sign_and_exponent = u16::from_be_bytes([bytes[0], bytes[1]]);
  let mantissa = u64::from_be_bytes(bytes[2..10].try_into().ok()?);
  let exponent = i32::from(sign_and_exponent & 0x7fff) - 16383 - 63;
  if sign_and_exponent & 0x8000 != 0 || mantissa == 0 {
    return None;
  }
  let rate = (mantissa as f64 * 2f64.powi(exponent)).round();
  (rate >= 1.0 && rate <= f64::from(u32::MAX)).then_some(rate as u32)
}

/// Turns whole samples of `width` bytes, stored as `stored`, into what a WAV holds in place:
/// little-endian, and unsigned at 8 bits.
pub(super) fn to_wav(stored: Stored, width: usize, samples: &mut [u8]) {
  match (stored, width) {
    (Stored::Unsigned, _) => {}
    (Stored::Signed | Stored::SignedLittle, 1) => {
      for sample in samples {
        *sample ^= 0x80;
      }
    }
    (Stored::SignedLittle, _) => {}
    (Stored::Signed | Stored::Float, _) => {
      for sample in samples.chunks_exact_mut(width) {
        sample.reverse();
      }
    }
  }
}

#[cfg(test)]
pub(super) mod tests {
  use std::io::Cursor;

  use super::*;

  /// 44100 as an 80-bit extended float.
  const RATE: [u8; 10] = [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0];

  /// An AIFF file of big-endian `samples`.
  pub fn aiff_file(channels: u16, bits: u16, samples: &[u8]) -> Vec<u8> {
    file(None, channels, bits, samples)
  }

  /// A mono 8-bit AIFF-C file of `samples` compressed as `kind`.
  pub fn aiff_c_file(kind: &[u8; 4], samples: &[u8]) -> Vec<u8> {
    file(Some(kind), 1, 8, samples)
  }

  /// An AIFF file of `samples`, compressed as `kind` for AIFF-C, with a chunk before them.
  fn file(kind: Option<&[u8; 4]>, channels: u16, bits: u16, samples: &[u8]) -> Vec<u8> {
    let width = usize::from(bits.div_ceil(8));
    let frames = samples.len() / (usize::from(channels) * width);
    let mut common = Vec::new();
    common.extend_from_slice(&channels.to_be_bytes());
    common.extend_from_slice(&u32::try_from(frames).unwrap().to_be_bytes());
    common.extend_from_slice(&bits.to_be_bytes());
    common.extend_from_slice(&RATE);
    if let Some(kind) = kind {
      common.extend_from_slice(kind);
      common.extend_from_slice(b"\x04name\x00");
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM\0\0\0\0");
    out.extend_from_slice(if kind.is_some() { b"AIFC" } else { b"AIFF" });
    // an odd chunk, whose pad byte the walk must step over
    out.extend_from_slice(b"NAME\0\0\0\x03abc\0");
    out.extend_from_slice(b"COMM");
    out.extend_from_slice(&u32::try_from(common.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&common);
    if common.len() % 2 == 1 {
      out.push(0);
    }
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&u32::try_from(samples.len() + 12).unwrap().to_be_bytes());
    // an offset of four bytes before the samples
    out.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 0, 0xde, 0xad, 0xbe, 0xef]);
    out.extend_from_slice(samples);
    out
  }

  fn header(bytes: &[u8]) -> Header {
    read(&mut Cursor::new(bytes), bytes.len() as u64).unwrap()
  }

  fn pcm(bytes: &[u8]) -> Aiff {
    match header(bytes) {
      Header::Pcm(aiff) => aiff,
      other => panic!("{other:?}"),
    }
  }

  #[test]
  fn the_samples_are_found_past_the_chunks_and_the_offset_before_them() {
    let bytes = file(None, 2, 16, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let aiff = pcm(&bytes);
    assert_eq!(aiff.sample_rate, 44100);
    assert_eq!((aiff.channels, aiff.width, aiff.frames), (2, 2, 2));
    assert_eq!(aiff.stored, Stored::Signed);
    let at = usize::try_from(aiff.data_at).unwrap();
    assert_eq!(&bytes[at..], [1, 2, 3, 4, 5, 6, 7, 8]);
  }

  #[test]
  fn a_file_cut_short_holds_only_the_frames_still_there() {
    let mut bytes = file(None, 1, 16, &[0; 100]);
    bytes.truncate(bytes.len() - 51);
    assert_eq!(pcm(&bytes).frames, 24);
  }

  #[test]
  fn aiff_c_says_how_its_samples_are_stored_and_what_is_compressed_is_refused() {
    let stored = |kind: &[u8; 4], bits| pcm(&file(Some(kind), 1, bits, &[0; 16])).stored;
    assert_eq!(stored(b"NONE", 16), Stored::Signed);
    assert_eq!(stored(b"twos", 16), Stored::Signed);
    assert_eq!(stored(b"sowt", 16), Stored::SignedLittle);
    assert_eq!(stored(b"in24", 24), Stored::Signed);
    assert_eq!(stored(b"fl32", 32), Stored::Float);
    assert_eq!(stored(b"raw ", 8), Stored::Unsigned);
    assert_eq!(
      header(&file(Some(b"ulaw"), 1, 8, &[0; 16])),
      Header::Refused("the audio is compressed as ulaw".into())
    );
    assert!(matches!(
      header(&file(Some(b"in24"), 1, 16, &[0; 16])),
      Header::Refused(_)
    ));
    assert_eq!(header(b"RIFF....WAVEfmt "), Header::Other);
    assert_eq!(header(b"FORM"), Header::Other);
  }

  #[test]
  fn samples_become_little_endian_and_unsigned_at_eight_bits() {
    let swapped = |stored, width: usize, bytes: &[u8]| {
      let mut bytes = bytes.to_vec();
      to_wav(stored, width, &mut bytes);
      bytes
    };
    assert_eq!(
      swapped(Stored::Signed, 2, &[0x12, 0x34, 0xab, 0xcd]),
      [0x34, 0x12, 0xcd, 0xab]
    );
    assert_eq!(
      swapped(Stored::Signed, 3, &[1, 2, 3, 4, 5, 6]),
      [3, 2, 1, 6, 5, 4]
    );
    assert_eq!(swapped(Stored::Signed, 4, &[1, 2, 3, 4]), [4, 3, 2, 1]);
    assert_eq!(
      swapped(Stored::Float, 8, &[1, 2, 3, 4, 5, 6, 7, 8]),
      [8, 7, 6, 5, 4, 3, 2, 1]
    );
    assert_eq!(swapped(Stored::SignedLittle, 2, &[1, 2]), [1, 2]);
    assert_eq!(
      swapped(Stored::Signed, 1, &[0x00, 0xff, 0x7f]),
      [0x80, 0x7f, 0xff]
    );
    assert_eq!(swapped(Stored::Unsigned, 1, &[0x80]), [0x80]);
  }
}
