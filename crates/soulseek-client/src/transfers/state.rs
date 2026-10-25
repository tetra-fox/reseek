//! Where a transfer is in its life, as a set of flags. Only a few combinations ever occur:
//! `Queued, Locally`, `Requested`, `Queued, Remotely`, `Initializing`, `InProgress`, and
//! `Completed` with exactly one outcome. They are written as their flag names joined with ", ",
//! the strings the API and the UI read.

use std::fmt;
use std::str::FromStr;

use crate::state::{parse_flags, write_flags};

bitflags::bitflags! {
  /// Where a transfer is in its life
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct TransferState: u32 {
    /// the request was written to the peer; for an upload, its answer is awaited
    const REQUESTED = 1;
    /// waiting, here or at the peer, as `LOCALLY` or `REMOTELY` says
    const QUEUED = 2;
    /// accepted; the transfer connection is being made or the start offset exchanged
    const INITIALIZING = 4;
    /// bytes are moving
    const IN_PROGRESS = 8;
    /// over, with one of the outcomes below
    const COMPLETED = 16;
    /// every byte moved
    const SUCCEEDED = 32;
    /// the caller cancelled
    const CANCELLED = 64;
    /// a wait, a connect or the transfer connection's inactivity timed out
    const TIMED_OUT = 128;
    /// any other failure
    const ERRORED = 256;
    /// the peer refused the transfer
    const REJECTED = 512;
    /// a download whose size the peer reported differently, and nothing settled which is right
    const ABORTED = 1024;
    /// with `QUEUED`: waiting for a local limit or upload slot
    const LOCALLY = 2048;
    /// with `QUEUED`: the peer has the request in its queue
    const REMOTELY = 4096;
  }
}

const NAMES: [(TransferState, &str); 13] = [
  (TransferState::REQUESTED, "Requested"),
  (TransferState::QUEUED, "Queued"),
  (TransferState::INITIALIZING, "Initializing"),
  (TransferState::IN_PROGRESS, "InProgress"),
  (TransferState::COMPLETED, "Completed"),
  (TransferState::SUCCEEDED, "Succeeded"),
  (TransferState::CANCELLED, "Cancelled"),
  (TransferState::TIMED_OUT, "TimedOut"),
  (TransferState::ERRORED, "Errored"),
  (TransferState::REJECTED, "Rejected"),
  (TransferState::ABORTED, "Aborted"),
  (TransferState::LOCALLY, "Locally"),
  (TransferState::REMOTELY, "Remotely"),
];

impl TransferState {
  /// waiting for a local limit or slot
  pub const QUEUED_LOCALLY: Self = Self::QUEUED.union(Self::LOCALLY);
  /// in the peer's queue
  pub const QUEUED_REMOTELY: Self = Self::QUEUED.union(Self::REMOTELY);

  /// whether the transfer is over
  pub fn is_terminal(self) -> bool {
    self.contains(Self::COMPLETED)
  }

  /// whether the transfer is over and every byte moved
  pub fn succeeded(self) -> bool {
    self.contains(Self::COMPLETED | Self::SUCCEEDED)
  }
}

/// the flag names joined with ", ", as in "Completed, Succeeded"; "None" for no flag
impl fmt::Display for TransferState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write_flags(f, *self, &NAMES)
  }
}

/// reads the strings `Display` writes
impl FromStr for TransferState {
  type Err = String;

  fn from_str(text: &str) -> Result<Self, Self::Err> {
    parse_flags(text, &NAMES).ok_or_else(|| format!("not a transfer state: {text}"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn transfer_states_are_written_as_their_flag_names_and_read_back() {
    let states = [
      (TransferState::QUEUED_LOCALLY, "Queued, Locally"),
      (TransferState::REQUESTED, "Requested"),
      (TransferState::QUEUED_REMOTELY, "Queued, Remotely"),
      (TransferState::INITIALIZING, "Initializing"),
      (TransferState::IN_PROGRESS, "InProgress"),
      (
        TransferState::COMPLETED | TransferState::SUCCEEDED,
        "Completed, Succeeded",
      ),
      (
        TransferState::COMPLETED | TransferState::TIMED_OUT,
        "Completed, TimedOut",
      ),
      (
        TransferState::COMPLETED | TransferState::ABORTED,
        "Completed, Aborted",
      ),
      (TransferState::empty(), "None"),
    ];
    for (state, text) in states {
      assert_eq!(state.to_string(), text);
      assert_eq!(text.parse::<TransferState>(), Ok(state));
    }
    assert!("Completed, Exploded".parse::<TransferState>().is_err());
  }
}
