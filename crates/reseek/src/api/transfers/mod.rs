//! The transfers routes: downloads under `/transfers/downloads` and uploads under
//! `/transfers/uploads`. Reading takes the ReadOnly role, changing anything ReadWrite, bumping
//! an upload Administrator. A user literally named `all`, `batches`, `bulk` or `list` is reached
//! through the listings, not the per-user routes.

mod downloads;
mod uploads;

use api_types::transfers::TransferListQuery;
use utoipa_axum::router::OpenApiRouter;

use super::error::ApiError;
use super::validate;
use crate::app::AppState;

/// The most transfers a page of a flat listing holds.
const PAGE_MAX: u32 = 1000;

/// The transfers routes.
pub fn routes() -> OpenApiRouter<AppState> {
  downloads::routes().merge(uploads::routes())
}

/// Refuses a flat listing's page size outside 1 to 1000.
fn list_query(query: &TransferListQuery) -> Result<(), ApiError> {
  validate::limit("limit", query.limit, PAGE_MAX)
}
