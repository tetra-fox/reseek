//! Our place in the distributed search tree as the server needs to hear it: branch level, branch
//! root, whether we take children and whether we need a parent, sent together, only when it
//! changed, coalesced over a few seconds, and again by force when the server must offer parents
//! anew.

use std::fmt;
use std::time::Duration;

use soulseek_proto::ServerRequest;
use soulseek_proto::server::{AcceptChildren, BranchLevel, BranchRoot, HaveNoParents};
use tokio::time::Instant;

use super::tree::Tree;

/// how long changes are coalesced before the server is told
const DEBOUNCE: Duration = Duration::from_secs(5);
/// a status older than this is sent again by force when another change comes
const STATUS_MAX_AGE: Duration = Duration::from_secs(300);
/// how often a client with neither parent nor branch asks the server for candidates again
pub(super) const WATCHDOG: Duration = Duration::from_secs(900);

/// The values the server keeps about our place in the tree
pub(super) struct DistributedStatus {
  pub(super) have_no_parents: bool,
  pub(super) branch_level: u32,
  pub(super) branch_root: String,
  pub(super) children: usize,
  pub(super) child_limit: u32,
  pub(super) accept_children: bool,
}

/// the line that is logged and compared to decide whether the server needs telling again
impl fmt::Display for DistributedStatus {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "Requesting parent: {}, Branch level: {}, Branch root: {}, Number of children: {}/{}, Accepting children: {}",
      self.have_no_parents,
      self.branch_level,
      self.branch_root,
      self.children,
      self.child_limit,
      self.accept_children
    )
  }
}

impl DistributedStatus {
  /// the four messages, written as one
  pub(super) fn messages(&self) -> [ServerRequest; 4] {
    [
      ServerRequest::BranchLevel(BranchLevel {
        level: self.branch_level,
      }),
      ServerRequest::BranchRoot(BranchRoot {
        root: self.branch_root.clone(),
      }),
      ServerRequest::AcceptChildren(AcceptChildren {
        accept: self.accept_children,
      }),
      ServerRequest::HaveNoParents(HaveNoParents {
        have_no_parents: self.have_no_parents,
      }),
    ]
  }
}

impl Tree {
  /// Tells the server our place in the tree, unless it has heard exactly this already and
  /// `force` is not set. A failed write leaves the last status as it was, so the next update
  /// tries again.
  pub(super) async fn update_status(&mut self, force: bool) {
    let Some(client) = self.client() else {
      return;
    };
    if !client.state().is_logged_in() {
      return;
    }
    let options = client.options();
    let status = DistributedStatus {
      have_no_parents: options.enable_distributed_network && !self.has_parent(),
      branch_level: self.branch_level(),
      branch_root: self.branch_root(&client),
      children: self.children.len(),
      child_limit: self.child_limit(&client),
      accept_children: self.can_accept_children(&client),
    };
    let text = status.to_string();
    if !force
      && self
        .last_status
        .as_deref()
        .is_some_and(|last| last.eq_ignore_ascii_case(&text))
    {
      tracing::debug!(status = text, "Update skipped; status has not changed");
      return;
    }
    match client.write_server_messages(&status.messages()).await {
      Ok(()) => {
        tracing::info!(status = text, "Updated distributed status");
        self.last_status = Some(text);
        self.last_status_at = Some(Instant::now());
        self.publish(true);
      }
      Err(error) if client.state().is_logged_in() => {
        tracing::warn!(%error, "Failed to update distributed status");
      }
      Err(error) => tracing::debug!(%error, "Failed to update distributed status"),
    }
  }

  /// Coalesces status updates: the server is told five seconds after the last of a burst of
  /// changes, and at once by force when the status it has is older than five minutes.
  pub(super) async fn update_status_eventually(&mut self) {
    let now = Instant::now();
    let stale = self.debounce.is_some()
      && self
        .last_status_at
        .is_some_and(|at| at + STATUS_MAX_AGE <= now);
    self.debounce = Some(now + DEBOUNCE);
    if stale {
      self.update_status(true).await;
    }
  }

  pub(super) fn reset_status(&mut self) {
    self.last_status = None;
    self.last_status_at = None;
    self.demote();
  }

  pub(super) async fn watchdog(&mut self) {
    let Some(client) = self.client() else {
      return;
    };
    if client.options().enable_distributed_network
      && !self.has_parent()
      && !self.is_branch_root
      && client.state().is_logged_in()
    {
      tracing::warn!("No distributed parent connected.  Requesting a list of candidates.");
      self.update_status(true).await;
    }
  }
}
