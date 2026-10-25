//! The event log calls.

use api_types::Page;
use api_types::events::{EventRecord, EventType, EventsQuery};

use super::{Client, Error, Result, Route, json, send};

impl Client {
  /// `GET /events`: a page of the event log, with how many events match.
  pub async fn events(&self, query: &EventsQuery) -> Result<Page<EventRecord>> {
    let response = send(self.get(Route::new("events").query(query)?)).await?;
    let total = response
      .headers()
      .get("x-total-count")
      .and_then(|value| value.to_str().ok())
      .and_then(|value| value.parse().ok());
    let status = response.status();
    let items: Vec<EventRecord> = serde_json::from_slice(&response.bytes().await?)?;
    let total = total.ok_or_else(|| Error::Unexpected {
      status,
      text: "no X-Total-Count".to_string(),
    })?;
    Ok(Page { items, total })
  }

  /// `POST /events/{type}`: raises a sample event whose texts start with `disambiguator`.
  pub async fn raise_sample_event(
    &self,
    event_type: EventType,
    disambiguator: &str,
  ) -> Result<EventRecord> {
    let route = Route::new("events").segment(event_type.name());
    json(self.post(route).json(&disambiguator)).await
  }
}
