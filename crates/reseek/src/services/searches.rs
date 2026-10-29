//! Searches we start on the network.

use std::time::Duration;

use api_types::search::{Search, SearchListQuery, SearchRequest, SearchResponse};
use async_trait::async_trait;
use uuid::Uuid;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "searches";

/// What asking to cancel a search did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canceled {
  /// the search was running and is being stopped
  Now,
  /// the search had already completed
  Already,
}

/// Starting searches, following their responses, and stopping and deleting them.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait SearchService: Send + Sync {
  /// Records a search and starts it in the background. `request` has passed the API's checks,
  /// which read its `searchTimeout` as `search_timeout`, how long the search waits for another
  /// response. A search with the request's id already existing is a conflict, as is not being
  /// logged in.
  async fn start(
    &self,
    request: SearchRequest,
    search_timeout: Duration,
  ) -> Result<Search, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The searches, without their responses.
  async fn list(&self, query: SearchListQuery) -> Result<Vec<Search>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One search, with its responses when asked for.
  async fn get(&self, id: Uuid, include_responses: bool) -> Result<Search, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The responses of one search, empty while it runs.
  async fn responses(&self, id: Uuid) -> Result<Vec<SearchResponse>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Stops a running search.
  async fn cancel(&self, id: Uuid) -> Result<Canceled, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Deletes a search, stopping it first when it runs.
  async fn delete(&self, id: Uuid) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }
}
