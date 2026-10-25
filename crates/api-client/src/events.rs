//! The event log calls.

use api_types::Page;
use api_types::events::{EventRecord, EventType, EventsQuery};

use super::{Client, Error, Result, Route, json, send};

impl Client {
  /// `GET /events`: a page of the event log, with how many events match.
  pub async fn events(&self, query: &EventsQuery) -> Result<Page<EventRecord>> {
    let response = send(self.get(Route::new("events").query(query)?)).await?;
    let status = response.status();
    let unexpected = |text: &str| Error::Unexpected {
      status,
      text: text.to_string(),
    };
    let total = response
      .headers()
      .get("x-total-count")
      .ok_or_else(|| unexpected("no X-Total-Count"))?
      .to_str()
      .map_err(|_| unexpected("X-Total-Count is not text"))?
      .parse()
      .map_err(|_| unexpected("X-Total-Count is not a count"))?;
    let items: Vec<EventRecord> = serde_json::from_slice(&response.bytes().await?)?;
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
