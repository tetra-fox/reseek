//! A page of a longer list.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One page of a list, with how many items the whole list has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
  /// the items on this page, in the order asked for
  pub items: Vec<T>,
  /// how many items match, on every page together
  pub total: u64,
}
