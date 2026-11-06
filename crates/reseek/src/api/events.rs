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

/// The catalog's pseudo-types, which name no event; which real types have no sample is the
/// event log's to say.
const NOT_RAISABLE: &[&str] = &["None", "Any"];

/// The events routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(raise))
}

/// A page of the event log, newest first, or oldest first after `since`; `X-Total-Count` is
/// how many match the type and username filters.
#[utoipa::path(
  operation_id = "list_events",
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
      "Event type '{name}' cannot be raised"
    )));
  }
  name.parse().map_err(ApiError::bad_request)
}

#[cfg(test)]
mod tests {
  use crate::testing::{OUTSIDE, TRUSTED, app, request};
  use std::time::Duration;

  #[tokio::test]
  async fn a_raised_sample_is_logged_and_counted() {
    let app = app(TRUSTED, &[]).await;
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
    let record = app
      .send(request("POST", "/api/v0/events/ScriptRunCompleted", None))
      .await;
    assert_eq!(record.status, 400, "no sample is made of a script run");
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

  /// The types of the events the log holds, oldest first, once `count` are there.
  async fn logged(app: &crate::testing::TestApp, count: usize) -> Vec<String> {
    for _ in 0..200 {
      let listed = app
        .send(request("GET", "/api/v0/events?limit=100", None))
        .await
        .json();
      let mut types: Vec<String> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["type"].as_str().unwrap().to_string())
        .collect();
      if types.len() >= count {
        types.reverse();
        return types;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fewer than {count} events were logged");
  }

  #[tokio::test]
  async fn the_api_s_own_actions_each_raise_their_event_once() {
    let app = app(TRUSTED, &[]).await;
    let outside = OUTSIDE;
    let login = |password: &str| {
      request(
        "POST",
        "/api/v0/session",
        Some(serde_json::json!({ "username": "reseek", "password": password })),
      )
    };
    assert_eq!(app.send_from(outside, login("wrong")).await.status, 401);
    let signed_in = app.send_from(outside, login("reseek")).await;
    let cookie = crate::testing::set_cookie(&signed_in.headers, "reseek_session").unwrap();
    let logout = crate::testing::with(
      crate::testing::with(
        request("POST", "/api/v0/session/logout", None),
        "cookie",
        &cookie,
      ),
      "origin",
      "http://localhost",
    );
    assert_eq!(app.send_from(outside, logout).await.status, 200);
    let created = app
      .send(request(
        "POST",
        "/api/v0/tokens",
        Some(serde_json::json!({ "name": "script", "role": "ReadOnly" })),
      ))
      .await
      .json();
    let token = created["token"]["id"].as_str().unwrap().to_string();
    app
      .send(request("DELETE", &format!("/api/v0/tokens/{token}"), None))
      .await;
    let block = serde_json::json!({ "reason": "spam" });
    let blocked = app
      .send(request("PUT", "/api/v0/users/mallory/block", Some(block)))
      .await;
    assert!(blocked.status.is_success(), "{:?}", blocked.body);
    let unblocked = app
      .send(request("DELETE", "/api/v0/users/mallory/block", None))
      .await;
    assert!(unblocked.status.is_success(), "{:?}", unblocked.body);
    // the share scan at the start is a job of its own, told of as it ends
    let mut types = logged(&app, 8).await;
    types.retain(|kind| kind != "JobFinished");
    assert_eq!(
      types,
      [
        "LoginFailed",
        "SessionCreated",
        "SessionRevoked",
        "TokenCreated",
        "TokenRevoked",
        "UserBlocked",
        "UserUnblocked"
      ]
    );
  }
}
