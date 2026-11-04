//! A file the browser cannot play, answered as a WAV made as it is read and never stored: a
//! header followed by PCM. AIFF needs its samples swapped to little-endian, ALAC is decoded. The
//! length is known from the start, and a range maps to a place in the PCM, so seeking works.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;

use axum::body::{Body, Bytes};
use axum::response::Response;
use futures_util::{StreamExt, stream};
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSource;
use tracing::{debug, error, warn};

use super::ServeSpeed;
use super::aiff::{self, Aiff, Header};
use super::serve::{self, Entity, Ranged, Served};
use crate::analysis::Decoder;
use crate::api::error::ApiError;
use crate::services::ContentRequest;

/// The bytes of the WAV header.
const HEADER: u64 = 44;
/// The most a RIFF file can say it holds.
const MAX_RIFF: u64 = u32::MAX as u64;
/// How much PCM one piece of the response holds, at least.
const PIECE: usize = 256 * 1024;
/// The WAV format tags of integer and floating point samples.
const INTEGER: u16 = 1;
const FLOAT: u16 = 3;

/// The PCM a WAV holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Format {
  tag: u16,
  channels: u16,
  sample_rate: u32,
  /// the bytes one sample takes
  width: u16,
  frames: u64,
}

impl Format {
  /// The bytes of one sample frame.
  fn block(&self) -> u64 {
    u64::from(self.channels) * u64::from(self.width)
  }

  /// The WAV of this PCM, or why a WAV cannot hold it: its length and frame size have to fit
  /// the header's fields.
  fn fits(self) -> Result<Wav, String> {
    let size = self
      .frames
      .checked_mul(self.block())
      .and_then(|pcm| pcm.checked_add(HEADER))
      .filter(|size| *size <= MAX_RIFF && self.block() <= u64::from(u16::MAX))
      .ok_or("it is too long for a WAV")?;
    Ok(Wav { format: self, size })
  }
}

/// PCM a WAV can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wav {
  format: Format,
  /// the bytes of the whole WAV, header and PCM
  size: u64,
}

impl Wav {
  /// The WAV header.
  fn header(&self) -> [u8; HEADER as usize] {
    let format = &self.format;
    // both fit their fields, which `Format::fits` checked
    let pcm = (self.size - HEADER) as u32;
    let block = format.block() as u16;
    let mut header = [0; HEADER as usize];
    let mut at = 0;
    let mut put = |bytes: &[u8]| {
      header[at..at + bytes.len()].copy_from_slice(bytes);
      at += bytes.len();
    };
    put(b"RIFF");
    put(&(pcm + HEADER as u32 - 8).to_le_bytes());
    put(b"WAVEfmt ");
    put(&16u32.to_le_bytes());
    put(&format.tag.to_le_bytes());
    put(&format.channels.to_le_bytes());
    put(&format.sample_rate.to_le_bytes());
    put(
      &format
        .sample_rate
        .saturating_mul(u32::from(block))
        .to_le_bytes(),
    );
    put(&block.to_le_bytes());
    put(&(format.width * 8).to_le_bytes());
    put(b"data");
    put(&pcm.to_le_bytes());
    header
  }
}

/// Where the PCM comes from.
enum Source {
  /// samples read from an AIFF file and turned around
  Aiff(Aiff, Box<dyn MediaSource>),
  /// samples decoded from ALAC, each kept to its top `width` bytes
  Alac(Box<Decoder>),
}

/// A file found to be servable, ready to read.
struct Opened {
  wav: Wav,
  source: Source,
}

/// Why a file is not served.
enum Refused {
  /// nothing plain can be made of it
  Format(String),
  /// it could not be read
  Read(io::Error),
}

