//! The downloads routes: listing, enqueueing one by one or as a batch, the place in a peer's
//! queue, canceling and removing.

use std::collections::HashSet;

use api_types::Page;
use api_types::transfers::{
  Batch, BulkRequest, BulkResult, CancelQuery, EnqueueDownloadBatchRequest,
  EnqueueDownloadBatchResponse, IncludeRemovedQuery, QueueDownloadRequest, QueueDownloadResponse,
  Transfer, TransferListQuery, TransferQuery, UserResponse,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::list_query;
use crate::api::error::ApiError;
use crate::api::extract::{Json, Path, Query};
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};
use crate::safety::{non_traversing, relative_everywhere, traverses};

/// The longest username a batch takes.
const USERNAME_CHARS: usize = 500;

/// The downloads routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(page))
    .routes(routes!(user_downloads, enqueue))
    .routes(routes!(download, cancel))
    .routes(routes!(place_in_queue))
    .routes(routes!(enqueue_batch))
    .routes(routes!(batch))
    .routes(routes!(remove_completed))
    .routes(routes!(bulk))
}

/// Every download, grouped by user and remote directory.
#[utoipa::path(
  operation_id = "list_downloads",
  get, path = "/transfers/downloads", tag = "transfers", params(IncludeRemovedQuery),
  responses(
    (status = 200, body = Vec<UserResponse>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<IncludeRemovedQuery>,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
  app
    .services
    .transfers
    .downloads(query.include_removed)
    .await
    .map(Json)
}

/// A page of downloads, flat, filtered and sorted.
#[utoipa::path(
  operation_id = "page_downloads",
  get, path = "/transfers/downloads/list", tag = "transfers", params(TransferListQuery),
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
  app.services.transfers.download_list(query).await.map(Json)
}

/// The downloads from one user, grouped by remote directory.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}", tag = "transfers",
  params(("username" = String, Path, description = "the peer")),
  responses(
    (status = 200, body = UserResponse), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn user_downloads(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(username): Path<String>,
) -> Result<Json<UserResponse>, ApiError> {
  app
    .services
    .transfers
    .user_downloads(&username)
    .await
    .map(Json)
}

/// One download; `includeHistory` adds the states it went through.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    TransferQuery
  ),
  responses(
    (status = 200, body = Transfer), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<TransferQuery>,
) -> Result<Json<Transfer>, ApiError> {
  app
    .services
    .transfers
    .download(&username, id, query.include_history)
    .await
    .map(Json)
}

/// Asks the peer for a queued download's place in its queue.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/position", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id")
  ),
  responses(
    (status = 200, body = u32, content_type = "application/json"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody), (status = 504, body = api_types::ErrorBody)
  )
)]
async fn place_in_queue(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
) -> Result<Json<u32>, ApiError> {
  app
    .services
    .transfers
    .place_in_queue(&username, id)
    .await
    .map(Json)
}

