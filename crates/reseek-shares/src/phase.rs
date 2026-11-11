//! Where the share engine is: idle, loading the index at startup, scanning, or telling the scan
//! hook a scan ended. All but idle hold the one scan slot. [`Phase::after`] is the table of the
//! moves between phases, and the engine's transition is the only writer.

use api_types::state::SharesState;
use tokio_util::sync::CancellationToken;

use crate::scanner::Progress;
use crate::{ScanOrigin, SharedCounts};

/// How the last scan ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
  /// a scan filled the index
  Completed,
  /// a scan was canceled, and the index from before is served
  Canceled,
  /// a scan failed, and the index from before is served
  Failed,
}

/// The engine's phase. `served` is what the index serves outside a scan, the totals of the
/// last load or completed scan, `None` before the first.
#[derive(Clone, Debug)]
pub(crate) enum Phase {
  /// no load or scan runs; `ended` says how the last scan ended, `None` before the first
  Idle {
    served: Option<SharedCounts>,
    ended: Option<Ended>,
  },
  /// the startup load reads the index; `cancel` cancels the scan that follows it, which reads
  /// every file again when a scan asked for meanwhile was `force`d, and is started by `origin`,
  /// the startup or that ask
  Loading {
    cancel: CancellationToken,
    force: bool,
    origin: ScanOrigin,
  },
  /// a scan runs; a scan that does not complete leaves `before` served
  Scanning {
    cancel: CancellationToken,
    progress: Progress,
    before: Option<SharedCounts>,
  },
  /// a scan ended and the scan hook is being told, still holding the slot so no scan starts
  /// before the hook heard of the end
  Ending {
    served: Option<SharedCounts>,
    ended: Ended,
  },
}

/// A move from one phase to another.
#[derive(Clone, Debug)]
pub(crate) enum Edge {
  /// the startup load takes the slot, from idle
  Load(CancellationToken),
  /// a scan asked for while the index loads, by `origin`, is the scan that follows the load,
  /// forced when this one is
  Join { force: bool, origin: ScanOrigin },
  /// a scan takes the slot, from idle
  Scan(CancellationToken),
  /// the load ended, with the totals of the index it filled or `None` when it could not, and
  /// hands the slot to the scan that follows it
  Loaded(Option<SharedCounts>),
  /// the running scan got further
  Progress(Progress),
  /// the running scan ended; a completed one carries the totals of the index it filled
  ScanEnded(ScanResult),
  /// the scan hook heard of the end, and the slot is free
  Release,
  /// a watched change counted the index again, outside a load or a scan
  Counted(SharedCounts),
}

/// How a scan ended, as the phase takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanResult {
  Completed(SharedCounts),
  Canceled,
  Failed,
}

impl Default for Phase {
  fn default() -> Self {
    Self::Idle {
      served: None,
      ended: None,
    }
  }
}

impl Phase {
  /// The phase `edge` leads to from this one, or `None` when this phase does not take it.
  pub(crate) fn after(&self, edge: &Edge) -> Option<Self> {
    let next = match (self, edge) {
      (Self::Idle { .. }, Edge::Load(cancel)) => Self::Loading {
        cancel: cancel.clone(),
        force: false,
        origin: ScanOrigin::Startup,
      },
      (
        Self::Loading { cancel, force, .. },
        Edge::Join {
          force: asked,
          origin,
        },
      ) => Self::Loading {
        cancel: cancel.clone(),
        force: *force || *asked,
        origin: *origin,
      },
      (Self::Idle { served, .. }, Edge::Scan(cancel)) => Self::Scanning {
        cancel: cancel.clone(),
        progress: Progress::default(),
        before: *served,
      },
      (Self::Loading { cancel, .. }, Edge::Loaded(served)) => Self::Scanning {
        cancel: cancel.clone(),
        progress: Progress::default(),
        before: *served,
      },
      (Self::Scanning { cancel, before, .. }, Edge::Progress(progress)) => Self::Scanning {
        cancel: cancel.clone(),
        progress: *progress,
        before: *before,
      },
      (Self::Scanning { before, .. }, Edge::ScanEnded(result)) => match result {
        ScanResult::Completed(totals) => Self::Ending {
          served: Some(*totals),
          ended: Ended::Completed,
        },
        ScanResult::Canceled => Self::Ending {
          served: *before,
          ended: Ended::Canceled,
        },
        ScanResult::Failed => Self::Ending {
          served: *before,
          ended: Ended::Failed,
        },
      },
      (Self::Ending { served, ended }, Edge::Release) => Self::Idle {
        served: *served,
        ended: Some(*ended),
      },
      (Self::Idle { ended, .. }, Edge::Counted(totals)) => Self::Idle {
        served: Some(*totals),
        ended: *ended,
      },
      (Self::Ending { ended, .. }, Edge::Counted(totals)) => Self::Ending {
        served: Some(*totals),
        ended: *ended,
      },
      _ => return None,
    };
    Some(next)
  }

  /// whether a load or a scan holds the slot, up to the hook hearing of the scan's end
  pub(crate) fn holds_slot(&self) -> bool {
    !matches!(self, Self::Idle { .. })
  }

  /// the token that cancels the load's scan or the running scan
  pub(crate) fn cancel(&self) -> Option<&CancellationToken> {
    match self {
      Self::Loading { cancel, .. } | Self::Scanning { cancel, .. } => Some(cancel),
      Self::Idle { .. } | Self::Ending { .. } => None,
    }
  }

  /// whether the index was filled once, so the host list is known
  fn filled(&self) -> bool {
    match self {
      Self::Idle { served, .. } | Self::Ending { served, .. } => served.is_some(),
      Self::Scanning { before, .. } => before.is_some(),
      Self::Loading { .. } => false,
    }
  }

