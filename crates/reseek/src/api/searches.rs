//! The searches routes: starting a search, listing searches, reading one with its responses,
//! and stopping and deleting it. Reading takes the ReadOnly role, the rest ReadWrite.

use std::time::Duration;

use api_types::search::{
  Search, SearchGetQuery, SearchListQuery, SearchRequest, SearchResponse, SearchScopeType,
  SearchTimeout,
};
use axum::extract::State;
use axum::http::StatusCode;
use soulseek_proto::SearchQuery;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};
use crate::searches;
use crate::services::Canceled;

/// The shortest wait for another response.
const SHORTEST_TIMEOUT: Duration = Duration::from_secs(1);

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
  Json(request): Json<SearchRequest>,
) -> Result<Json<Search>, ApiError> {
  let search_timeout = timeout(&request.search_timeout)?;
  search_request(&request)?;
  let search = app.services.searches.start(request, search_timeout).await?;
  info!(
    by = caller.principal.subject,
    id = %search.id,
    text = search.search_text,
    "search started"
  );
  Ok(Json(search))
}

/// How long the search waits for another response, at least a second.
fn timeout(timeout: &SearchTimeout) -> Result<Duration, ApiError> {
  let duration = match timeout {
    SearchTimeout::Milliseconds(millis) => Duration::from_millis(*millis),
    SearchTimeout::Text(text) => validate::duration("searchTimeout", text)?,
  };
  if duration < SHORTEST_TIMEOUT {
    return Err(ApiError::bad_request(
      "The value of searchTimeout must be at least 1s",
    ));
  }
  Ok(duration)
}

/// Refuses a search with no text, with only excluded terms, with only terms of one character,
/// or with a scope that names the wrong number of users or rooms. Whom a rooms or buddies scope
/// reaches is the search engine's to work out.
fn search_request(request: &SearchRequest) -> Result<(), ApiError> {
  if request.search_text.trim().is_empty() {
    return Err(ApiError::bad_request(
      "The field SearchText cannot be null, empty or only whitespace",
    ));
  }
  let query = SearchQuery::parse(&request.search_text);
  if query.terms.is_empty() {
    return Err(ApiError::bad_request(
      "Search query must contain at least one non-exclusion term",
    ));
  }
  if searches::sendable(query).terms.is_empty() {
    return Err(ApiError::bad_request(
      "Search query must contain at least one non-exclusion term with length greater than 1",
    ));
  }
  let subjects = request.scope.subjects.len();
  match request.scope.scope_type {
    SearchScopeType::User if subjects == 0 => Err(ApiError::bad_request(
      "A user search must name at least one user",
    )),
    SearchScopeType::Room if subjects != 1 => {
      Err(ApiError::bad_request("A room search must name one room"))
    }
    _ => Ok(()),
  }
}

/// The searches, without their responses, oldest first unless `order` says otherwise.
#[utoipa::path(
  operation_id = "list_searches",
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
  operation_id = "cancel_search",
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
    Canceled::Now => {
      info!(by = caller.principal.subject, %id, "search canceled");
      Ok(StatusCode::OK)
    }
    Canceled::Already => Ok(StatusCode::NOT_MODIFIED),
  }
}

/// Deletes a search, stopping it first when it runs.
#[utoipa::path(
  operation_id = "delete_search",
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
    let seconds = Duration::from_secs;
    assert_eq!(
      timeout(&SearchTimeout::Milliseconds(15_000)).unwrap(),
      seconds(15)
    );
    assert_eq!(
      timeout(&SearchTimeout::Text("15s".into())).unwrap(),
      seconds(15)
    );
    assert_eq!(
      timeout(&SearchTimeout::Text("1m 30s".into())).unwrap(),
      seconds(90)
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
    let message = |text: &str| format!("{:?}", search_request(&request(text)).unwrap_err());
    assert!(search_request(&request("pink floyd -live")).is_ok());
    assert!(
      search_request(&request("a pink")).is_ok(),
      "one long term is enough"
    );
    assert!(
      search_request(&request("\u{e9}t\u{e9}")).is_ok(),
      "characters, not bytes"
    );
    assert!(message("   ").contains("cannot be null"));
    assert!(message("-live --remix").contains("non-exclusion term\""));
    assert!(
      message("- a b -live").contains("length greater than 1"),
      "a lone dash is short"
    );
    assert!(message("\u{e9} x").contains("length greater than 1"));
    let mut room = request("animals");
    room.scope.scope_type = SearchScopeType::Room;
    assert!(search_request(&room).is_err());
    room.scope.subjects = vec!["indie".into()];
    assert!(search_request(&room).is_ok());
  }
}