/// Enqueues files from one user. Answered 201 when at least one file was enqueued and 200 when
/// none was; the files that were not are listed with why.
#[utoipa::path(
  post, path = "/transfers/downloads/{username}", tag = "transfers",
  request_body = Vec<QueueDownloadRequest>,
  params(("username" = String, Path, description = "the peer")),
  responses(
    (status = 201, body = QueueDownloadResponse), (status = 200, body = QueueDownloadResponse),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody),
    (status = 502, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn enqueue(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(username): Path<String>,
  Json(files): Json<Vec<Option<QueueDownloadRequest>>>,
) -> Result<Response, ApiError> {
  let files = queue_requests(files)?;
  let count = files.len();
  let answer = app.services.transfers.enqueue(&username, files).await?;
  info!(
    by = caller.principal.subject,
    username,
    asked = count,
    enqueued = answer.enqueued.len(),
    "downloads enqueued"
  );
  let status = if answer.enqueued.is_empty() {
    StatusCode::OK
  } else {
    StatusCode::CREATED
  };
  Ok((status, Json(answer)).into_response())
}

/// Refuses an empty list, a null entry and a filename that is empty or steps outside its
/// directory.
fn queue_requests(
  files: Vec<Option<QueueDownloadRequest>>,
) -> Result<Vec<QueueDownloadRequest>, ApiError> {
  if files.is_empty() {
    return Err(ApiError::bad_request("At least one file is required"));
  }
  let files: Vec<QueueDownloadRequest> = files
    .into_iter()
    .collect::<Option<_>>()
    .ok_or_else(|| ApiError::bad_request("One or more files in the request are null"))?;
  if files.iter().any(|file| file.filename.trim().is_empty()) {
    return Err(ApiError::bad_request("The Filename field is required"));
  }
  if files.iter().any(|file| traverses(&file.filename)) {
    return Err(ApiError::bad_request(
      "One or more files in the request contain a dangerous path traversal segment",
    ));
  }
  Ok(files)
}

/// Enqueues files from one user as a batch. Answered 201 when every file was enqueued, 207 when
/// some were, and 200 when none was; the batch exists in every case.
#[utoipa::path(
  post, path = "/transfers/downloads/batches", tag = "transfers",
  request_body = EnqueueDownloadBatchRequest,
  responses(
    (status = 201, body = EnqueueDownloadBatchResponse),
    (status = 207, body = EnqueueDownloadBatchResponse),
    (status = 200, body = EnqueueDownloadBatchResponse),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody),
    (status = 502, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn enqueue_batch(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(request): Json<EnqueueDownloadBatchRequest>,
) -> Result<Response, ApiError> {
  batch_request(&request)?;
  let count = request.files.len();
  let answer = app.services.transfers.enqueue_batch(request).await?;
  info!(
    by = caller.principal.subject,
    batch = %answer.batch.id,
    username = answer.batch.username,
    asked = count,
    failed = answer.failures.len(),
    "download batch enqueued"
  );
  let status = if answer.failures.is_empty() {
    StatusCode::CREATED
  } else if answer.failures.len() < count {
    StatusCode::MULTI_STATUS
  } else {
    StatusCode::OK
  };
  Ok((status, Json(answer)).into_response())
}

/// Refuses a batch whose ids are nil, whose username is blank or too long, whose files are
/// none, repeated, unnamed, too large or step outside their directory, or whose destination is
/// not a relative path that stays inside the downloads directory.
fn batch_request(request: &EnqueueDownloadBatchRequest) -> Result<(), ApiError> {
  if request.id == Some(Uuid::nil()) {
    return Err(ApiError::bad_request(
      "The Id field must not contain an empty GUID/UUID",
    ));
  }
  if request.search_id == Some(Uuid::nil()) {
    return Err(ApiError::bad_request(
      "The SearchId field must not contain an empty GUID/UUID",
    ));
  }
  if request.username.trim().is_empty() {
    return Err(ApiError::bad_request("The Username field is required"));
  }
  if request.username.chars().count() > USERNAME_CHARS {
    return Err(ApiError::bad_request(
      "The Username field must be between 1 and 500 characters",
    ));
  }
  if request.files.is_empty() {
    return Err(ApiError::bad_request(
      "The field Files must contain at least one file",
    ));
  }
  let mut seen = HashSet::new();
  for file in &request.files {
    if file.filename.trim().is_empty() {
      return Err(ApiError::bad_request("The Filename field is required"));
    }
    non_traversing("Filename", &file.filename).map_err(ApiError::bad_request)?;
    if file.size > i64::MAX as u64 {
      return Err(ApiError::bad_request(format!(
        "The field Size must be between 0 and {}",
        i64::MAX
      )));
    }
    if !seen.insert(file.filename.as_str()) {
      return Err(ApiError::bad_request(
        "Two or more files in the request are repeated",
      ));
    }
  }
  if let Some(destination) = &request.options.destination {
    if destination.is_empty() {
      return Err(ApiError::bad_request(
        "The Destination field must contain at least one character",
      ));
    }
    if destination.trim().is_empty() {
      return Err(ApiError::bad_request(
        "The Destination field must not contain only whitespace",
      ));
    }
    relative_everywhere("Destination", destination).map_err(ApiError::bad_request)?;
    non_traversing("Destination", destination).map_err(ApiError::bad_request)?;
  }
  Ok(())
}

/// One batch with its downloads; `includeRemoved` lists removed ones too.
#[utoipa::path(
  get, path = "/transfers/downloads/batches/{id}", tag = "transfers",
  params(("id" = Uuid, Path, description = "the batch's id"), IncludeRemovedQuery),
  responses(
    (status = 200, body = Batch), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn batch(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<Uuid>,
  Query(query): Query<IncludeRemovedQuery>,
) -> Result<Json<Batch>, ApiError> {
  app
    .services
    .transfers
    .batch(id, query.include_removed)
    .await
    .map(Json)
}

/// Cancels a download; `remove` also removes it from the listings once it has completed.
#[utoipa::path(
  operation_id = "cancel_download",
  delete, path = "/transfers/downloads/{username}/{id}", tag = "transfers",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    CancelQuery
  ),
  responses(
    (status = 204, description = "canceled"), (status = 400, body = api_types::ErrorBody),
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
    .cancel_download(&username, id, query.remove)
    .await?;
  info!(by = caller.principal.subject, username, %id, remove = query.remove, "download canceled");
  Ok(StatusCode::NO_CONTENT)
}

/// Removes every completed download from the listings; the reports still count them.
#[utoipa::path(
  operation_id = "remove_completed_downloads",
  delete, path = "/transfers/downloads/all/completed", tag = "transfers",
  responses(
    (status = 204, description = "removed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn remove_completed(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
) -> Result<StatusCode, ApiError> {
  app.services.transfers.remove_completed_downloads().await?;
  info!(by = caller.principal.subject, "completed downloads removed");
  Ok(StatusCode::NO_CONTENT)
}

/// Cancels, removes or retries several downloads in one call.
#[utoipa::path(
  operation_id = "bulk_downloads",
  post, path = "/transfers/downloads/bulk", tag = "transfers", request_body = BulkRequest,
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
  let result = app.services.transfers.bulk_downloads(request).await?;
  info!(
    by = caller.principal.subject,
    ?action,
    ok = result.ok.len(),
    failed = result.failed.len(),
    "bulk download action"
  );
  Ok(Json(result))
}

#[cfg(test)]
mod tests {
  use super::*;
  use api_types::transfers::{BatchOptions, EnqueueDownloadBatchItem};

  fn file(filename: &str, size: u64) -> EnqueueDownloadBatchItem {
    EnqueueDownloadBatchItem {
      filename: filename.to_string(),
      size,
      attributes: Vec::new(),
    }
  }

  fn batch_of(
    files: Vec<EnqueueDownloadBatchItem>,
    destination: Option<&str>,
  ) -> EnqueueDownloadBatchRequest {
    EnqueueDownloadBatchRequest {
      id: None,
      search_id: None,
      username: "alice".to_string(),
      files,
      options: BatchOptions {
        destination: destination.map(str::to_string),
        external_id: None,
      },
    }
  }

  fn refusal(request: &EnqueueDownloadBatchRequest) -> String {
    format!("{:?}", batch_request(request).unwrap_err())
  }

  #[test]
  fn batches_refuse_what_cannot_be_downloaded_safely() {
    let fine = batch_of(vec![file("@@a\\Music\\01.flac", 1)], Some("Artist/Album"));
    assert!(batch_request(&fine).is_ok());
    let mut nil = fine.clone();
    nil.id = Some(Uuid::nil());
    assert!(refusal(&nil).contains("Id field must not contain an empty"));
    let mut nil_search = fine.clone();
    nil_search.search_id = Some(Uuid::nil());
    assert!(refusal(&nil_search).contains("SearchId field"));
    let mut blank = fine.clone();
    blank.username = "  ".to_string();
    assert!(refusal(&blank).contains("Username field is required"));
    let mut long = fine.clone();
    long.username = "x".repeat(501);
    assert!(refusal(&long).contains("between 1 and 500"));
    assert!(refusal(&batch_of(vec![], None)).contains("at least one file"));
    let repeated = batch_of(vec![file("a\\b", 1), file("a\\b", 2)], None);
    assert!(refusal(&repeated).contains("repeated"));
    let traversing = batch_of(vec![file("a\\..\\b", 1)], None);
    assert!(refusal(&traversing).contains("field Filename contains one or more unsafe"));
    let huge = batch_of(vec![file("a", u64::MAX)], None);
    assert!(refusal(&huge).contains("Size must be between 0 and 9223372036854775807"));
    for (destination, message) in [
      ("", "at least one character"),
      ("   ", "only whitespace"),
      ("/srv/music", "relative path on all operating systems"),
      ("C:\\Music", "relative path on all operating systems"),
      (
        "Artist/../..",
        "field Destination contains one or more unsafe",
      ),
    ] {
      let request = batch_of(vec![file("a", 1)], Some(destination));
      assert!(refusal(&request).contains(message), "{destination:?}");
    }
  }

  #[test]
  fn single_enqueues_refuse_empty_null_and_traversing_files() {
    let ok = |name: &str| {
      Some(QueueDownloadRequest {
        filename: name.to_string(),
        size: 0,
      })
    };
    assert!(queue_requests(vec![ok("a\\b.flac")]).is_ok());
    let message = |files| format!("{:?}", queue_requests(files).unwrap_err());
    assert!(message(vec![]).contains("At least one file is required"));
    assert!(message(vec![ok("a"), None]).contains("are null"));
    assert!(message(vec![ok("")]).contains("Filename field is required"));
    assert!(message(vec![ok("..\\secret")]).contains("dangerous path traversal"));
  }
}
