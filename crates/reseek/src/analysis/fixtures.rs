//! Audio the analyser is tested on, made in the test from a fixed seed: noise shaped like
//! music's spectrum, written as FLAC, and the same noise through the LAME MP3 encoder and back
//! for the transcodes. Nothing here is fetched or shipped as a binary.

use std::path::Path;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap};
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

use super::decode::{Decoder, Filled};

/// How long most generated files are, in seconds: more than the 30 s a reading needs.
pub const SECONDS: usize = 34;

/// Audio in memory: a sample rate and one sample vector per channel.
#[derive(Clone)]
pub struct Audio {
  pub rate: u32,
  pub channels: Vec<Vec<f32>>,
}

/// A small fixed-seed generator, so every run makes the same audio.
struct Rng(u64);

impl Rng {
  fn next(&mut self) -> f64 {
    // xorshift64*
    self.0 ^= self.0 >> 12;
    self.0 ^= self.0 << 25;
    self.0 ^= self.0 >> 27;
    let value = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
    (value >> 11) as f64 / (1u64 << 53) as f64
  }

  /// A normally distributed value.
  fn gaussian(&mut self) -> f64 {
    let u = self.next().max(f64::MIN_POSITIVE);
    let v = self.next();
    (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
  }
}

/// `seconds` of a sine at `hz`, one channel at 44.1 kHz, at -15 dBFS: a test tone.
pub fn tone(hz: f64, seconds: usize) -> Audio {
  let rate = 44_100u32;
  let step = 2.0 * std::f64::consts::PI * hz / f64::from(rate);
  let mut audio = Audio {
    rate,
    channels: vec![
      (0..seconds * rate as usize)
        .map(|i| ((i as f64 * step) % (2.0 * std::f64::consts::PI)).sin() as f32)
        .collect(),
    ],
  };
  audio.set_level(-15.0);
  audio
}

/// [`SECONDS`] of a sawtooth at `hz`, its harmonics up to `top` falling 6 dB an octave, as a
/// synth or a bowed string has, one channel at 44.1 kHz at -15 dBFS.
pub fn sawtooth(hz: f64, top: f64) -> Audio {
  let rate = 44_100u32;
  let mut channel = vec![0f32; SECONDS * rate as usize];
  let mut harmonic = 1.0;
  while hz * harmonic < top {
    let step = 2.0 * std::f64::consts::PI * hz * harmonic / f64::from(rate);
    for (i, sample) in channel.iter_mut().enumerate() {
      *sample += (((i as f64 * step) % (2.0 * std::f64::consts::PI)).sin() / harmonic) as f32;
    }
    harmonic += 1.0;
  }
  let mut audio = Audio {
    rate,
    channels: vec![channel],
  };
  audio.set_level(-15.0);
  audio
}

/// Music's spectrum, roughly: power falling 3 dB an octave, from 20 Hz up.
pub fn pink(hz: f64) -> f64 {
  if hz < 20.0 { 0.0 } else { (100.0 / hz).sqrt() }
}

/// [`pink`] with nothing above `hz`: an encoder's low-pass, or a filter applied when mastering.
pub fn cut_at(hz: f64) -> impl Fn(f64) -> f64 + Copy {
  move |f| if f < hz { pink(f) } else { 0.0 }
}

/// [`pink`] with a comb of gaps 40 Hz wide every 80 Hz between 16 and 20 kHz: what the gap
/// measure counts.
pub fn combed(hz: f64) -> f64 {
  if (16_000.0..20_000.0).contains(&hz) && (hz % 80.0) < 40.0 {
    0.0
  } else {
    pink(hz)
  }
}

/// [`pink`] rolling off gently from 3.5 kHz, 24 dB an octave, as a lo-fi recording does.
pub fn lo_fi(hz: f64) -> f64 {
  pink(hz) / (1.0 + (hz / 3_500.0).powi(8)).sqrt()
}

/// `seconds` of noise at `rate` whose spectrum has the shape `envelope`, one channel.
fn noise(seed: u64, seconds: usize, rate: u32, envelope: impl Fn(f64) -> f64) -> Vec<f32> {
  let length = seconds * rate as usize;
  let mut rng = Rng(seed | 1);
  let mut planner = RealFftPlanner::<f32>::new();
  let forward = planner.plan_fft_forward(length);
  let inverse = planner.plan_fft_inverse(length);
  let mut samples: Vec<f32> = (0..length).map(|_| rng.gaussian() as f32).collect();
  let mut spectrum = forward.make_output_vec();
  forward.process(&mut samples, &mut spectrum).unwrap();
  let bin_hz = f64::from(rate) / length as f64;
  for (bin, value) in spectrum.iter_mut().enumerate() {
    *value *= envelope(bin as f64 * bin_hz) as f32;
  }
  spectrum[0] = Complex::default();
  if let Some(last) = spectrum.last_mut() {
    last.im = 0.0;
  }
  inverse.process(&mut spectrum, &mut samples).unwrap();
  samples
}

/// [`SECONDS`] of two partly correlated channels of shaped noise, with a slow swell in
/// loudness, at -20 dBFS.
pub fn music(seed: u64, rate: u32, envelope: impl Fn(f64) -> f64 + Copy) -> Audio {
  music_for(SECONDS, seed, rate, envelope)
}

/// [`music`] lasting `seconds`.
pub fn music_for(
  seconds: usize,
  seed: u64,
  rate: u32,
  envelope: impl Fn(f64) -> f64 + Copy,
) -> Audio {
  let common = noise(seed, seconds, rate, envelope);
  let apart = noise(seed.wrapping_mul(31), seconds, rate, envelope);
  let swell = |i: usize| 0.75 + 0.25 * (i as f64 / f64::from(rate) * 1.3).sin();
  let channel = |sign: f32| -> Vec<f32> {
    common
      .iter()
      .zip(&apart)
      .enumerate()
      .map(|(i, (c, a))| (c + sign * 0.5 * a) * swell(i) as f32)
      .collect()
  };
  let mut audio = Audio {
    rate,
    channels: vec![channel(1.0), channel(-1.0)],
  };
  audio.set_level(-20.0);
  audio
}

impl Audio {
  /// Scales the audio to an RMS level of `dbfs`.
  pub fn set_level(&mut self, dbfs: f64) {
    let count: usize = self.channels.iter().map(Vec::len).sum();
    let squares: f64 = self
      .channels
      .iter()
      .flatten()
      .map(|s| f64::from(*s).powi(2))
      .sum();
    let gain = (10f64.powf(dbfs / 20.0) / (squares / count as f64).sqrt()) as f32;
    for sample in self.channels.iter_mut().flatten() {
      *sample *= gain;
    }
  }

