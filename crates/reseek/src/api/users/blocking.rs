//! Blocking users: the list of who is blocked and by what rule, blocking and unblocking one
//! user, and importing and exporting the list.

use std::time::Duration;

use api_types::blocking::{Block, BlockImportResult, BlockRequest, BlockedQuery, BlockedUsers};
use api_types::chat::{ExportFormat, ExportQuery};
use api_types::users::UsernameQuery;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::{Json, NamedUser, Query, essence};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, ReadWrite};
use crate::services::BlockImport;

/// The blocking routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(blocked))
    .routes(routes!(block, unblock))
    .routes(routes!(import))
    .routes(routes!(export))
}

/// The blocked users, those the configuration blocks with them unless `includeConfig` is
/// false, and the configuration's blocking rules.
#[utoipa::path(
  get, path = "/users/blocked", tag = "blocking", params(BlockedQuery),
  responses(
    (status = 200, body = BlockedUsers), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn blocked(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<BlockedQuery>,
) -> Result<Json<BlockedUsers>, ApiError> {
  app
    .services
    .blocking
    .blocked(query.include_config)
    .await
    .map(Json)
}

/// Blocks a user, or replaces their block: from what (`scopes`), why, and until when, given as
/// `expiresIn` or `expiresAt`.
#[utoipa::path(
  put, path = "/users/{username}/block", tag = "blocking", request_body = BlockRequest,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = Block), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn block(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(request): Json<BlockRequest>,
) -> Result<Json<Block>, ApiError> {
  let expires_in = block_request(&request)?;
  let cancel_transfers = request.cancel_transfers;
  let block = app
    .services
    .blocking
    .block(&caller.principal, &username, request, expires_in)
    .await?;
  info!(by = caller.principal.subject, username, "user blocked");
  if cancel_transfers {
    if block
      .scopes
      .contains(&api_types::blocking::BlockScope::Uploads)
    {
      app.uploads.blocked(&username);
    }
    app.downloads.blocked(&username, &block.scopes).await;
  }
  Ok(Json(block))
}

/// Refuses a reason over 500 characters, an empty list of scopes, both `expiresIn` and
/// `expiresAt`, and an `expiresIn` that is no duration; the duration `expiresIn` gives
/// otherwise.
fn block_request(request: &BlockRequest) -> Result<Option<Duration>, ApiError> {
  if !crate::users::reason_fits(&request.reason) {
    return Err(ApiError::bad_request(
      "reason must be at most 500 characters",
    ));
  }
  if request.scopes.as_ref().is_some_and(Vec::is_empty) {
    return Err(ApiError::bad_request(
      "scopes must name at least one scope; leave it out for the default scopes",
    ));
  }
  if request.expires_in.is_some() && request.expires_at.is_some() {
    return Err(ApiError::bad_request(
      "give expiresIn or expiresAt, not both",
    ));
  }
  request
    .expires_in
    .as_deref()
    .map(|text| validate::duration("expiresIn", text))
    .transpose()
}

/// Removes a user's block.
#[utoipa::path(
  delete, path = "/users/{username}/block", tag = "blocking",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "unblocked"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody, description = "the configuration blocks the user"),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn unblock(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .blocking
    .unblock(&caller.principal, &username)
    .await?;
  info!(by = caller.principal.subject, username, "user unblocked");
  Ok(StatusCode::NO_CONTENT)
}

/// Blocks the users of a list: plain text with one username per line and `#` comments, or the
/// JSON blocks an export wrote. Users blocked already are skipped.
#[utoipa::path(
  post, path = "/users/blocked/import", tag = "blocking",
  request_body(content((String = "text/plain"), (Vec<Block> = "application/json"))),
  responses(
    (status = 200, body = BlockImportResult), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 415, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn import(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  headers: HeaderMap,
  body: Bytes,
) -> Result<Json<BlockImportResult>, ApiError> {
  let import = block_import(&headers, &body)?;
  let result = app
    .services
    .blocking
    .import(&admin.principal, import)
    .await?;
  info!(
    by = admin.principal.subject,
    added = result.added,
    skipped = result.skipped,
    "blocks imported"
  );
  Ok(Json(result))
}

/// The users an import body names, by its content type.
fn block_import(headers: &HeaderMap, body: &[u8]) -> Result<BlockImport, ApiError> {
  let kind = headers
    .get(header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .map(essence)
    .unwrap_or_default();
  match kind.as_str() {
    "text/plain" => {
      let text =
        std::str::from_utf8(body).map_err(|_| ApiError::bad_request("the list is not UTF-8"))?;
      let names = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect();
      Ok(BlockImport::Names(names))
    }
    "application/json" => serde_json::from_slice(body)
      .map(BlockImport::Blocks)
      .map_err(|e| ApiError::invalid("invalid request body", e.to_string())),
    _ => Err(ApiError::unsupported_media(
      "the list must be text/plain or application/json",
    )),
  }
}

/// The blocks made through the API, as JSON or as one username per line.
#[utoipa::path(
  get, path = "/users/blocked/export", tag = "blocking", params(ExportQuery),
  responses(
    (status = 200, content((Vec<Block> = "application/json"), (String = "text/plain"))),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn export(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
  let blocks = app.services.blocking.export().await?;
  Ok(match query.format {
    ExportFormat::Json => Json(blocks).into_response(),
    ExportFormat::Text => {
      let text: String = blocks.iter().map(|b| format!("{}\n", b.username)).collect();
      ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
    }
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn imports_read_names_or_blocks_by_content_type() {
    let typed = |kind: &'static str| {
      let mut headers = HeaderMap::new();
      headers.insert(header::CONTENT_TYPE, kind.parse().unwrap());
      headers
    };
    let names = block_import(
      &typed("text/plain; charset=utf-8"),
      b"alice\n# spam\n\n bob \n",
    );
    assert_eq!(
      names.unwrap(),
      BlockImport::Names(vec!["alice".to_string(), "bob".to_string()])
    );
    let blocks = block_import(&typed("application/json"), b"[]").unwrap();
    assert_eq!(blocks, BlockImport::Blocks(Vec::new()));
    assert_eq!(
      block_import(&typed("application/json"), b"{")
        .unwrap_err()
        .status(),
      400
    );
    assert_eq!(
      block_import(&typed("text/csv"), b"x").unwrap_err().status(),
      415
    );
  }
}
