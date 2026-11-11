//! Whether other users can connect to us, judged from what happens rather than asked of anyone:
//! a peer that connects to our listener shows they can, whether it came by itself or to answer
//! a connection request we had the server relay (nothing else gets a peer to our port); ten
//! minutes logged in, or five of our searches drawing responses, with every peer connection one
//! we opened ourselves, shows they cannot. The judgment starts over at every login and whenever
//! the listening port or its mapping on the router changes; it is
//! `State.server.listen.reachable`.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use api_types::state::Reachable;
use reseek_shares::lock;
use tokio::sync::{Notify, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::state::StateOwner;

/// How long we stay logged in before nobody connecting to us says they cannot.
pub const WAITED: Duration = Duration::from_secs(600);

/// How many of our searches draw responses before nobody connecting to us says they cannot.
pub const SEARCHES: usize = 5;

/// What the judgment is made from, since it last started over.
struct Evidence {
  logged_in: bool,
  since: Instant,
  /// the connections to our listener counted when it started over
  base: u64,
  /// the tokens of our searches that took a response
  answered: HashSet<u32>,
}

/// The judgment of whether other users can connect to us.
pub struct Reachability {
  state: StateOwner,
  inbound: watch::Receiver<u64>,
  evidence: Mutex<Evidence>,
  changed: Notify,
}

/// What `reached` connections to our listener, `searches` answered and whether we `waited`
/// long enough say.
pub fn judge(reached: u64, searches: usize, waited: bool) -> Reachable {
  if reached > 0 {
    Reachable::Yes
  } else if waited || searches >= SEARCHES {
    Reachable::No
  } else {
    Reachable::Unknown
  }
}

impl Reachability {
  /// A judgment made from the connections `inbound` counts, written into `state`; it is
  /// "unknown" until we log in.
  pub fn new(state: StateOwner, inbound: watch::Receiver<u64>) -> Self {
    let base = *inbound.borrow();
    Self {
      state,
      inbound,
      evidence: Mutex::new(Evidence {
        logged_in: false,
        since: Instant::now(),
        base,
        answered: HashSet::new(),
      }),
      changed: Notify::new(),
    }
  }

  fn evidence(&self) -> std::sync::MutexGuard<'_, Evidence> {
    lock(&self.evidence)
  }

  /// We logged in: the judgment starts over.
  pub fn logged_in(&self) {
    self.evidence().logged_in = true;
    self.start_over("logged in");
  }

  /// The session ended: nothing is known until the next login.
  pub fn logged_out(&self) {
    self.evidence().logged_in = false;
    self.changed.notify_one();
  }

  /// What other users could reach changed, for `why`: the judgment starts over.
  pub fn start_over(&self, why: &str) {
    let mut evidence = self.evidence();
    evidence.since = Instant::now();
    evidence.base = *self.inbound.borrow();
    evidence.answered.clear();
    drop(evidence);
    debug!(why, "reachability judged anew");
    self.changed.notify_one();
  }

  /// Our search `token` took a response.
  pub fn search_answered(&self, token: u32) {
    if self.evidence().answered.insert(token) {
      self.changed.notify_one();
    }
  }

  /// The judgment now, and when it next changes by time alone.
  fn now(&self) -> (Reachable, Option<Instant>) {
    let evidence = self.evidence();
    if !evidence.logged_in {
      return (Reachable::Unknown, None);
    }
    let reached = self.inbound.borrow().saturating_sub(evidence.base);
    let deadline = evidence.since + WAITED;
    let waited = Instant::now() >= deadline;
    let judged = judge(reached, evidence.answered.len(), waited);
    (judged, (!waited).then_some(deadline))
  }

  /// Keeps `State.server.listen.reachable` current until `shutdown`.
  pub async fn follow(self: std::sync::Arc<Self>, shutdown: CancellationToken) {
    let mut inbound = self.inbound.clone();
    loop {
      let (judged, deadline) = self.now();
      self.publish(judged);
      tokio::select! {
        () = shutdown.cancelled() => return,
        () = self.changed.notified() => {}
        changed = inbound.changed() => if changed.is_err() { return },
        () = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {}
      }
    }
  }

  fn publish(&self, judged: Reachable) {
    let current = self.state.current();
    let listen = &current.server.listen;
    if listen.reachable == judged {
      return;
    }
    match judged {
      Reachable::Yes => info!("Other users can connect to us"),
      Reachable::No => warn!(
        port = listen.port,
        "Other users cannot connect to us; forward the port to this computer or turn on port \
         mapping"
      ),
      Reachable::Unknown => debug!("whether other users can connect to us is not known yet"),
    }
    self
      .state
      .update(|state| state.server.listen.reachable = judged);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Lets the judgment's task run until it waits again.
  async fn settle() {
    for _ in 0..10 {
      tokio::task::yield_now().await;
    }
  }

  #[tokio::test(start_paused = true)]
  async fn ten_minutes_with_nobody_reaching_us_say_no_until_we_log_out() {
    let state = crate::testing::state();
    let (_inbound, counted) = watch::channel(0);
    let reachability = std::sync::Arc::new(Reachability::new(state.clone(), counted));
    tokio::spawn(reachability.clone().follow(CancellationToken::new()));
    let reachable = || state.current().server.listen.reachable;
    reachability.logged_in();
    tokio::time::sleep(WAITED - Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(reachable(), Reachable::Unknown);
    tokio::time::sleep(Duration::from_secs(2)).await;
    settle().await;
    assert_eq!(reachable(), Reachable::No);
    reachability.logged_out();
    settle().await;
    assert_eq!(reachable(), Reachable::Unknown);
  }
}
