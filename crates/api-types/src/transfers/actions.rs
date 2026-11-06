//! Canceling, removing and retrying transfers, one at a time or many in one call.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// Whether canceling a transfer also removes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct CancelQuery {
  /// also remove the transfer from the listings, once it has completed when it is still running
  #[serde(default)]
  pub remove: bool,
}

/// What a bulk call does to each of its transfers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BulkAction {
  /// cancel the transfer if it has not completed
  Cancel,
  /// remove the transfer from the listings, canceling it first if it is running
  Remove,
  /// enqueue a completed download again, as a new transfer with the same destination and search;
  /// the downloads retried in one call make one new download job
  Retry,
  /// pause a download that has not completed: it stops, keeps what arrived and waits until
  /// resumed; its user forgets its place in their queue
  Pause,
  /// resume a paused download
  Resume,
}

/// One action applied to several transfers of one direction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkRequest {
  /// the transfers to act on
  pub ids: Vec<Uuid>,
  /// what to do to each
  pub action: BulkAction,
}

/// What a bulk call did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult {
  /// the transfers the action was applied to
  pub ok: Vec<Uuid>,
  /// the transfers it could not be applied to, and why
  pub failed: Vec<BulkFailure>,
}

/// A transfer a bulk action could not be applied to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkFailure {
  /// the transfer's id
  pub id: Uuid,
  /// why the action failed for it
  pub error: String,
}