/// Reads what the file `file` holds, of `size` bytes and named with `extension` when it has one,
/// or why it cannot be served.
fn open(
  mut file: Box<dyn MediaSource>,
  size: u64,
  extension: Option<&str>,
) -> Result<Opened, Refused> {
  match aiff::read(&mut file, size).map_err(Refused::Read)? {
    Header::Pcm(aiff) => {
      let tag = if aiff.stored == aiff::Stored::Float {
        FLOAT
      } else {
        INTEGER
      };
      let format = Format {
        tag,
        channels: aiff.channels,
        sample_rate: aiff.sample_rate,
        width: aiff.width,
        frames: aiff.frames,
      };
      Ok(Opened {
        wav: format.fits().map_err(Refused::Format)?,
        source: Source::Aiff(aiff, file),
      })
    }
    Header::Refused(reason) => Err(Refused::Format(reason)),
    Header::Other => {
      file.seek(SeekFrom::Start(0)).map_err(Refused::Read)?;
      alac(file, extension)
    }
  }
}

/// `file` opened for decoding, when it is ALAC of a stated length.
fn alac(file: Box<dyn MediaSource>, extension: Option<&str>) -> Result<Opened, Refused> {
  let plain = || Refused::Format("only AIFF and ALAC are served as WAV".to_string());
  let mut hint = Hint::new();
  if let Some(extension) = extension {
    hint.with_extension(extension);
  }
  let decoder = Decoder::from_source(file, &hint).map_err(|reason| {
    debug!(reason, "a file was not opened for WAV");
    plain()
  })?;
  let claimed = &decoder.claimed;
  if claimed.codec != "alac" {
    return Err(plain());
  }
  let frames = claimed
    .frames
    .ok_or_else(|| Refused::Format("the file does not state its length".to_string()))?;
  let bits = claimed
    .bit_depth
    .ok_or_else(|| Refused::Format("the file does not state its bit depth".to_string()))?;
  let width = u16::try_from(bits.div_ceil(8))
    .ok()
    .filter(|width| (1..=4).contains(width))
    .ok_or_else(|| Refused::Format(format!("{bits}-bit samples cannot be served")))?;
  let channels = u16::try_from(claimed.channels)
    .map_err(|_| Refused::Format("the file states too many channels".to_string()))?;
  let format = Format {
    tag: INTEGER,
    channels,
    sample_rate: claimed.sample_rate,
    width,
    frames,
  };
  Ok(Opened {
    wav: format.fits().map_err(Refused::Format)?,
    source: Source::Alac(Box::new(decoder)),
  })
}

impl Source {
  /// Moves to sample frame `frame` of `format`, or for ALAC to the packet that holds it.
  fn seek(&mut self, format: &Format, frame: u64) -> Result<(), String> {
    match self {
      Self::Aiff(aiff, file) => file
        .seek(SeekFrom::Start(aiff.data_at + frame * format.block()))
        .map(|_| ())
        .map_err(|e| format!("seeking failed: {e}")),
      Self::Alac(decoder) => decoder.seek_frame(frame),
    }
  }

  /// Reads the next stretch of PCM into `out`, whole frames, and returns the frame it starts at
  /// when the source knows; `out` is empty at the end.
  fn read(&mut self, format: &Format, out: &mut Vec<u8>) -> Result<Option<u64>, String> {
    out.clear();
    match self {
      Self::Aiff(aiff, file) => {
        let block = format.block() as usize;
        out.resize((PIECE / block).max(1) * block, 0);
        let mut filled = 0;
        while filled < out.len() {
          match file.read(&mut out[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("reading stopped: {e}")),
          }
        }
        out.truncate(filled - filled % block);
        aiff::to_wav(aiff.stored, usize::from(aiff.width), out);
        Ok(None)
      }
      Self::Alac(decoder) => {
        let mut samples = Vec::new();
        let Some(start) = decoder.next_interleaved(&mut samples) else {
          return match decoder.stopped.take() {
            Some(reason) => Err(reason),
            None => Ok(None),
          };
        };
        let width = usize::from(format.width);
        out.reserve(samples.len() * width);
        for sample in samples {
          // the sample's bits are at the top of its 32
          out.extend_from_slice(&sample.to_le_bytes()[4 - width..]);
        }
        Ok(Some(start))
      }
    }
  }
}

