//! The event log.

use api_types::Page;
use api_types::events::{EventRecord, EventType, EventsQuery};
use async_trait::async_trait;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "events";

/// Reading the event log and raising sample events.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait EventService: Send + Sync {
  /// A page of the logged events, with how many match the type and username filters.
  async fn list(&self, query: EventsQuery) -> Result<Page<EventRecord>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Raises an event of `event_type` with placeholder data, every text in it starting with
  /// `disambiguator`, exactly as a real one is raised: it is logged and every script that
  /// takes it runs.
  async fn raise_sample(
    &self,
    event_type: EventType,
    disambiguator: String,
  ) -> Result<EventRecord, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
