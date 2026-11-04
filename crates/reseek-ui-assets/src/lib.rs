//! The web UI build, embedded in the program. A crate of its own so that building the UI again
//! rebuilds only this and the program, not the daemon library.

use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "$WEBUI_DIR"]
#[allow_missing = true]
struct Assets;

/// The file at `path` in the UI build, `None` when it has no such file or the UI was not built
pub fn get(path: &str) -> Option<Cow<'static, [u8]>> {
  Assets::get(path).map(|file| file.data)
}
