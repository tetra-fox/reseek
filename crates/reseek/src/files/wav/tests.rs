//! The WAV header and the range mapping, and ALAC files made hostile by editing a tone's boxes:
//! the length and timing they state, and the packets they hold.

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
const TONE: &[u8] = include_bytes!("../fixtures/tone.m4a");

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

/// `bytes` with `table` in place of the first sample table box of its kind, and the boxes that
/// hold it, which come before it, grown or shrunk to match.
fn replaced(mut bytes: Vec<u8>, table: &[u8]) -> Vec<u8> {
  let kind: &[u8; 4] = table[4..8].try_into().unwrap();
  let at = box_at(&bytes, kind);
  let old = get(&bytes, at) as usize;
  bytes.splice(at..at + old, table.iter().copied());
  for kind in [b"moov", b"trak", b"mdia", b"minf", b"stbl"] {
    // a track taken out of the file has no movie box around it
    let Some(name) = bytes.windows(4).position(|w| w == kind) else {
      continue;
    };
    let holder = name - 4;
    let grown = get(&bytes, holder) as usize + table.len() - old;
    put(&mut bytes, holder, u32::try_from(grown).unwrap());
  }
  bytes
}

/// The tone with `entries` as its time-to-sample table, as sample counts and the frames each
/// sample lasts.
fn timed(entries: &[(u32, u32)]) -> Vec<u8> {
  let mut words = vec![0, u32::try_from(entries.len()).unwrap()];
  words.extend(entries.iter().flat_map(|&(count, delta)| [count, delta]));
  replaced(TONE.to_vec(), &boxed(b"stts", &words, &[]))
}

#[test]
fn a_length_past_what_a_wav_holds_is_refused_however_far() {
  // the first entry sums to about 2^62 frames
  let bytes = timed(&[(0x4000_0001, 0xffff_ffff), (1, 3712)]);
  let refused = wav_length(&bytes).unwrap_err();
  assert!(refused.contains("too long"), "{refused}");
}

#[test]
fn alac_whose_first_packet_cannot_be_decoded_is_refused() {
  // the magic cookie, after the 12 bytes that start the inner `alac` box, opens with the
  // frames a packet holds
  let mut bytes = TONE.to_vec();
  let inner = bytes
    .windows(8)
    .position(|w| w == [0, 0, 0, 0x24, b'a', b'l', b'a', b'c'])
    .unwrap();
  put(&mut bytes, inner + 12, 0);
  let refused = wav_length(&bytes).unwrap_err();
  assert!(
    refused.starts_with("no packet of the audio could be decoded: "),
    "{refused}"
  );
}

/// The whole WAV of `bytes`.
fn whole(bytes: &[u8]) -> Result<Vec<u8>, String> {
  wav_bytes(bytes, 0, wav_length(bytes)?)
}

/// The tone made fragmented by a movie extends box at the end of its movie box, the file's last,
/// and stating 30000 frames. After it come fragments of the first packet again, each with its
/// media data: `fragments` has one entry per fragment, holding the track fragment boxes it has
/// before the packet's, or none.
fn fragmented(fragments: &[&[u8]]) -> Vec<u8> {
  let mut bytes = TONE.to_vec();
  let mdhd = box_at(&bytes, b"mdhd");
  put(&mut bytes, mdhd + 24, 30_000);
  let trex = boxed(b"trex", &[0, 1, 1, 0, 0, 0], &[]);
  let mvex = boxed(b"mvex", &[], &trex);
  let moov = box_at(&bytes, b"moov");
  let grown = get(&bytes, moov) + u32::try_from(mvex.len()).unwrap();
  put(&mut bytes, moov, grown);
  bytes.extend_from_slice(&mvex);
  // the reader reads a packet only while it is at a media data box, so one follows the movie box
  bytes.extend_from_slice(&boxed(b"mdat", &[], &[]));
  let first_at = get(&bytes, box_at(&bytes, b"stco") + 16) as usize;
  let first_size = get(&bytes, box_at(&bytes, b"stsz") + 20);
  let packet = bytes[first_at..][..first_size as usize].to_vec();
  for (sequence, before) in (1..).zip(fragments) {
    let fragment = |base: usize| {
      let tfhd = boxed(b"tfhd", &[1, 1, 0, u32::try_from(base).unwrap()], &[]);
      let trun = boxed(b"trun", &[0x301, 1, 0, 4096, first_size], &[]);
      let traf = boxed(b"traf", &[], &[tfhd, trun].concat());
      let mfhd = boxed(b"mfhd", &[0, sequence], &[]);
      boxed(b"moof", &[], &[mfhd, before.to_vec(), traf].concat())
    };
    let base = bytes.len() + fragment(0).len() + 8;
    bytes.extend_from_slice(&fragment(base));
    bytes.extend_from_slice(&boxed(b"mdat", &[], &packet));
  }
  bytes
}

#[test]
fn a_packet_timed_past_what_a_position_holds_fails_the_stream() {
  // a run of 2^30 + 1 samples lasting 2^32 - 1 frames each, none of them read, which the reader
  // counts toward where the second fragment starts
  let unread = [
    boxed(b"tfhd", &[8, 1, 0xffff_ffff], &[]),
    boxed(b"trun", &[0, 0x4000_0001], &[]),
  ]
  .concat();
  let bytes = fragmented(&[&boxed(b"traf", &[], &unread), &[]]);
  assert_eq!(wav_length(&bytes), Ok(HEADER + 30_000 * 4));
  assert_eq!(
    whole(&bytes),
    Err("the file's timing is out of range".to_string())
  );
}

