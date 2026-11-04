//! Reading the PCM of a WAV a stretch at a time: samples read from an AIFF file and turned
//! around, or decoded from ALAC and placed by their packet's time, with silence where packets
//! leave a gap or the audio ends before its stated length.

use std::io::{self, Read, Seek, SeekFrom};

use axum::body::Bytes;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSource;

use super::super::aiff::{self, Aiff};
use super::{Format, Opened};
use crate::analysis::Decoder;

/// How much PCM one piece of the response read from an AIFF file holds, at least, and how
/// much silence one piece holds at most.
const PIECE: usize = 256 * 1024;
/// The bytes a piece of silence is cut from.
static SILENCE: [u8; PIECE] = [0; PIECE];

/// Opens the file again, from its start.
pub(super) type Reopen = Box<dyn Fn() -> io::Result<Box<dyn MediaSource>> + Send>;

/// Where the PCM comes from.
pub(super) enum Source {
  /// samples read from an AIFF file and turned around
  Aiff(Aiff, Box<dyn MediaSource>),
  /// samples decoded from ALAC, each kept to its top `width` bytes; `reopen` and `hint` open the
  /// file's container anew
  Alac {
    decoder: Box<Decoder>,
    hint: Hint,
    reopen: Reopen,
  },
}

impl Source {
  /// Moves to sample frame `frame` of `format`, or for ALAC to the packet that holds it. False
  /// when the container cannot place the frame: the audio is then at its start instead.
  fn seek(&mut self, format: &Format, frame: u64) -> Result<bool, String> {
    match self {
      // the frames an AIFF states are clipped to the samples it holds, so every frame is there
      Self::Aiff(aiff, file) => file
        .seek(SeekFrom::Start(aiff.data_at + frame * format.block()))
        .map(|_| true)
        .map_err(|e| format!("seeking failed: {e}")),
      Self::Alac {
        decoder,
        hint,
        reopen,
      } => {
        if decoder.seek_frame(frame)? {
          return Ok(true);
        }
        // symphonia 0.6.1's MP4 reader, when a seek finds no sample, may walk the file for more
        // fragments and drop the media data atom it was reading, so every later packet fails
        // with "no atom pending read"; the container is opened anew. TODO: seek to 0 in place
        // once the reader survives a failed seek
        let file = reopen().map_err(|e| format!("the file could not be opened again: {e}"))?;
        let reopened = Decoder::from_source(file, hint)?;
        if reopened.claimed != decoder.claimed {
          return Err("the file changed while it was read".to_string());
        }
        **decoder = reopened;
        Ok(false)
      }
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
      Self::Alac { decoder, .. } => {
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

/// The PCM of a WAV from one byte to another, read a stretch of the source at a time.
pub(super) struct Pcm {
  source: Source,
  format: Format,
  /// the next byte of the PCM to give
  at: u64,
  /// the byte of the PCM where giving stops
  end: u64,
  /// where the stretch read last ends in the PCM, `None` until the source was moved to where the
  /// PCM starts and read
  next: Option<u64>,
  /// a stretch read and not given yet, with where it starts in the PCM
  held: Option<(u64, Bytes)>,
  /// whether the source ended before the PCM did, so the rest is silence
  ended: bool,
  /// the bytes of silence the end of the source left, until it is logged
  pub(super) short: Option<u64>,
}

impl Pcm {
  /// `length` bytes of the PCM of `opened` from byte `start`.
  pub(super) fn new(opened: Opened, start: u64, length: u64) -> Self {
    Self {
      source: opened.source,
      format: opened.wav.format,
      at: start,
      end: start + length,
      next: None,
      held: None,
      ended: false,
      short: None,
    }
  }

  /// The next piece, `None` once every byte was given or after a piece that failed. Reads the
  /// source until it has a piece, which blocks; when the range was not sought, that is every
  /// packet before it.
  pub(super) fn step(&mut self) -> Option<Result<Bytes, String>> {
    while self.at < self.end {
      if let Some((from, stretch)) = self.held.take() {
        // packets dropped as undecodable, or holding fewer frames than they last, leave silence
        if from > self.at {
          let gap = from - self.at;
          self.held = Some((from, stretch));
          return Some(Ok(self.silence(gap)));
        }
        // a sought packet can start before the range, and a range can start inside a sample
        let skip = self.at - from;
        let length = stretch.len() as u64;
        if skip < length {
          let given = (length - skip).min(self.end - self.at);
          self.at += given;
          return Some(Ok(stretch.slice(skip as usize..(skip + given) as usize)));
        }
        continue;
      }
      if self.ended {
        return Some(Ok(self.silence(self.end - self.at)));
      }
      if let Err(reason) = self.read() {
        self.at = self.end;
        return Some(Err(reason));
      }
    }
    None
  }

  /// Up to `length` bytes of silence, as much as one piece holds and the range still wants.
  fn silence(&mut self, length: u64) -> Bytes {
    let given = length.min(self.end - self.at).min(PIECE as u64);
    self.at += given;
    Bytes::from_static(&SILENCE[..given as usize])
  }

  /// Notes that the source ended before the PCM, so the rest is silence.
  fn end_source(&mut self) {
    self.ended = true;
    self.short = Some(self.end - self.at);
  }

  /// Reads the next stretch of the source into `held`, or notes that the source ended.
  fn read(&mut self) -> Result<(), String> {
    let block = self.format.block();
    let out_of_range = || "the file's timing is out of range".to_string();
    let next = match self.next {
      Some(next) => next,
      None => {
        let first = self.at / block;
        // when the frame could not be sought, the packets are read from the start and those
        // before the range dropped, so a frame past the last packet is silence and one past
        // where another track ends is still audio
        if self.source.seek(&self.format, first)? {
          first * block
        } else {
          0
        }
      }
    };
    let mut pcm = Vec::new();
    let frame = self.source.read(&self.format, &mut pcm)?;
    if pcm.is_empty() {
      self.end_source();
      return Ok(());
    }
    let from = match frame {
      Some(frame) => frame.checked_mul(block).ok_or_else(out_of_range)?,
      None => next,
    };
    // the packet a seek lands on can start before the range, a later one only where the last
    // one ends or after
    if self.next.is_some() && from < next {
      return Err("the file's packets overlap in time".to_string());
    }
    let to = from
      .checked_add(pcm.len() as u64)
      .ok_or_else(out_of_range)?;
    self.next = Some(to);
    self.held = Some((from, Bytes::from(pcm)));
    Ok(())
  }
}
