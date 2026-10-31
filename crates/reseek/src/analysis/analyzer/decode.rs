//! Reading an audio file as samples, a little at a time: the container and codec are found by
//! probing, packets are decoded one by one, and the analyzer takes the first two channels as
//! floats in one-second windows. Integer samples are also looked at as they come, for the bits
//! they actually use.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::audio::sample::{i24, u24};
use symphonia::core::audio::{Audio, AudioBuffer};
use symphonia::core::codecs::audio::well_known::{
  CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_PCM_ALAW, CODEC_ID_PCM_MULAW,
};
use symphonia::core::codecs::audio::{
  AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions,
};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};
use symphonia_common::apple::audio::alac::MagicCookie;
use tracing::{debug, trace};

/// The most channels the analyzer reads; the rest of a surround file is left out.
pub(in crate::analysis) const ANALYZED_CHANNELS: usize = 2;

/// The highest sample rate the analyzer reads; a window holds a second of samples, so a header
/// claiming more is not trusted with the memory.
const MAX_RATE: u32 = 768_000;

/// How many undecodable packets are skipped before reading stops.
const MAX_SKIPPED: u32 = 1000;

/// The format a file's container and codec claim.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Claimed {
  /// the codec's short name, such as `flac` or `mp3`
  pub codec: String,
  /// samples per second
  pub sample_rate: u32,
  /// bits per sample, `None` for a codec without integer samples
  pub bit_depth: Option<u32>,
  /// every channel of the file, analyzed or not
  pub channels: u32,
  /// whether the codec keeps every bit of the audio
  pub lossless: bool,
  /// the length the container states, in sample frames, when it states one
  pub frames: Option<u64>,
}

/// An audio file open for decoding.
pub(crate) struct Decoder {
  format: Box<dyn FormatReader>,
  decoder: Box<dyn AudioDecoder>,
  track: u32,
  /// what the track's timestamps count, when the container says
  time_base: Option<TimeBase>,
  /// what the file claims
  pub claimed: Claimed,
  /// the channels analyzed: one for mono, else the first two
  pub channels: usize,
  /// decoded samples not taken yet, one vector per analyzed channel
  pending: Vec<Vec<f32>>,
  /// where the next sample of `pending` starts in `pending`
  taken: usize,
  /// a scratch buffer each packet is converted into
  scratch: Vec<Vec<f32>>,
  /// the bits set in any integer sample decoded, and the width of the sample type
  bits: Option<(u32, u32)>,
  /// the bytes and the sample frames of the packets decoded, for the stream's bitrate
  decoded: (u64, u64),
  /// packets the decoder could not read and skipped
  pub skipped: u32,
  /// why decoding stopped before the end of the file, when it did
  pub stopped: Option<String>,
}

/// What filling a window gave.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::analysis) enum Filled {
  /// the window is full
  Full,
  /// the file ended first; the window holds this many frames
  End(usize),
}

