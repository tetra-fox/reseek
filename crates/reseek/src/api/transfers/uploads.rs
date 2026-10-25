//! The uploads routes: listing, why a queued upload waits, bumping it, cancelling and removing.
//! Uploads are asked for by peers, never through the API.

use api_types::Page;
use api_types::transfers::{
  BulkRequest, BulkResult, CancelQuery, Transfer, TransferListQuery, TransferQuery,
  UploadQueueStatus, UploadsQuery, UserResponse,
};
use axum::extract::State;
use axum::http::StatusCode;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::list_query;
use crate::api::error::ApiError;
use crate::api::extract::{Json, Path, Query};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, ReadWrite};

/// The uploads routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(page))
    .routes(routes!(user_uploads))
    .routes(routes!(upload, cancel))
    .routes(routes!(why))
    .routes(routes!(bump))
    .routes(routes!(remove_completed))
    .routes(routes!(bulk))
}

/// Every upload, grouped by user and remote directory; `since`, `until` and `limit` bound it.
#[utoipa::path(
  get, path = "/transfers/uploads", tag = "transfers", params(UploadsQuery),
  responses(
    (status = 200, body = Vec<UserResponse>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<UploadsQuery>,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
  if let Some(limit) = query.limit {
    validate::positive("limit", limit)?;
  }
  app.services.transfers.uploads(query).await.map(Json)
}

/// A page of uploads, flat, filtered and sorted.
#[utoipa::path(
  get, path = "/transfers/uploads/list", tag = "transfers", params(TransferListQuery),
  responses(
    (status = 200, body = Page<Transfer>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn page(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<TransferListQuery>,
) -> Result<Json<Page<Transfer>>, ApiError> {
  list_query(&query)?;
  app.services.transfers.upload_list(query).await.map(Json)
}

/// The uploads to one user, grouped by remote directory; no directories when there are none.
#[utoipa::path(
  get, path = "/transfers/uploads/{username}", tag = "transfers",
  params(("username" = String, Path, description = "the peer")),
  responses(
    (status = 200, body = UserResponse), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn user_uploads(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(username): Path<String>,
) -> Result<Json<UserResponse>, ApiError> {
  app
    .services
    .transfers
    .user_uploads(&username)
    .await
    .map(Json)
}

/// One upload; `includeHistory` adds the states it went through.
#[utoipa::path(
  get, path = "/transfers/uploads/{username}/{id}", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the upload's id"),
    TransferQuery
  ),
  responses(
    (status = 200, body = Transfer), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn upload(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<TransferQuery>,
) -> Result<Json<Transfer>, ApiError> {
  app
    .services
    .transfers
    .upload(&username, id, query.include_history)
    .await
    .map(Json)
}

/// Why a queued upload waits: its place, its group's slots, the global slots and the user's
/// limits.
#[utoipa::path(
  get, path = "/transfers/uploads/{username}/{id}/why", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the upload's id")
  ),
  responses(
    (status = 200, body = UploadQueueStatus), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn why(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
) -> Result<Json<UploadQueueStatus>, ApiError> {
  app
    .services
    .transfers
    .upload_queue_status(&username, id)
    .await
    .map(Json)
}

/// Moves a queued upload to the front of its group's queue.
#[utoipa::path(
  post, path = "/transfers/uploads/{username}/{id}/bump", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the upload's id")
  ),
  responses(
    (status = 204, description = "bumped"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn bump(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Path((username, id)): Path<(String, Uuid)>,
) -> Result<StatusCode, ApiError> {
  app.services.transfers.bump(&username, id).await?;
  info!(by = admin.principal.subject, username, %id, "upload bumped");
  Ok(StatusCode::NO_CONTENT)
}

/// Cancels an upload; `remove` also removes it from the listings once it has completed.
#[utoipa::path(
  delete, path = "/transfers/uploads/{username}/{id}", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the upload's id"),
    CancelQuery
  ),
  responses(
    (status = 204, description = "cancelled"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn cancel(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<CancelQuery>,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .transfers
    .cancel_upload(&username, id, query.remove)
    .await?;
  info!(by = caller.principal.subject, username, %id, remove = query.remove, "upload cancelled");
  Ok(StatusCode::NO_CONTENT)
}

/// Removes every completed upload from the listings; the reports still count them.
#[utoipa::path(
  delete, path = "/transfers/uploads/all/completed", tag = "transfers",
  responses(
    (status = 204, description = "removed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn remove_completed(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
) -> Result<StatusCode, ApiError> {
  app.services.transfers.remove_completed_uploads().await?;
  info!(by = caller.principal.subject, "completed uploads removed");
  Ok(StatusCode::NO_CONTENT)
}

/// Cancels, removes or retries several uploads in one call.
#[utoipa::path(
  post, path = "/transfers/uploads/bulk", tag = "transfers", request_body = BulkRequest,
  responses(
    (status = 200, body = BulkResult), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn bulk(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(request): Json<BulkRequest>,
) -> Result<Json<BulkResult>, ApiError> {
  let action = request.action;
  let result = app.services.transfers.bulk_uploads(request).await?;
  info!(
    by = caller.principal.subject,
    ?action,
    ok = result.ok.len(),
    failed = result.failed.len(),
    "bulk upload action"
  );
  Ok(Json(result))
}