  /// The audio with `other` added in at `gain`, sample for sample.
  pub fn mixed(mut self, other: &Audio, gain: f32) -> Audio {
    for (mine, theirs) in self.channels.iter_mut().zip(&other.channels) {
      for (a, b) in mine.iter_mut().zip(theirs) {
        *a += gain * b;
      }
    }
    self
  }

  /// The first `seconds` of `self`, then the rest of `other`.
  pub fn spliced(&self, other: &Audio, seconds: usize) -> Audio {
    let split = seconds * self.rate as usize;
    Audio {
      rate: self.rate,
      channels: self
        .channels
        .iter()
        .zip(&other.channels)
        .map(|(a, b)| {
          a[..split]
            .iter()
            .chain(&b[split..b.len().min(a.len())])
            .copied()
            .collect()
        })
        .collect(),
    }
  }

  /// The audio with a short burst of noise at the start of every `period` samples, as a
  /// lossy codec can leave at its frame edges.
  pub fn with_frame_rhythm(mut self, period: usize) -> Audio {
    let mut rng = Rng(5);
    for channel in &mut self.channels {
      for (i, sample) in channel.iter_mut().enumerate() {
        if i % period < 64 {
          *sample += 0.06 * rng.gaussian() as f32;
        }
      }
    }
    self
  }

  /// The first channel twice, as a mono recording copied to stereo is.
  pub fn mono_to_stereo(&self) -> Audio {
    Audio {
      rate: self.rate,
      channels: vec![self.channels[0].clone(), self.channels[0].clone()],
    }
  }

  /// The audio resampled to `rate` by padding its spectrum with silence: what a converter
  /// makes of it, with nothing above the old rate's band.
  pub fn resampled(&self, rate: u32) -> Audio {
    let mut planner = RealFftPlanner::<f32>::new();
    let channels = self
      .channels
      .iter()
      .map(|channel| {
        let length = channel.len();
        let target = length * rate as usize / self.rate as usize;
        let forward = planner.plan_fft_forward(length);
        let inverse = planner.plan_fft_inverse(target);
        let mut input = channel.clone();
        let mut spectrum = forward.make_output_vec();
        forward.process(&mut input, &mut spectrum).unwrap();
        let mut wide = inverse.make_input_vec();
        let shared = spectrum.len().min(wide.len()) - 1;
        wide[..shared].copy_from_slice(&spectrum[..shared]);
        let mut output = inverse.make_output_vec();
        inverse.process(&mut wide, &mut output).unwrap();
        // each transform scales by its length; this undoes the forward one
        let scale = 1.0 / length as f32;
        output.iter_mut().for_each(|s| *s *= scale);
        output
      })
      .collect();
    let mut audio = Audio { rate, channels };
    let level = self.level();
    audio.set_level(level);
    audio
  }

  fn level(&self) -> f64 {
    let count: usize = self.channels.iter().map(Vec::len).sum();
    let squares: f64 = self
      .channels
      .iter()
      .flatten()
      .map(|s| f64::from(*s).powi(2))
      .sum();
    10.0 * (squares / count as f64).log10()
  }