  /// The share state in this phase with `scan_pending`, the hosts of the relay agents `agents`
  /// and their totals `added`.
  pub(crate) fn to_api(
    &self,
    scan_pending: bool,
    agents: Vec<String>,
    added: SharedCounts,
  ) -> SharesState {
    let (scanning, served, ended, progress) = match self {
      Self::Idle { served, ended } => (false, *served, *ended, None),
      Self::Ending { served, ended } => (false, *served, Some(*ended), None),
      // the load is the start of the scan that follows it
      Self::Loading { .. } => (true, None, None, Some(Progress::default())),
      Self::Scanning { progress, .. } => (true, None, None, Some(*progress)),
    };
    let scan_progress = match (progress, ended) {
      (Some(progress), _) => progress.fraction,
      (None, Some(Ended::Completed | Ended::Canceled)) => 1.0,
      (None, Some(Ended::Failed) | None) => 0.0,
    };
    let (directories, files) = match (progress, served) {
      (Some(progress), _) => (progress.directories, progress.files),
      (None, Some(served)) => (served.directories, served.files),
      (None, None) => (0, 0),
    };
    let listed = self.filled() || !agents.is_empty();
    SharesState {
      scan_pending,
      scanning,
      ready: served.is_some(),
      faulted: ended == Some(Ended::Failed),
      cancelled: ended == Some(Ended::Canceled),
      scan_progress,
      hosts: listed.then(|| {
        std::iter::once(crate::LOCAL_HOST.to_string())
          .chain(agents)
          .collect()
      }),
      directories: directories.saturating_add(added.directories),
      files: files.saturating_add(added.files),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const TOTALS: SharedCounts = SharedCounts {
    directories: 2,
    files: 5,
  };

  fn name(phase: &Phase) -> &'static str {
    match phase {
      Phase::Idle { .. } => "Idle",
      Phase::Loading { .. } => "Loading",
      Phase::Scanning { .. } => "Scanning",
      Phase::Ending { .. } => "Ending",
    }
  }

  fn edges() -> Vec<(&'static str, Edge)> {
    let cancel = CancellationToken::new();
    vec![
      ("Load", Edge::Load(cancel.clone())),
      (
        "Join",
        Edge::Join {
          force: true,
          origin: ScanOrigin::Asked,
        },
      ),
      ("Scan", Edge::Scan(cancel)),
      ("Loaded", Edge::Loaded(Some(TOTALS))),
      ("Progress", Edge::Progress(Progress::default())),
      ("ScanEnded", Edge::ScanEnded(ScanResult::Failed)),
      ("Release", Edge::Release),
      ("Counted", Edge::Counted(TOTALS)),
    ]
  }

  #[test]
  fn each_phase_takes_only_its_edges() {
    let cancel = CancellationToken::new();
    let phases = [
      Phase::default(),
      Phase::Loading {
        cancel: cancel.clone(),
        force: false,
        origin: ScanOrigin::Startup,
      },
      Phase::Scanning {
        cancel,
        progress: Progress::default(),
        before: None,
      },
      Phase::Ending {
        served: None,
        ended: Ended::Failed,
      },
    ];
    let allowed = [
      vec!["Load", "Scan", "Counted"],
      vec!["Join", "Loaded"],
      vec!["Progress", "ScanEnded"],
      vec!["Release", "Counted"],
    ];
    for (phase, allowed) in phases.iter().zip(allowed) {
      for (edge, taken) in edges() {
        assert_eq!(
          phase.after(&taken).is_some(),
          allowed.contains(&edge),
          "{} with {edge}",
          name(phase)
        );
      }
    }
  }

  #[test]
  fn a_scan_that_does_not_complete_serves_what_was_there_before() {
    let loaded = Phase::Loading {
      cancel: CancellationToken::new(),
      force: false,
      origin: ScanOrigin::Startup,
    }
    .after(&Edge::Loaded(Some(TOTALS)))
    .unwrap();
    let progress = Progress {
      fraction: 0.5,
      directories: 9,
      files: 1,
    };
    let scanning = loaded.after(&Edge::Progress(progress)).unwrap();
    let shown = scanning.to_api(false, Vec::new(), SharedCounts::default());
    assert!(shown.scanning && !shown.ready);
    assert_eq!((shown.directories, shown.scan_progress), (9, 0.5));

    let failed = scanning
      .after(&Edge::ScanEnded(ScanResult::Failed))
      .unwrap();
    assert!(failed.holds_slot(), "the slot is held until the hook heard");
    let idle = failed.after(&Edge::Release).unwrap();
    let shown = idle.to_api(true, Vec::new(), SharedCounts::default());
    assert!(shown.ready && shown.faulted && !shown.cancelled && !shown.scanning);
    assert_eq!((shown.directories, shown.files), (2, 5));
    assert_eq!(shown.scan_progress, 0.0);

    let rescanned = idle.after(&Edge::Scan(CancellationToken::new())).unwrap();
    let shown = rescanned.to_api(false, Vec::new(), SharedCounts::default());
    assert!(!shown.faulted, "a new scan clears the last one's failure");
    let completed = rescanned
      .after(&Edge::ScanEnded(ScanResult::Completed(SharedCounts {
        directories: 3,
        files: 6,
      })))
      .unwrap()
      .after(&Edge::Release)
      .unwrap();
    let shown = completed.to_api(false, Vec::new(), SharedCounts::default());
    assert!(shown.ready && !shown.faulted);
    assert_eq!((shown.files, shown.scan_progress), (6, 1.0));
  }
}
