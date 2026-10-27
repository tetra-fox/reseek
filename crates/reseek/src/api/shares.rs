//! The shares routes: every host's shares, what one share holds, and scanning this host's
//! shares. Reading takes the ReadOnly role, scanning ReadWrite.

use api_types::shares::{Share, ShareScanQuery, SharesByHost};
use api_types::users::Directory;
use axum::extract::State;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};

/// The shares routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list, scan, cancel_scan))
    .routes(routes!(contents))
    .routes(routes!(share))
    .routes(routes!(share_contents))
}

/// The shares of every host by host name, this one as `local`.
#[utoipa::path(
  operation_id = "list_shares",
  get, path = "/shares", tag = "shares",
  responses(
    (status = 200, body = SharesByHost), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<SharesByHost>, ApiError> {
  app.services.shares.shares().await.map(Json)
}

/// One share, on any host.
#[utoipa::path(
  get, path = "/shares/{id}", tag = "shares",
  params(("id" = String, Path, description = "the share's id")),
  responses(
    (status = 200, body = Share), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn share(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<String>,
) -> Result<Json<Share>, ApiError> {
  app.services.shares.share(&id).await.map(Json)
}

/// Everything every host shares, as other users browsing us see it.
#[utoipa::path(
  get, path = "/shares/contents", tag = "shares",
  responses(
    (status = 200, body = Vec<Directory>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn contents(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<Directory>>, ApiError> {
  app.services.shares.contents().await.map(Json)
}

/// What one share holds, its own directory included.
#[utoipa::path(
  get, path = "/shares/{id}/contents", tag = "shares",
  params(("id" = String, Path, description = "the share's id")),
  responses(
    (status = 200, body = Vec<Directory>), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn share_contents(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<String>,
) -> Result<Json<Vec<Directory>>, ApiError> {
  app.services.shares.share_contents(&id).await.map(Json)
}

/// Starts a scan of this host's shares; `force` reads every file again. The scan's progress is
/// in the application state.
#[utoipa::path(
  put, path = "/shares", tag = "shares", params(ShareScanQuery),
  responses(
    (status = 204, description = "the scan started"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody, description = "a scan is running"),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn scan(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Query(query): Query<ShareScanQuery>,
) -> Result<StatusCode, ApiError> {
  app.services.shares.scan(query.force).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Stops the running scan.
#[utoipa::path(
  delete, path = "/shares", tag = "shares",
  responses(
    (status = 204, description = "the scan is stopping"),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody, description = "no scan is running"),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn cancel_scan(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
) -> Result<StatusCode, ApiError> {
  app.services.shares.cancel_scan().await?;
  Ok(StatusCode::NO_CONTENT)
}