/// The part of the header and the part of the PCM a range of the WAV covers.
#[derive(Debug, PartialEq, Eq)]
struct Parts {
  /// the header's bytes that go first
  header: Range<usize>,
  /// where in the PCM the rest starts
  pcm_from: u64,
  /// how much of the PCM goes
  pcm_length: u64,
}

/// Maps `length` bytes of the WAV from `start` to the header and the PCM.
fn parts(start: u64, length: u64) -> Parts {
  let header_from = start.min(HEADER);
  let in_header = (HEADER - header_from).min(length);
  Parts {
    header: header_from as usize..(header_from + in_header) as usize,
    pcm_from: start.saturating_sub(HEADER),
    pcm_length: length - in_header,
  }
}

/// The PCM of a WAV from one byte to another, read a stretch of the source at a time.
struct Pcm {
  source: Source,
  format: Format,
  /// the next byte of the PCM to give
  at: u64,
  /// the byte of the PCM where giving stops
  end: u64,
  /// whether the source was moved to where the PCM starts
  sought: bool,
  /// where the next stretch starts in the PCM, when the source does not say
  next: u64,
  /// a stretch read and not given yet, with where it starts in the PCM
  held: Option<(u64, Bytes)>,
}

impl Pcm {
  /// `length` bytes of the PCM of `opened` from byte `start`.
  fn new(opened: Opened, start: u64, length: u64) -> Self {
    Self {
      source: opened.source,
      format: opened.wav.format,
      at: start,
      end: start + length,
      sought: false,
      next: 0,
      held: None,
    }
  }

  /// The next piece, `None` once every byte was given or after a piece that failed. Reads the
  /// source at most once, which blocks.
  fn step(&mut self) -> Option<Result<Bytes, String>> {
    while self.at < self.end {
      if let Some((from, stretch)) = self.held.take() {
        // a sought packet can start before the range, and a range can start inside a sample
        let skip = self.at.saturating_sub(from);
        let length = stretch.len() as u64;
        if skip < length {
          let given = (length - skip).min(self.end - self.at);
          self.at += given;
          return Some(Ok(stretch.slice(skip as usize..(skip + given) as usize)));
        }
        continue;
      }
      if let Err(reason) = self.read() {
        self.at = self.end;
        return Some(Err(reason));
      }
    }
    None
  }

  /// Reads the next stretch of the source into `held`.
  fn read(&mut self) -> Result<(), String> {
    let block = self.format.block();
    if !self.sought {
      let first = self.at / block;
      self.source.seek(&self.format, first)?;
      self.next = first * block;
      self.sought = true;
    }
    let mut pcm = Vec::new();
    let frame = self.source.read(&self.format, &mut pcm)?;
    if pcm.is_empty() {
      return Err("the audio ends before its stated length".to_string());
    }
    let from = frame.map_or(self.next, |frame| frame * block);
    self.next = from + pcm.len() as u64;
    self.held = Some((from, Bytes::from(pcm)));
    Ok(())
  }
}

/// The WAV's `length` bytes from `start`, read as they are sent and no faster than `speed`
/// allows. The file is named `shown` in the log.
fn body(opened: Opened, shown: String, start: u64, length: u64, speed: ServeSpeed) -> Body {
  let Parts {
    header,
    pcm_from,
    pcm_length,
  } = parts(start, length);
  let head = Bytes::copy_from_slice(&opened.wav.header()[header]);
  let pcm = (pcm_length > 0).then(|| Pcm::new(opened, pcm_from, pcm_length));
  // each piece is read on the blocking pool, so a body nobody reads any more holds no thread
  let pieces = stream::unfold((pcm, speed, shown), |(pcm, speed, shown)| async move {
    let mut pcm = pcm?;
    let stepped = tokio::task::spawn_blocking(move || {
      let piece = pcm.step();
      (pcm, piece)
    })
    .await;
    let (piece, pcm) = match stepped {
      Ok((_, None)) => return None,
      Ok((pcm, Some(Ok(bytes)))) => {
        let mut left = bytes.len();
        while left > 0 {
          left -= speed.allowed(left).await;
        }
        (Ok(bytes), Some(pcm))
      }
      Ok((_, Some(Err(reason)))) => {
        warn!(
          path = shown,
          reason, "a file served as WAV could not be read"
        );
        (Err(io::Error::other(reason)), None)
      }
      Err(e) => {
        error!(path = shown, error = %e, "reading a file served as WAV failed");
        (Err(io::Error::other(e)), None)
      }
    };
    Some((piece, (pcm, speed, shown)))
  });
  Body::from_stream(stream::iter((!head.is_empty()).then_some(Ok(head))).chain(pieces))
}

