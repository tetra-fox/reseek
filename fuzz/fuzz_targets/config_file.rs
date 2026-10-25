#![no_main]

use cascade::Layers;
use libfuzzer_sys::fuzz_target;
use reseek_fuzz::config;

// any text as the configuration file: it loads or says why, it never panics
fuzz_target!(|data: &[u8]| {
  let Ok(text) = std::str::from_utf8(data) else {
    return;
  };
  let layers = Layers::<config::Options>::new().files("/nonexistent", ["config.toml"]);
  // any result is fine, this only checks it does not panic
  let _ = layers.check_file("config.toml", text);
});
