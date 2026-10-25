//! The running version.

use api_types::state::VersionInfo;

/// The revision the build was made from, when the build says.
const REVISION: Option<&str> = option_env!("RESEEK_GIT_REVISION");

/// The running version: `full` is `<semver> (<semver>+<revision>)`, the revision left out when
/// the build does not name one. A debug build is a development build.
pub fn info() -> VersionInfo {
  let current = env!("CARGO_PKG_VERSION").to_string();
  let informational = match REVISION {
    Some(revision) => format!("{current}+{revision}"),
    None => current.clone(),
  };
  VersionInfo {
    full: format!("{current} ({informational})"),
    current,
    latest: None,
    is_update_available: None,
    is_canary: false,
    is_development: cfg!(debug_assertions),
  }
}
