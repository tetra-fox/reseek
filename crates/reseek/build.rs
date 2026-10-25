//! Points the embedded web UI at `WEBUI_DIR`, or at `web/build` in the repository when it is
//! not set. Only what is already built is embedded; this never builds the UI.

use std::path::Path;

fn main() {
  println!("cargo:rerun-if-env-changed=WEBUI_DIR");
  let dir = std::env::var("WEBUI_DIR").unwrap_or_else(|_| "../../web/build".to_string());
  // a path that does not exist would rerun this on every build, so its parent is watched
  // until the UI is first built
  let dir_path = Path::new(&dir);
  let watched = [Some(dir_path), dir_path.parent()]
    .into_iter()
    .flatten()
    .find(|p| p.exists() && !p.as_os_str().is_empty());
  if let Some(watched) = watched {
    println!("cargo:rerun-if-changed={}", watched.display());
  }
  println!("cargo:rustc-env=WEBUI_DIR={dir}");
}
