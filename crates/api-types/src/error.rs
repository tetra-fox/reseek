//! The body of every error response.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// What went wrong, sent with every 4xx and 5xx status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
  /// a short message, or a fixed code such as `soulseek_unavailable`
  #[schema(example = "invalid configuration")]
  pub error: String,
  /// a longer explanation, null when the message says it all
  pub detail: Option<String>,
}
