//! Our place in the distributed search tree as the server needs to hear it: branch level, branch
//! root, whether we take children and whether we need a parent, sent together and only when they
//! change. Until a parent is adopted the client is at level 0 under its own name.

use std::fmt;

use soulseek_proto::ServerRequest;
use soulseek_proto::server::{AcceptChildren, BranchLevel, BranchRoot, HaveNoParents};

use super::Client;
use crate::state::ClientState;

/// The values the server keeps about our place in the tree
struct DistributedStatus {
  have_no_parents: bool,
  branch_level: u32,
  branch_root: String,
  children: usize,
  child_limit: u32,
  accept_children: bool,
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
  fn messages(&self) -> [ServerRequest; 4] {
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

impl Client {
  /// Sends our distributed status in one write when it differs from the last one sent. A
  /// failure is logged and leaves the last status as it was, so the next update tries again.
  pub(super) async fn update_distributed_status(&self) {
    let mut last = self.inner.distributed_status.lock().await;
    if !self.state().contains(ClientState::READY) {
      return;
    }
    // logged in means the session holds our username; a disconnect racing this clears it
    let Some(username) = self.username() else {
      return;
    };
    let options = self.inner.options();
    // children are taken only below a parent or as a branch root, and we are neither yet
    let status = DistributedStatus {
      have_no_parents: options.enable_distributed_network,
      branch_level: 0,
      branch_root: username,
      children: 0,
      child_limit: options.distributed_child_limit,
      accept_children: false,
    };
    let text = status.to_string();
    if last
      .as_deref()
      .is_some_and(|last| last.eq_ignore_ascii_case(&text))
    {
      tracing::debug!(status = text, "Update skipped; status has not changed");
      return;
    }
    match self.write_server_messages(&status.messages()).await {
      Ok(()) => {
        tracing::info!(status = text, "Updated distributed status");
        *last = Some(text);
      }
      Err(error) if self.state().contains(ClientState::READY) => {
        tracing::warn!(%error, "Failed to update distributed status");
      }
      Err(error) => tracing::debug!(%error, "Failed to update distributed status"),
    }
  }
}