  /// The audio as integers of `bits`, with triangular dither, the same dither in every
  /// channel so identical channels stay identical.
  fn quantised(&self, bits: u32, seed: u64) -> Vec<Vec<i32>> {
    let full = f64::from(1u32 << (bits - 1));
    self
      .channels
      .iter()
      .map(|channel| {
        let mut rng = Rng(seed | 1);
        channel
          .iter()
          .map(|s| {
            let dither = rng.next() - rng.next();
            (f64::from(*s) * full + dither)
              .round()
              .clamp(-full, full - 1.0) as i32
          })
          .collect()
      })
      .collect()
  }
}

/// Writes `audio` as FLAC at `path`: as `bits`-bit samples, or as 16-bit samples stored in 24
/// bits when `padded`.
pub fn write_flac(audio: &Audio, bits: u32, padded: bool, path: &Path) {
  let mut planes = audio.quantised(if padded { 16 } else { bits }, 7);
  if padded {
    planes.iter_mut().flatten().for_each(|s| *s <<= 8);
  }
  let config = flacenc::config::Encoder::default()
    .into_verified()
    .map_err(|(_, e)| e)
    .unwrap();
  // flacenc states the short last block as the stream's smallest, which readers take as a
  // stream of varying block sizes; whole blocks only keep the stream a fixed-size one
  let frames = planes[0].len() / config.block_size * config.block_size;
  let interleaved: Vec<i32> = (0..frames)
    .flat_map(|i| planes.iter().map(move |p| p[i]))
    .collect();
  let source = flacenc::source::MemSource::from_samples(
    &interleaved,
    planes.len(),
    bits as usize,
    audio.rate as usize,
  );
  let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).unwrap();
  let mut sink = flacenc::bitsink::ByteSink::new();
  stream.write(&mut sink).unwrap();
  std::fs::write(path, sink.as_slice()).unwrap();
}

/// Writes `audio`, two channels at 44.1 kHz, as a constant-bitrate MP3 at `path`.
pub fn write_mp3(audio: &Audio, bitrate: Bitrate, path: &Path) {
  let planes = audio.quantised(16, 11);
  let left: Vec<i16> = planes[0].iter().map(|s| *s as i16).collect();
  let right: Vec<i16> = planes[1].iter().map(|s| *s as i16).collect();
  let mut encoder = Builder::new()
    .unwrap()
    .with_num_channels(2)
    .unwrap()
    .with_sample_rate(audio.rate)
    .unwrap()
    .with_brate(bitrate)
    .unwrap()
    .build()
    .unwrap();
  // the encoder writes into the space reserved and no further
  let mut bytes = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(left.len()));
  encoder
    .encode_to_vec(
      DualPcm {
        left: &left,
        right: &right,
      },
      &mut bytes,
    )
    .unwrap();
  bytes.reserve(7200);
  encoder.flush_to_vec::<FlushNoGap>(&mut bytes).unwrap();
  std::fs::write(path, bytes).unwrap();
}

/// Puts an ID3v2.3 tag holding a cover picture of `bytes` bytes in front of the MP3 at `path`,
/// as a tagger embeds album art.
pub fn add_cover(path: &Path, bytes: usize) {
  let mut picture = vec![0u8; bytes];
  picture[..4].copy_from_slice(&[0xff, 0xd8, 0xff, 0xe0]);
  let mut frame = vec![0u8];
  frame.extend_from_slice(b"image/jpeg\0\x03\0");
  frame.extend_from_slice(&picture);
  let mut tag = b"APIC".to_vec();
  tag.extend_from_slice(&(frame.len() as u32).to_be_bytes());
  tag.extend_from_slice(&[0, 0]);
  tag.extend_from_slice(&frame);
  // the tag's size is written seven bits to a byte
  let size = tag.len() as u32;
  let synchsafe = [size >> 21, size >> 14, size >> 7, size].map(|part| (part & 0x7f) as u8);
  let mut file = b"ID3\x03\0\0".to_vec();
  file.extend_from_slice(&synchsafe);
  file.extend_from_slice(&tag);
  file.extend_from_slice(&std::fs::read(path).unwrap());
  std::fs::write(path, file).unwrap();
}

/// The audio of the file at `path`, decoded whole.
pub fn decoded(path: &Path) -> Audio {
  let mut decoder = Decoder::open(path).unwrap();
  let rate = decoder.claimed.sample_rate;
  let mut channels = vec![Vec::new(); decoder.channels];
  let mut window = vec![Vec::new(); decoder.channels];
  loop {
    let filled = decoder.fill(&mut window, rate as usize);
    for (all, part) in channels.iter_mut().zip(&window) {
      all.extend_from_slice(part);
    }
    if filled != Filled::Full {
      break;
    }
  }
  Audio { rate, channels }
}

/// `audio` through an MP3 encoder at `bitrate` and decoded again, as a transcode starts.
pub fn through_mp3(audio: &Audio, bitrate: Bitrate, dir: &Path) -> Audio {
  let mp3 = dir.join(format!("through-{}.mp3", bitrate as u32));
  write_mp3(audio, bitrate, &mp3);
  decoded(&mp3)
}
