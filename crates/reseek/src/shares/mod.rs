//! What we share, as the daemon serves it: the share engine with the local index and the
//! indexes relay agents upload, and the API over it.

mod service;

pub use reseek_shares::{
  FolderError, HostError, LOCAL_HOST, Resolved, ScanConflict, ScanOrigin, SharedCounts, Shares,
  View,
};

use std::sync::Arc;

use crate::config::Options;
use crate::jobs::Jobs;
use crate::paths::Paths;
use crate::state::StateOwner;

/// How the daemon opens its share engine with the options it started with: the index in the
/// cache directory, the share state mirrored into `State.shares`, each scan kept as a job in
/// `jobs`, whose scheduler runs the rescans.
pub fn setup(
  options: &Options,
  paths: &Paths,
  state: &StateOwner,
  jobs: &Arc<Jobs>,
) -> reseek_shares::Setup {
  let state = state.clone();
  reseek_shares::Setup {
    cache: paths.cache.clone(),
    backups: paths.backups.clone(),
    downloads: paths.downloads.clone(),
    scan_at_startup: !options.flags.no_share_scan,
    force_scan: options.flags.force_share_scan,
    volatile: options.flags.volatile,
    case_sensitive: options.flags.case_sensitive_regex,
    rescan_on_schedule: false,
    scans: crate::jobs::scans::hook(jobs.clone()),
    publish: Box::new(move |shares| state.update(|current| current.shares = shares)),
  }
}