impl Decoder {
  /// Opens the file at `path`, finding its container from its contents and its extension.
  pub fn open(path: &Path) -> Result<Self, String> {
    let file = File::open(path).map_err(|e| format!("the file could not be opened: {e}"))?;
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
      hint.with_extension(extension);
    }
    Self::from_source(Box::new(file), &hint)
  }

  /// Opens audio from any source of bytes, with `hint` naming what it might be.
  pub fn from_source(source: Box<dyn MediaSource>, hint: &Hint) -> Result<Self, String> {
    let stream = MediaSourceStream::new(source, Default::default());
    let format = symphonia::default::get_probe()
      .probe(
        hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
      )
      .map_err(|e| format!("the container was not recognized: {e}"))?;
    let track = format
      .default_track(TrackType::Audio)
      .ok_or("the file holds no audio track")?;
    let params = track
      .codec_params
      .as_ref()
      .and_then(|p| p.audio())
      .ok_or("the audio track has no codec parameters")?
      .clone();
    let track_id = track.id;
    let time_base = track.time_base;
    let frames = track.num_frames;
    let codecs = symphonia::default::get_codecs();
    let codec = codecs
      .get_audio_decoder(params.codec)
      .map(|registered| registered.codec.info.short_name.to_string())
      .ok_or("the codec is not one the analyzer can decode")?;
    let decoder = codecs
      .make_audio_decoder(&params, &AudioDecoderOptions::default())
      .map_err(|e| format!("the decoder could not start: {e}"))?;
    let sample_rate = params
      .sample_rate
      .filter(|rate| *rate > 0)
      .ok_or("the sample rate is not stated")?;
    if sample_rate > MAX_RATE {
      return Err(format!(
        "the sample rate of {sample_rate} Hz is above the {MAX_RATE} Hz the analyzer reads"
      ));
    }
    let all = params.channels.as_ref().map_or(0, |c| c.count());
    if all == 0 {
      return Err("the number of channels is not stated".to_string());
    }
    let channels = all.min(ANALYZED_CHANNELS);
    debug!(codec, sample_rate, channels = all, "audio opened");
    Ok(Self {
      format,
      decoder,
      track: track_id,
      time_base,
      claimed: Claimed {
        codec,
        sample_rate,
        bit_depth: stated_bits(&params),
        channels: u32::try_from(all).unwrap_or(u32::MAX),
        lossless: lossless(params.codec),
        frames,
      },
      channels,
      pending: vec![Vec::new(); channels],
      taken: 0,
      scratch: Vec::new(),
      bits: None,
      decoded: (0, 0),
      skipped: 0,
      stopped: None,
    })
  }

  /// The bits per sample the integer samples decoded so far use: the sample type's width less
  /// the low bits that were zero in every one. `None` for float samples and while every sample
  /// was zero.
  pub fn bits_used(&self) -> Option<u32> {
    let (set, width) = self.bits?;
    (set != 0).then(|| width - set.trailing_zeros().min(width))
  }

  /// The audio's bitrate in kbit/s from the packets decoded so far: their bytes over their
  /// length, so tags and cover art around the stream do not count. `None` before any.
  pub fn bitrate_kbps(&self) -> Option<u32> {
    let (bytes, frames) = self.decoded;
    (frames > 0).then(|| {
      let seconds = frames as f64 / f64::from(self.claimed.sample_rate);
      (bytes as f64 * 8.0 / seconds / 1000.0).round() as u32
    })
  }

  /// Fills `window`, one vector per analyzed channel, with the next `frames` sample frames.
  pub(in crate::analysis) fn fill(&mut self, window: &mut [Vec<f32>], frames: usize) -> Filled {
    for channel in window.iter_mut() {
      channel.clear();
    }
    loop {
      let available = self.pending[0].len() - self.taken;
      let wanted = frames - window[0].len();
      let moved = available.min(wanted);
      for (channel, pending) in window.iter_mut().zip(&self.pending) {
        channel.extend_from_slice(&pending[self.taken..self.taken + moved]);
      }
      self.taken += moved;
      if window[0].len() == frames {
        return Filled::Full;
      }
      if !self.decode_next() {
        return Filled::End(window[0].len());
      }
    }
  }

  /// Moves to `seconds` from the start, dropping what was decoded and not taken.
  pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
    let time = Time::try_from_secs_f64(seconds).ok_or("the position is out of range")?;
    self
      .format
      .seek(
        SeekMode::Accurate,
        SeekTo::Time {
          time,
          track_id: Some(self.track),
        },
      )
      .map_err(|e| format!("seeking failed: {e}"))?;
    self.decoder.reset();
    for pending in &mut self.pending {
      pending.clear();
    }
    self.taken = 0;
    Ok(())
  }

  /// Moves to the packet holding sample frame `frame`.
  pub fn seek_frame(&mut self, frame: u64) -> Result<(), String> {
    let (numer, denom) = self.ticks();
    let rate = u128::from(self.claimed.sample_rate);
    let ticks = u128::from(frame) * denom / (numer * rate);
    let ts = i64::try_from(ticks).map_err(|_| "the position is out of range")?;
    self
      .format
      .seek(
        SeekMode::Accurate,
        SeekTo::Timestamp {
          ts: Timestamp::new(ts),
          track_id: self.track,
        },
      )
      .map_err(|e| format!("seeking failed: {e}"))?;
    self.decoder.reset();
    Ok(())
  }

  /// Decodes the next packet into `samples`, interleaved over every channel with each sample at
  /// the top of its 32 bits, and returns the sample frame it starts at; `None` at the end of the
  /// file or when decoding cannot go on, with the reason in `stopped`.
  pub fn next_interleaved(&mut self, samples: &mut Vec<i32>) -> Option<u64> {
    let pts = self.step()?;
    self.decoder.last_decoded().copy_to_vec_interleaved(samples);
    let (numer, denom) = self.ticks();
    let frame = u128::try_from(pts).unwrap_or(0) * numer * u128::from(self.claimed.sample_rate);
    Some(u64::try_from(frame / denom).unwrap_or(u64::MAX))
  }

  /// A timestamp tick as a fraction of a second; one sample frame when the container gives none.
  fn ticks(&self) -> (u128, u128) {
    self.time_base.map_or_else(
      || (1, u128::from(self.claimed.sample_rate)),
      |base| (u128::from(base.numer.get()), u128::from(base.denom.get())),
    )
  }

  /// Decodes the next packet of the track into `pending`; false at the end of the file or when
  /// decoding cannot go on, with the reason in `stopped`.
  fn decode_next(&mut self) -> bool {
    let shift = self.taken;
    for pending in &mut self.pending {
      pending.drain(..shift);
    }
    self.taken = 0;
    if self.step().is_none() {
      return false;
    }
    self
      .decoder
      .last_decoded()
      .copy_to_vecs_planar::<f32>(&mut self.scratch);
    // the analyzed channels must stay the same length
    if self.scratch.len() < self.channels {
      debug!(
        planes = self.scratch.len(),
        "a packet holds fewer channels than the stream"
      );
      self.stopped = Some("a packet holds fewer channels than the stream".to_string());
      return false;
    }
    for (pending, plane) in self.pending.iter_mut().zip(&self.scratch) {
      pending.extend_from_slice(plane);
    }
    true
  }

  /// Decodes the next packet of the track that holds samples, which the decoder then holds, and
  /// returns its timestamp; `None` at the end of the file or when decoding cannot go on, with
  /// the reason in `stopped`.
  fn step(&mut self) -> Option<i64> {
    loop {
      let packet = match self.format.next_packet() {
        Ok(Some(packet)) => packet,
        Ok(None) => return None,
        Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
          debug!("audio ends early");
          self.stopped = Some("the file ends before its last packet".to_string());
          return None;
        }
        Err(e) => {
          debug!(error = %e, "audio could not be read on");
          self.stopped = Some(format!("reading stopped: {e}"));
          return None;
        }
      };
      if packet.track_id != self.track {
        continue;
      }
      let decoded = match self.decoder.decode(&packet) {
        Ok(decoded) => decoded,
        Err(Error::DecodeError(reason)) => {
          trace!(reason, "audio packet skipped");
          if self.skip() {
            return None;
          }
          continue;
        }
        Err(Error::IoError(e)) => {
          trace!(error = %e, "audio packet skipped");
          if self.skip() {
            return None;
          }
          continue;
        }
        Err(e) => {
          debug!(error = %e, "audio could not be decoded on");
          self.stopped = Some(format!("decoding stopped: {e}"));
          return None;
        }
      };
      if decoded.frames() == 0 {
        continue;
      }
      note_bits(&mut self.bits, &decoded);
      self.decoded.0 += packet.data.len() as u64;
      self.decoded.1 += decoded.frames() as u64;
      return Some(packet.pts.get());
    }
  }

  /// Counts a packet skipped; true, with the reason in `stopped`, once so many were skipped
  /// that reading on is not worth it.
  fn skip(&mut self) -> bool {
    self.skipped += 1;
    if self.skipped < MAX_SKIPPED {
      return false;
    }
    debug!(skipped = self.skipped, "too many audio packets skipped");
    self.stopped = Some(format!(
      "reading stopped after {MAX_SKIPPED} packets could not be decoded"
    ));
    true
  }
}

