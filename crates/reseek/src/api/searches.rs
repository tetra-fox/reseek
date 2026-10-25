//! The searches routes: starting a search, listing searches, reading one with its responses,
//! and stopping and deleting it. Reading takes the ReadOnly role, the rest ReadWrite.

use api_types::search::{
  Search, SearchGetQuery, SearchListQuery, SearchRequest, SearchResponse, SearchScopeType,
  SearchTimeout,
};
use axum::extract::State;
use axum::http::StatusCode;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};
use crate::services::Cancelled;

/// The shortest wait for another response, in milliseconds.
const SHORTEST_TIMEOUT: u64 = 1000;

/// The searches routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list, start))
    .routes(routes!(get, cancel, delete))
    .routes(routes!(responses))
}

/// Starts a search and answers with its record before any response arrives; the responses
/// follow on the event stream.
#[utoipa::path(
  post, path = "/searches", tag = "searches", request_body = SearchRequest,
  responses(
    (status = 200, body = Search), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn start(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(mut request): Json<SearchRequest>,
) -> Result<Json<Search>, ApiError> {
  request.search_timeout = SearchTimeout::Milliseconds(timeout(&request.search_timeout)?);
  search_request(&request)?;
  let search = app.services.searches.start(request).await?;
  info!(
    by = caller.principal.subject,
    id = %search.id,
    text = search.search_text,
    "search started"
  );
  Ok(Json(search))
}

/// The timeout in milliseconds, at least a second.
fn timeout(timeout: &SearchTimeout) -> Result<u64, ApiError> {
  let millis = match timeout {
    SearchTimeout::Milliseconds(millis) => *millis,
    SearchTimeout::Text(text) => {
      let duration = validate::duration("searchTimeout", text)?;
      // a duration past u64::MAX milliseconds waits as long as one of u64::MAX does
      u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    }
  };
  if millis < SHORTEST_TIMEOUT {
    return Err(ApiError::bad_request("searchTimeout must be at least 1s"));
  }
  Ok(millis)
}

/// Refuses a search with no text, with only excluded terms, or with a scope that names the
/// wrong number of users or rooms.
fn search_request(request: &SearchRequest) -> Result<(), ApiError> {
  if request.search_text.trim().is_empty() {
    return Err(ApiError::bad_request(
      "The field SearchText can not be null, empty, or consist of only whitespace",
    ));
  }
  let excluded = |term: &str| term.len() > 1 && term.starts_with('-');
  if request.search_text.split_whitespace().all(excluded) {
    return Err(ApiError::bad_request(
      "Search query must contain at least one non-exclusion term",
    ));
  }
  let subjects = request.scope.subjects.len();
  match request.scope.scope_type {
    SearchScopeType::User if subjects == 0 => Err(ApiError::bad_request(
      "a user search names at least one user",
    )),
    SearchScopeType::Room if subjects != 1 => {
      Err(ApiError::bad_request("a room search names one room"))
    }
    _ => Ok(()),
  }
}

/// The searches, without their responses, oldest first unless `order` says otherwise.
#[utoipa::path(
  get, path = "/searches", tag = "searches", params(SearchListQuery),
  responses(
    (status = 200, body = Vec<Search>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<SearchListQuery>,
) -> Result<Json<Vec<Search>>, ApiError> {
  if let Some(limit) = query.limit {
    validate::positive("limit", limit)?;
  }
  app.services.searches.list(query).await.map(Json)
}

/// One search; `includeResponses` adds its responses.
#[utoipa::path(
  get, path = "/searches/{id}", tag = "searches",
  params(("id" = Uuid, Path, description = "the search's id"), SearchGetQuery),
  responses(
    (status = 200, body = Search), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn get(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<Uuid>,
  Query(query): Query<SearchGetQuery>,
) -> Result<Json<Search>, ApiError> {
  app
    .services
    .searches
    .get(id, query.include_responses)
    .await
    .map(Json)
}

/// One search's responses, empty while it runs.
#[utoipa::path(
  get, path = "/searches/{id}/responses", tag = "searches",
  params(("id" = Uuid, Path, description = "the search's id")),
  responses(
    (status = 200, body = Vec<SearchResponse>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn responses(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<Uuid>,
) -> Result<Json<Vec<SearchResponse>>, ApiError> {
  app.services.searches.responses(id).await.map(Json)
}

/// Stops a running search. Answered 304 when it had completed already.
#[utoipa::path(
  put, path = "/searches/{id}", tag = "searches",
  params(("id" = Uuid, Path, description = "the search's id")),
  responses(
    (status = 200, description = "stopping"),
    (status = 304, description = "the search had completed already"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn cancel(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  match app.services.searches.cancel(id).await? {
    Cancelled::Now => {
      info!(by = caller.principal.subject, %id, "search cancelled");
      Ok(StatusCode::OK)
    }
    Cancelled::Already => Ok(StatusCode::NOT_MODIFIED),
  }
}

/// Deletes a search, stopping it first when it runs.
#[utoipa::path(
  delete, path = "/searches/{id}", tag = "searches",
  params(("id" = Uuid, Path, description = "the search's id")),
  responses(
    (status = 204, description = "deleted"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn delete(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  app.services.searches.delete(id).await?;
  info!(by = caller.principal.subject, %id, "search deleted");
  Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn timeouts_are_milliseconds_or_durations_of_a_second_or_more() {
    assert_eq!(
      timeout(&SearchTimeout::Milliseconds(15_000)).unwrap(),
      15_000
    );
    assert_eq!(timeout(&SearchTimeout::Text("15s".into())).unwrap(), 15_000);
    assert_eq!(
      timeout(&SearchTimeout::Text("1m 30s".into())).unwrap(),
      90_000
    );
    assert!(
      timeout(&SearchTimeout::Milliseconds(15)).is_err(),
      "15 is milliseconds"
    );
    assert!(timeout(&SearchTimeout::Text("500ms".into())).is_err());
    assert!(timeout(&SearchTimeout::Text("soon".into())).is_err());
    let read: SearchRequest =
      serde_json::from_str(r#"{"searchText":"x","searchTimeout":"20s"}"#).unwrap();
    assert_eq!(read.search_timeout, SearchTimeout::Text("20s".into()));
  }

  #[test]
  fn a_search_needs_a_term_that_is_not_excluded() {
    let request = |text: &str| -> SearchRequest {
      serde_json::from_value(serde_json::json!({ "searchText": text })).unwrap()
    };
    assert!(search_request(&request("pink floyd -live")).is_ok());
    assert!(
      search_request(&request("-")).is_ok(),
      "a lone dash is a term"
    );
    assert!(search_request(&request("   ")).is_err());
    assert!(search_request(&request("-live --remix")).is_err());
    let mut room = request("animals");
    room.scope.scope_type = SearchScopeType::Room;
    assert!(search_request(&room).is_err());
    room.scope.subjects = vec!["indie".into()];
    assert!(search_request(&room).is_ok());
  }
}
