//! The parts of the application state the daemon builds itself, ready for the state document.

use api_types::state::ConfigurationState;
use cascade::Loaded;

use crate::config::Options;

/// The outcome of the last configuration reload: why it failed and when, both null once a
/// reload succeeds.
pub fn configuration(loaded: &Loaded<Options>) -> ConfigurationState {
  ConfigurationState {
    last_reload_error: loaded.load_error.clone(),
    last_reload_at: loaded.load_error_at,
  }
}
