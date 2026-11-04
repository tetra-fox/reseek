//! What we share, as the daemon serves it: the share engine with the local index and the
//! indexes relay agents upload, and the API over it.

mod service;

pub use reseek_shares::{
  FolderError, HostError, LOCAL_HOST, Resolved, ScanConflict, SharedCounts, Shares,
};

use crate::config::Options;
use crate::config::enums::RelayMode;
use crate::paths::Paths;
use crate::state::StateOwner;

/// How the daemon opens its share engine with the options it started with: the index in the
/// cache directory, the share state mirrored into `State.shares`.
pub fn setup(options: &Options, paths: &Paths, state: &StateOwner) -> reseek_shares::Setup {
  let state = state.clone();
  reseek_shares::Setup {
    cache: paths.cache.clone(),
    backups: paths.backups.clone(),
    downloads: paths.downloads.clone(),
    scan_at_startup: !options.flags.no_share_scan,
    force_scan: options.flags.force_share_scan,
    volatile: options.flags.volatile,
    case_sensitive: options.flags.case_sensitive_regex,
    relay_debug: options.relay.enabled && options.relay.mode == RelayMode::Debug,
    publish: Box::new(move |shares| state.update(|current| current.shares = shares)),
  }
}