/// Whether a codec keeps every bit: the lossless codecs and plain PCM, but not the companded
/// telephone formats.
/// The bits per sample `params` state. A container says nothing of them for ALAC, whose magic
/// cookie does; the decoder made from `params` read the same cookie, so a cookie that does not
/// read here never gets this far.
fn stated_bits(params: &AudioCodecParameters) -> Option<u32> {
  params.bits_per_sample.or_else(|| {
    let cookie = params.extra_data.as_deref()?;
    let read = (params.codec == CODEC_ID_ALAC).then(|| MagicCookie::read(cookie));
    Some(u32::from(read?.ok()?.bit_depth))
  })
}

fn lossless(codec: AudioCodecId) -> bool {
  if codec == CODEC_ID_FLAC || codec == CODEC_ID_ALAC {
    return true;
  }
  codec != CODEC_ID_PCM_ALAW
    && codec != CODEC_ID_PCM_MULAW
    && symphonia::default::get_codecs()
      .get_audio_decoder(codec)
      .is_some_and(|registered| registered.codec.info.short_name.starts_with("pcm_"))
}

/// Adds the bits set in the integer samples of `decoded` to `bits`, kept with the sample
/// type's width. Decoders put a sample's bits at the top of its type, so bits never used show
/// as zeros at the bottom.
fn note_bits(bits: &mut Option<(u32, u32)>, decoded: &GenericAudioBufferRef<'_>) {
  fn set<S: symphonia::core::audio::sample::Sample>(
    buffer: &AudioBuffer<S>,
    raw: impl Fn(S) -> u32,
  ) -> u32 {
    buffer
      .iter_planes()
      .flat_map(|plane| plane.iter().copied())
      .fold(0, |set, sample| set | raw(sample))
  }
  let (found, width) = match decoded {
    GenericAudioBufferRef::U8(b) => (set(b, u32::from), 8),
    GenericAudioBufferRef::S8(b) => (set(b, |s| u32::from(s as u8)), 8),
    GenericAudioBufferRef::U16(b) => (set(b, u32::from), 16),
    GenericAudioBufferRef::S16(b) => (set(b, |s| u32::from(s as u16)), 16),
    GenericAudioBufferRef::U24(b) => (set(b, |s: u24| s.inner() & 0xff_ffff), 24),
    GenericAudioBufferRef::S24(b) => (set(b, |s: i24| s.inner() as u32 & 0xff_ffff), 24),
    GenericAudioBufferRef::U32(b) => (set(b, |s| s), 32),
    GenericAudioBufferRef::S32(b) => (set(b, |s| s as u32), 32),
    _ => return,
  };
  let (before, _) = bits.unwrap_or((0, width));
  *bits = Some((before | found, width));
}