/// A file's `bytes` opened as they are served as WAV, or why they are not.
fn opened_bytes(bytes: &[u8]) -> Result<Opened, String> {
  let file = Box::new(Cursor::new(bytes.to_vec()));
  match open(file, bytes.len() as u64, None) {
    Ok(opened) => Ok(opened),
    Err(Refused::Format(reason)) => Err(reason),
    Err(Refused::Read(e)) => Err(e.to_string()),
  }
}

/// The length of the WAV a file of `bytes` is served as, or why it is not served as one.
pub fn wav_length(bytes: &[u8]) -> Result<u64, String> {
  Ok(opened_bytes(bytes)?.wav.size)
}

/// `length` bytes from `start` of the WAV a file of `bytes` is served as, read as a response
/// reads them, or why they could not be.
pub fn wav_bytes(bytes: &[u8], start: u64, length: u64) -> Result<Vec<u8>, String> {
  let opened = opened_bytes(bytes)?;
  let size = opened.wav.size;
  if start.checked_add(length).is_none_or(|end| end > size) {
    return Err(format!("the WAV has only {size} bytes"));
  }
  let Parts {
    header,
    pcm_from,
    pcm_length,
  } = parts(start, length);
  let mut out = opened.wav.header()[header].to_vec();
  let mut pcm = Pcm::new(opened, pcm_from, pcm_length);
  while let Some(piece) = pcm.step() {
    out.extend_from_slice(&piece?);
  }
  Ok(out)
}

