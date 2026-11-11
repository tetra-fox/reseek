//! Where a transfer is in its life and the moves it may make. A transfer starts queued here and
//! only moves forward: a download asks the peer and may wait in its queue, an upload offers the
//! file, then the transfer connection is made, bytes move, and it completes with one outcome.
//! Any state but a completed one may complete. The states are written as the strings the API
//! and the UI read, such as "Queued, Locally" and "Completed, Succeeded".

use std::fmt;
use std::str::FromStr;

/// Where a transfer is in its life
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferState {
  /// waiting here for a local limit or upload slot, or for its task to start
  QueuedLocally,
  /// the request was written to the peer; for an upload, its answer is awaited
  Requested,
  /// the peer has the request in its queue
  QueuedRemotely,
  /// accepted; the transfer connection is being made or the start offset exchanged
  Initializing,
  /// bytes are moving
  InProgress,
  /// over, with how it ended
  Completed(TransferOutcome),
}

/// How a completed transfer ended
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferOutcome {
  /// every byte moved
  Succeeded,
  /// the caller canceled
  Cancelled,
  /// a wait, a connect or the transfer connection's inactivity timed out
  TimedOut,
  /// any other failure
  Errored,
  /// the peer refused the transfer
  Rejected,
  /// a download whose size the peer reported differently, and nothing settled which is right
  Aborted,
}

/// A move the state machine does not allow
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refused {
  from: TransferState,
  to: TransferState,
}

impl fmt::Display for Refused {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "a transfer cannot move from {} to {}",
      self.from, self.to
    )
  }
}

impl TransferState {
  /// every state, in the order a transfer can go through them
  const ALL: [Self; 11] = [
    Self::QueuedLocally,
    Self::Requested,
    Self::QueuedRemotely,
    Self::Initializing,
    Self::InProgress,
    Self::Completed(TransferOutcome::Succeeded),
    Self::Completed(TransferOutcome::Cancelled),
    Self::Completed(TransferOutcome::TimedOut),
    Self::Completed(TransferOutcome::Errored),
    Self::Completed(TransferOutcome::Rejected),
    Self::Completed(TransferOutcome::Aborted),
  ];

  /// whether the transfer is over
  pub fn is_terminal(self) -> bool {
    matches!(self, Self::Completed(_))
  }

  /// whether the transfer is over and every byte moved
  pub fn succeeded(self) -> bool {
    self == Self::Completed(TransferOutcome::Succeeded)
  }

  /// Whether a transfer in this state may move to `next`. Requested goes on to Initializing
  /// for an upload, and for a download its peer queued in passing.
  fn allows(self, next: Self) -> bool {
    use TransferState::*;
    match (self, next) {
      (Completed(_), _) => false,
      (_, Completed(_)) => true,
      (QueuedLocally, Requested | Initializing)
      | (Requested, QueuedRemotely | Initializing)
      | (QueuedRemotely, Initializing)
      | (Initializing, InProgress) => true,
      _ => false,
    }
  }

  /// `next`, when a transfer in this state may move to it
  pub(crate) fn to(self, next: Self) -> Result<Self, Refused> {
    if self.allows(next) {
      Ok(next)
    } else {
      Err(Refused {
        from: self,
        to: next,
      })
    }
  }
}

impl fmt::Display for TransferState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Self::QueuedLocally => "Queued, Locally",
      Self::Requested => "Requested",
      Self::QueuedRemotely => "Queued, Remotely",
      Self::Initializing => "Initializing",
      Self::InProgress => "InProgress",
      Self::Completed(TransferOutcome::Succeeded) => "Completed, Succeeded",
      Self::Completed(TransferOutcome::Cancelled) => "Completed, Cancelled",
      Self::Completed(TransferOutcome::TimedOut) => "Completed, TimedOut",
      Self::Completed(TransferOutcome::Errored) => "Completed, Errored",
      Self::Completed(TransferOutcome::Rejected) => "Completed, Rejected",
      Self::Completed(TransferOutcome::Aborted) => "Completed, Aborted",
    })
  }
}

/// reads the strings `Display` writes
impl FromStr for TransferState {
  type Err = String;

  fn from_str(text: &str) -> Result<Self, Self::Err> {
    Self::ALL
      .into_iter()
      .find(|state| state.to_string() == text)
      .ok_or_else(|| format!("not a transfer state: {text}"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use TransferOutcome::*;
  use TransferState::*;

  #[test]
  fn transfer_states_are_written_as_their_names_and_read_back() {
    let names = [
      "Queued, Locally",
      "Requested",
      "Queued, Remotely",
      "Initializing",
      "InProgress",
      "Completed, Succeeded",
      "Completed, Cancelled",
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted",
    ];
    for (state, name) in TransferState::ALL.into_iter().zip(names) {
      assert_eq!(state.to_string(), name);
      assert_eq!(name.parse::<TransferState>(), Ok(state));
    }
    for text in ["None", "Queued", "Completed", "Completed, Exploded"] {
      assert!(text.parse::<TransferState>().is_err(), "{text}");
    }
  }

  #[test]
  fn a_transfer_moves_only_forward_and_never_out_of_completed() {
    let forward = [
      (QueuedLocally, Requested),
      (QueuedLocally, Initializing),
      (Requested, QueuedRemotely),
      (Requested, Initializing),
      (QueuedRemotely, Initializing),
      (Initializing, InProgress),
    ];
    let open = [
      QueuedLocally,
      Requested,
      QueuedRemotely,
      Initializing,
      InProgress,
    ];
    for from in TransferState::ALL {
      for to in TransferState::ALL {
        let ends = open.contains(&from) && matches!(to, Completed(_));
        let allowed = forward.contains(&(from, to)) || ends;
        assert_eq!(from.to(to).is_ok(), allowed, "{from} to {to}");
      }
    }
    assert_eq!(
      Completed(Succeeded)
        .to(Completed(Cancelled))
        .unwrap_err()
        .to_string(),
      "a transfer cannot move from Completed, Succeeded to Completed, Cancelled"
    );
  }
}
