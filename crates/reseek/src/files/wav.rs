//! A file the browser cannot play, answered as a WAV made as it is read and never stored: a
//! header followed by PCM. AIFF needs its samples swapped to little-endian, ALAC is decoded. The
//! length is known from the start, and a range maps to a place in the PCM, so seeking works.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::response::Response;
use futures_util::{StreamExt, stream};
use tokio::sync::mpsc;
use tracing::{debug, warn};

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
/// How many pieces wait to be sent before the reader waits.
const BACKLOG: usize = 4;
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

  /// The bytes of PCM.
  fn pcm(&self) -> u64 {
    self.frames * self.block()
  }

  /// Whether a WAV can hold this PCM: its length and frame size have to fit the header's fields.
  fn fits(&self) -> bool {
    HEADER + self.pcm() <= MAX_RIFF && self.block() <= u64::from(u16::MAX)
  }

  /// The WAV header of this PCM, which [`Self::fits`] one.
  fn header(&self) -> [u8; HEADER as usize] {
    let pcm = self.pcm() as u32;
    let block = self.block() as u16;
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
    put(&self.tag.to_le_bytes());
    put(&self.channels.to_le_bytes());
    put(&self.sample_rate.to_le_bytes());
    put(
      &self
        .sample_rate
        .saturating_mul(u32::from(block))
        .to_le_bytes(),
    );
    put(&block.to_le_bytes());
    put(&(self.width * 8).to_le_bytes());
    put(b"data");
    put(&pcm.to_le_bytes());
    header
  }
}

/// Where the PCM comes from.
enum Source {
  /// samples read from an AIFF file and turned around
  Aiff(Aiff, File),
  /// samples decoded from ALAC, each kept to its top `width` bytes
  Alac(Box<Decoder>),
}

/// A file found to be servable, ready to read.
struct Opened {
  format: Format,
  source: Source,
}

/// Why a file is not served.
enum Refused {
  /// nothing plain can be made of it
  Format(String),
  /// it could not be read
  Read(io::Error),
}

/// Reads what `path` is, or why it cannot be served.
fn open(path: &Path, size: u64) -> Result<Opened, Refused> {
  let mut file = File::open(path).map_err(Refused::Read)?;
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
        format,
        source: Source::Aiff(aiff, file),
      })
    }
    Header::Refused(reason) => Err(Refused::Format(reason)),
    Header::Other => alac(path),
  }
}

/// `path` opened for decoding, when it is ALAC of a stated length.
fn alac(path: &Path) -> Result<Opened, Refused> {
  let plain = || Refused::Format("only AIFF and ALAC are served as WAV".to_string());
  let decoder = Decoder::open(path).map_err(|reason| {
    debug!(path = %path.display(), reason, "a file was not opened for WAV");
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
    format,
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

/// `length` bytes of PCM from byte `start`, into `pieces`, until they are read, the file fails or
/// nobody is reading any more. Runs on a thread of its own, since decoding takes a while.
fn read_pcm(
  mut source: Source,
  format: Format,
  start: u64,
  length: u64,
  shown: &str,
  pieces: &mpsc::Sender<io::Result<Bytes>>,
) {
  let block = format.block();
  let first = start / block;
  let failed = |reason: String| {
    warn!(
      path = shown,
      reason, "a file served as WAV could not be read"
    );
    if pieces.blocking_send(Err(io::Error::other(reason))).is_err() {
      debug!(path = shown, "nobody is reading the WAV any more");
    }
  };
  if let Err(reason) = source.seek(&format, first) {
    return failed(reason);
  }
  let mut left = length;
  // where the next stretch starts in the PCM, when the source does not say
  let mut position = first * block;
  let mut pcm = Vec::new();
  while left > 0 {
    let at = match source.read(&format, &mut pcm) {
      Ok(frame) => frame.map_or(position, |frame| frame * block),
      Err(reason) => return failed(reason),
    };
    if pcm.is_empty() {
      return failed("the audio ends before its stated length".to_string());
    }
    let end = at + pcm.len() as u64;
    position = end;
    // a sought packet can start before the range, and a range can start inside a sample
    if end <= start {
      continue;
    }
    let skip = start.saturating_sub(at) as usize;
    let piece = &pcm[skip..];
    let piece = &piece[..piece.len().min(left as usize)];
    left -= piece.len() as u64;
    if pieces
      .blocking_send(Ok(Bytes::copy_from_slice(piece)))
      .is_err()
    {
      return;
    }
  }
}

/// The WAV's `length` bytes from `start`, read as they are sent and no faster than `speed` allows.
fn body(opened: Opened, path: PathBuf, start: u64, length: u64, speed: ServeSpeed) -> Body {
  let Opened { format, source } = opened;
  let Parts {
    header,
    pcm_from,
    pcm_length,
  } = parts(start, length);
  let head = Bytes::copy_from_slice(&format.header()[header]);
  let (sender, receiver) = mpsc::channel(BACKLOG);
  if pcm_length > 0 {
    tokio::task::spawn_blocking(move || {
      let shown = path.display().to_string();
      read_pcm(source, format, pcm_from, pcm_length, &shown, &sender);
    });
  }
  let pcm = stream::unfold((receiver, speed), |(mut receiver, speed)| async move {
    let piece = receiver.recv().await?;
    if let Ok(bytes) = &piece {
      let mut left = bytes.len();
      while left > 0 {
        left -= speed.allowed(left).await;
      }
    }
    Some((piece, (receiver, speed)))
  });
  Body::from_stream(stream::iter((!head.is_empty()).then_some(Ok(head))).chain(pcm))
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
  let opened = tokio::task::spawn_blocking(move || open(&path, size))
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
  if !opened.format.fits() {
    return Err(ApiError::unsupported_media(
      "This file cannot be played: it is too long for a WAV",
    ));
  }
  let entity = Entity {
    name: Path::new(&file.name())
      .with_extension("wav")
      .to_string_lossy()
      .into_owned(),
    etag: file.etag_of("wav"),
    kind: "audio/wav".to_string(),
    size: HEADER + opened.format.pcm(),
  };
  match serve::ranged(&entity, request) {
    Ranged::Answered(response) => Ok(response),
    Ranged::Bytes(part) => {
      let (start, length) = (part.start, part.length);
      Ok(part.with(body(opened, file.path, start, length, speed.clone())))
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
    let header = format.header();
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
}