#[test]
fn a_range_past_the_last_packet_is_silence_as_in_the_whole() {
  // the movie box's four packets and the fragment's one hold 20096 of the 30000 frames stated
  let bytes = fragmented(&[&[]]);
  let whole = whole(&bytes).unwrap();
  assert!(whole[44 + 20_096 * 4..].iter().all(|b| *b == 0));
  let start = 44 + 25_000 * 4;
  assert_eq!(
    wav_bytes(&bytes, start, 1000),
    Ok(whole[start as usize..][..1000].to_vec())
  );
}

#[test]
fn a_range_past_where_another_track_ends_reads_the_audio() {
  // a second track holding only the first packet again, so it ends 4096 frames in, and a seek
  // that moves every track finds no place in it for a later frame
  let trak_at = box_at(TONE, b"trak");
  let trak = TONE[trak_at..][..get(TONE, trak_at) as usize].to_vec();
  let trak_end = trak_at + trak.len();
  let first_size = get(TONE, box_at(TONE, b"stsz") + 20);
  let second = replaced(trak, &boxed(b"stts", &[0, 1, 1, 4096], &[]));
  let mut second = replaced(second, &boxed(b"stsz", &[0, 0, 1, first_size], &[]));
  let tkhd = box_at(&second, b"tkhd");
  put(&mut second, tkhd + 20, 2);
  let mut bytes = TONE.to_vec();
  let moov = box_at(&bytes, b"moov");
  let grown = get(&bytes, moov) + u32::try_from(second.len()).unwrap();
  put(&mut bytes, moov, grown);
  bytes.splice(trak_end..trak_end, second);
  let audio = whole(&bytes).unwrap();
  assert_eq!(audio, whole(TONE).unwrap());
  let start = 44 + 12_000 * 4;
  assert_eq!(
    wav_bytes(&bytes, start, 1000),
    Ok(audio[start as usize..][..1000].to_vec())
  );
}

#[test]
fn a_packet_holding_more_than_it_lasts_is_cut_to_its_duration() {
  // every packet holds 4096 frames, but the table says the first three last 2048
  let bytes = timed(&[(3, 2048), (1, 3712)]);
  let cut = whole(&bytes).unwrap();
  assert_eq!(cut.len(), 44 + (3 * 2048 + 3712) * 4);
  let tone = whole(TONE).unwrap();
  let pcm = |wav: &[u8], from: usize, to: usize| wav[44 + from * 4..44 + to * 4].to_vec();
  for packet in 0..3 {
    let at = packet * 2048;
    assert_eq!(
      pcm(&cut, at, at + 2048),
      pcm(&tone, packet * 4096, packet * 4096 + 2048),
      "packet {packet}"
    );
  }
  assert_eq!(pcm(&cut, 6144, 9856), pcm(&tone, 12_288, 16_000));
}

#[test]
fn packets_shorter_than_their_time_leave_silence_and_ranges_match_the_whole() {
  // the first three packets last 8192 frames and hold 4096, the last lasts 5000 and holds 3712
  let bytes = timed(&[(3, 8192), (1, 5000)]);
  let frames = 3 * 8192 + 5000;
  let gapped = whole(&bytes).unwrap();
  assert_eq!(gapped.len(), 44 + frames * 4);
  let tone = whole(TONE).unwrap();
  let pcm = |wav: &[u8], from: usize, to: usize| wav[44 + from * 4..44 + to * 4].to_vec();
  for packet in 0..3 {
    let at = packet * 8192;
    assert_eq!(
      pcm(&gapped, at, at + 4096),
      pcm(&tone, packet * 4096, (packet + 1) * 4096),
      "packet {packet}"
    );
    assert!(pcm(&gapped, at + 4096, at + 8192).iter().all(|b| *b == 0));
  }
  assert_eq!(pcm(&gapped, 24_576, 28_288), pcm(&tone, 12_288, 16_000));
  assert!(pcm(&gapped, 28_288, frames).iter().all(|b| *b == 0));
  for (start, length) in [
    (44 + 4096 * 4 + 2, 9),
    (44 + 8000 * 4, 2000),
    (44 + 28_000 * 4, 3000),
  ] {
    assert_eq!(
      wav_bytes(&bytes, start, length).unwrap(),
      gapped[start as usize..(start + length) as usize],
      "{start}+{length}"
    );
  }
}

#[test]
fn a_range_past_a_packet_holding_more_than_it_lasts_matches_the_whole() {
  // the first three packets last 256 frames and the first holds 4096; the other three are cut
  // to a byte, which does not decode, so nothing after the first packet is audio
  let bytes = timed(&[(3, 256), (1, 3712)]);
  let first = get(TONE, box_at(TONE, b"stsz") + 20);
  let bytes = replaced(bytes, &boxed(b"stsz", &[0, 0, 4, first, 1, 1, 1], &[]));
  let whole = whole(&bytes).unwrap();
  assert_eq!(whole.len(), 44 + (3 * 256 + 3712) * 4);
  // a seek to a frame past the first 256 lands on a later packet
  for start in [44 + 256 * 4, 44 + 1486 * 4, 44 + 4000 * 4] {
    assert_eq!(
      wav_bytes(&bytes, start, 1000),
      Ok(whole[start as usize..][..1000].to_vec()),
      "{start}"
    );
  }
}
