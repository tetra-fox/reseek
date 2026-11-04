//! A file the browser cannot play, answered as a WAV made as it is read and never stored: a
//! header followed by PCM. AIFF needs its samples swapped to little-endian, ALAC is decoded. The
//! length is known from the start, and a range maps to a place in the PCM, so seeking works.

mod pcm;

use std::fs::File;
use std::io::{self, Cursor, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;

use axum::body::{Body, Bytes};
use axum::response::Response;
use futures_util::{StreamExt, stream};
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSource;
use tracing::{debug, error, warn};

use super::ServeSpeed;
use super::aiff::{self, Header};
use super::serve::{self, Entity, Ranged, Served};
use crate::analysis::Decoder;
use crate::api::error::ApiError;
use crate::services::ContentRequest;
use pcm::{Pcm, Source};

/// The bytes of the WAV header.
const HEADER: u64 = 44;
/// The most a RIFF file can say it holds.
const MAX_RIFF: u64 = u32::MAX as u64;
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
  let mut decoder = Decoder::from_source(file, &hint).map_err(|reason| {
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
  let wav = format.fits().map_err(Refused::Format)?;
  // a file nothing can be decoded from is refused before the response starts; reading seeks to
  // where it starts, so the packet decoded here is decoded again
  if decoder.next_interleaved(&mut Vec::new()).is_none() {
    let reason = match (decoder.stopped.take(), decoder.last_skipped.take()) {
      (Some(stopped), _) => stopped,
      (None, Some(skipped)) => format!("no packet of the audio could be decoded: {skipped}"),
      (None, None) => "the file holds no audio".to_string(),
    };
    return Err(Refused::Format(reason));
  }
  Ok(Opened {
    wav,
    source: Source::Alac(Box::new(decoder)),
  })
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
      Ok((mut pcm, Some(Ok(bytes)))) => {
        if let Some(missing) = pcm.short.take() {
          warn!(
            path = shown,
            missing, "a file served as WAV ends before its stated length; the rest is silence"
          );
        }
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
mod tests;
