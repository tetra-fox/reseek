#![no_main]

use libfuzzer_sys::fuzz_target;
use reseek::files::{wav_bytes, wav_length};

/// The longest WAV read whole to compare its ranges with; a longer one is only read in ranges.
const WHOLE: u64 = 1 << 20;
/// The bytes of each range read.
const RANGE: u64 = 5000;

// a file is whatever a peer sent; the WAV made of it is refused with a reason, or reads the same
// whole as in ranges, and a range of a WAV read whole cannot fail
fuzz_target!(|data: &[u8]| {
  let Ok(size) = wav_length(data) else {
    return;
  };
  let whole = (size <= WHOLE)
    .then(|| wav_bytes(data, 0, size))
    .and_then(Result::ok);
  if let Some(whole) = &whole {
    assert_eq!(whole.len() as u64, size);
  }
  // from the start, across the header's end, inside a sample and up to the end
  for start in [0, 40, size / 3, size / 2 + 1, size.saturating_sub(RANGE)] {
    let start = start.min(size - 1);
    let length = RANGE.min(size - start);
    let part = wav_bytes(data, start, length);
    match &whole {
      Some(whole) => assert_eq!(
        part.as_deref(),
        Ok(&whole[start as usize..(start + length) as usize])
      ),
      None => {
        if let Ok(part) = part {
          assert_eq!(part.len() as u64, length);
        }
      }
    }
  }
});
