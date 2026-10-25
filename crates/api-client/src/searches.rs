//! The search calls.

use api_types::search::{Search, SearchGetQuery, SearchListQuery, SearchRequest, SearchResponse};
use reqwest::StatusCode;
use uuid::Uuid;

use super::{Client, Result, Route, done, json, status};

impl Client {
  /// `POST /searches`: starts a search.
  pub async fn start_search(&self, request: &SearchRequest) -> Result<Search> {
    json(self.post(Route::new("searches")).json(request)).await
  }

  /// `GET /searches`: the searches, without their responses.
  pub async fn searches(&self, query: &SearchListQuery) -> Result<Vec<Search>> {
    json(self.get(Route::new("searches").query(query)?)).await
  }

  /// `GET /searches/{id}`: one search, with its responses when asked for.
  pub async fn search(&self, id: Uuid, include_responses: bool) -> Result<Search> {
    let route = Route::new("searches")
      .segment(&id.to_string())
      .query(&SearchGetQuery { include_responses })?;
    json(self.get(route)).await
  }

  /// `GET /searches/{id}/responses`: one search's responses.
  pub async fn search_responses(&self, id: Uuid) -> Result<Vec<SearchResponse>> {
    let route = Route::new("searches")
      .segment(&id.to_string())
      .then("responses");
    json(self.get(route)).await
  }

  /// `PUT /searches/{id}`: stops a search; false when it had completed already.
  pub async fn cancel_search(&self, id: Uuid) -> Result<bool> {
    let route = Route::new("searches").segment(&id.to_string());
    Ok(status(self.put(route)).await? != StatusCode::NOT_MODIFIED)
  }

  /// `DELETE /searches/{id}`: deletes a search.
  pub async fn delete_search(&self, id: Uuid) -> Result<()> {
    done(self.delete(Route::new("searches").segment(&id.to_string()))).await
  }
}
