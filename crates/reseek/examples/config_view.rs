//! Prints the settings page's view of the default configuration, read from an empty
//! `config.toml` with no environment and no flags, which `just config-fixture` writes to
//! `web/src/lib/config/fixtures/view.json` for the UI's tests.

use std::path::Path;

use cascade::Watcher;
use reseek::config::load::{self, ConfigFiles, Sources};
use reseek::config::view;
use reseek::paths::Defaults;

/// The directory the files are said to be in, so the output is the same on every machine.
const SHOWN_DIR: &str = "/var/lib/reseek";

/// The revision shown, since a real one is new each time the configuration loads.
const SHOWN_REVISION: &str = "fixture";

fn main() {
  let dir = tempfile::tempdir().expect("a temporary directory");
  std::fs::write(dir.path().join("config.toml"), "").expect("an empty config.toml");
  let sources = Sources {
    env: Vec::new(),
    files: ConfigFiles::new(&[], dir.path()).unwrap(),
    cli: Vec::new(),
  };
  let layers = load::layers(&sources, &Defaults::new(Some(dir.path())), None);
  let watcher = Watcher::new(layers).expect("the default configuration loads");
  let mut view = view::config_view(&watcher.current());
  view.revision = SHOWN_REVISION.to_string();
  for file in &mut view.files {
    let name = Path::new(&file.path)
      .strip_prefix(dir.path())
      .expect("every file is in the directory");
    file.path = Path::new(SHOWN_DIR).join(name).display().to_string();
  }
  let text = serde_json::to_string_pretty(&view).expect("the view serializes");
  println!("{text}");
}
