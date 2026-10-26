//! The events routes: the event log, newest first, and raising a sample event to try scripts
//! with.

use api_types::events::{EventRecord, EventType, EventsQuery};
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::validate;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized};

/// The event types a sample is never made of: the catalogue's pseudo-types and the record of a
/// script run, which scripts never receive.
const NOT_RAISABLE: &[&str] = &["None", "Any", "ScriptRunCompleted"];

/// The events routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(raise))
}

/// A page of the event log, newest first, or oldest first after `since`; `X-Total-Count` is
/// how many match the type and username filters.
#[utoipa::path(
  get, path = "/events", tag = "events", params(EventsQuery),
  responses(
    (status = 200, body = Vec<EventRecord>, headers(("X-Total-Count" = u64, description = "how many events match"))),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<EventsQuery>,
) -> Result<Response, ApiError> {
  validate::positive("limit", query.limit)?;
  let page = app.services.events.list(query).await?;
  let mut response = Json(page.items).into_response();
  response
    .headers_mut()
    .insert("x-total-count", HeaderValue::from(page.total));
  Ok(response)
}

/// Raises a sample event of a type, its data made up with every text starting with the body,
/// a JSON string that may be empty. It is logged and runs every script that takes it, exactly
/// as a real one does.
#[utoipa::path(
  post, path = "/events/{type}", tag = "events", request_body = Option<String>,
  params(("type" = EventType, Path, description = "the event type, in any case")),
  responses(
    (status = 201, body = EventRecord), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn raise(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Path(name): Path<String>,
  body: Option<Json<Option<String>>>,
) -> Result<Response, ApiError> {
  let event_type = raisable(&name)?;
  let disambiguator = body.and_then(|Json(text)| text).unwrap_or_default();
  let record = app
    .services
    .events
    .raise_sample(event_type, disambiguator)
    .await?;
  info!(by = admin.principal.subject, event = name, id = %record.id, "sample event raised");
  Ok((StatusCode::CREATED, Json(record)).into_response())
}

/// The event type `name` names in any case, refused when it is unknown or no sample is made of
/// it.
fn raisable(name: &str) -> Result<EventType, ApiError> {
  if NOT_RAISABLE
    .iter()
    .any(|kind| kind.eq_ignore_ascii_case(name))
  {
    return Err(ApiError::bad_request(format!(
      "Event type '{name}' can not be raised"
    )));
  }
  name.parse().map_err(ApiError::bad_request)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{app, request};
  use std::time::Duration;

  #[tokio::test]
  async fn a_raised_sample_is_logged_and_counted() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let raised = app
      .send(request(
        "POST",
        "/api/v0/events/noop",
        Some(serde_json::json!("x")),
      ))
      .await;
    assert_eq!(raised.status, 201);
    let id = raised.json()["id"].clone();
    assert_eq!(raised.json()["type"], "Noop");
    for _ in 0..200 {
      let listed = app
        .send(request("GET", "/api/v0/events?type=Noop,UserBlocked", None))
        .await;
      assert_eq!(listed.status, 200);
      if listed.headers["x-total-count"] == "1" {
        assert_eq!(listed.json()[0]["id"], id);
        return;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the sample was never logged");
  }

  #[test]
  fn samples_are_made_of_known_types_but_not_the_script_record() {
    assert_eq!(
      raisable("downloadfilecomplete").unwrap(),
      EventType::DownloadFileComplete
    );
    assert_eq!(raisable("Noop").unwrap(), EventType::Noop);
    for name in ["Any", "none", "ScriptRunCompleted"] {
      let error = format!("{:?}", raisable(name).unwrap_err());
      assert!(error.contains("can not be raised"), "{name}: {error}");
    }
    let unknown = format!("{:?}", raisable("Bogus").unwrap_err());
    assert!(unknown.contains("unknown event type 'Bogus'"), "{unknown}");
  }
}