/// `file` as a WAV, as the response `request` asks for, sent no faster than `speed` allows. What
/// cannot be served plainly is refused with the reason.
pub(super) async fn respond(
  file: Served,
  request: &ContentRequest,
  speed: &ServeSpeed,
) -> Result<Response, ApiError> {
  let path = file.path.clone();
  let size = file.size;
  let opened = tokio::task::spawn_blocking(move || {
    let opened = File::open(&path).map_err(Refused::Read)?;
    let extension = path.extension().and_then(|e| e.to_str());
    open(Box::new(opened), size, extension)
  })
  .await
  .map_err(ApiError::internal)?;
  let opened = match opened {
    Ok(opened) => opened,
    Err(Refused::Format(reason)) => {
      debug!(path = %file.path.display(), reason, "file not served as WAV");
      return Err(ApiError::unsupported_media(format!(
        "This file cannot be played: {reason}"
      )));
    }
    Err(Refused::Read(e)) => {
      return Err(ApiError::internal(format!(
        "{} not read: {e}",
        file.path.display()
      )));
    }
  };
  let entity = Entity {
    name: Path::new(&file.name())
      .with_extension("wav")
      .to_string_lossy()
      .into_owned(),
    etag: file.etag_of("wav"),
    kind: "audio/wav".to_string(),
    size: opened.wav.size,
  };
  match serve::ranged(&entity, request) {
    Ranged::Answered(response) => Ok(response),
    Ranged::Bytes(part) => {
      let (start, length) = (part.start, part.length);
      let shown = file.path.display().to_string();
      Ok(part.with(body(opened, shown, start, length, speed.clone())))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_range_maps_to_the_header_and_the_pcm() {
    assert_eq!(
      parts(0, 144),
      Parts {
        header: 0..44,
        pcm_from: 0,
        pcm_length: 100
      }
    );
    assert_eq!(
      parts(10, 4),
      Parts {
        header: 10..14,
        pcm_from: 0,
        pcm_length: 0
      },
      "only the header"
    );
    assert_eq!(
      parts(40, 10),
      Parts {
        header: 40..44,
        pcm_from: 0,
        pcm_length: 6
      },
      "across the two"
    );
    assert_eq!(
      parts(50, 10),
      Parts {
        header: 44..44,
        pcm_from: 6,
        pcm_length: 10
      },
      "only the PCM"
    );
  }

  #[test]
  fn the_header_says_what_the_pcm_is() {
    let format = Format {
      tag: INTEGER,
      channels: 2,
      sample_rate: 44100,
      width: 3,
      frames: 10,
    };
    let header = format.fits().unwrap().header();
    let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let u16_at = |at: usize| u16::from_le_bytes(header[at..at + 2].try_into().unwrap());
    assert_eq!(&header[..4], b"RIFF");
    assert_eq!(u32_at(4), 36 + 60);
    assert_eq!(&header[8..16], b"WAVEfmt ");
    assert_eq!(u32_at(16), 16);
    assert_eq!(u16_at(20), 1);
    assert_eq!(u16_at(22), 2);
    assert_eq!(u32_at(24), 44100);
    assert_eq!(u32_at(28), 44100 * 6);
    assert_eq!(u16_at(32), 6);
    assert_eq!(u16_at(34), 24);
    assert_eq!(&header[36..40], b"data");
    assert_eq!(u32_at(40), 60);
  }

  /// A 16-bit stereo ALAC tone at 8000 Hz in an MP4 container: four packets of 4096, 4096, 4096
  /// and 3712 frames, one after the other in its media data.
  const TONE: &[u8] = include_bytes!("fixtures/tone.m4a");

  /// Where the box `kind` starts; each kind the tests change is in the tone once.
  fn box_at(bytes: &[u8], kind: &[u8; 4]) -> usize {
    bytes.windows(4).position(|w| w == kind).unwrap() - 4
  }

  fn get(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
  }

  fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
  }

  /// A box of `kind` holding the big-endian `words` and then `inner`.
  fn boxed(kind: &[u8; 4], words: &[u32], inner: &[u8]) -> Vec<u8> {
    let length = 8 + 4 * words.len() + inner.len();
    let mut out = u32::try_from(length).unwrap().to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    for word in words {
      out.extend_from_slice(&word.to_be_bytes());
    }
    out.extend_from_slice(inner);
    out
  }

  /// The tone with `entries` as its time-to-sample table, as sample counts and the frames each
  /// sample lasts.
  fn timed(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = TONE.to_vec();
    let mut words = vec![0, u32::try_from(entries.len()).unwrap()];
    words.extend(entries.iter().flat_map(|&(count, delta)| [count, delta]));
    let table = boxed(b"stts", &words, &[]);
    let at = box_at(&bytes, b"stts");
    let old = get(&bytes, at) as usize;
    bytes.splice(at..at + old, table.iter().copied());
    // the boxes holding the table come before it, and the media data before them all
    for kind in [b"moov", b"trak", b"mdia", b"minf", b"stbl"] {
      let holder = box_at(&bytes, kind);
      let grown = get(&bytes, holder) as usize + table.len() - old;
      put(&mut bytes, holder, u32::try_from(grown).unwrap());
    }
    bytes
  }

  #[test]
  fn a_length_past_what_a_wav_holds_is_refused_however_far() {
    // the first entry sums to about 2^62 frames
    let bytes = timed(&[(0x4000_0001, 0xffff_ffff), (1, 3712)]);
    let refused = wav_length(&bytes).unwrap_err();
    assert!(refused.contains("too long"), "{refused}");
  }
}
