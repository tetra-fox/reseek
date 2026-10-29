#![no_main]

use libfuzzer_sys::fuzz_target;

// a downloaded file is whatever a peer sent; the analyzer must read any bytes to a verdict
fuzz_target!(|data: &[u8]| {
  reseek::analysis::verdict_of(data.to_vec());
});
