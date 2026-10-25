//! The HTTP API under `/api/v0`: the routes, their OpenAPI document, and the error type every
//! handler answers failures with.

pub mod agent;
mod analysis;
mod application;
pub mod error;
mod events;
pub mod extract;
mod files;
mod live;
mod messaging;
mod oidc;
mod options;
mod relay;
mod searches;
mod session;
mod shares;
mod telemetry;
mod tokens;
mod transfers;
mod users;
mod validate;

use axum::Router;
use axum::middleware;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::{Config, SwaggerUi};

use crate::app::AppState;
use crate::auth;

/// The OpenAPI document's description of the API as a whole.
#[derive(OpenApi)]
#[openapi(
  info(
    title = "reseek",
    description = "A Soulseek client daemon: its configuration, transfers, searches, chat and \
                   shares.",
    license(name = "AGPL-3.0-only", identifier = "AGPL-3.0-only")
  ),
  tags(
    (name = "application", description = "The daemon's state, version, restart and shutdown"),
    (name = "server", description = "The connection to the Soulseek server"),
    (name = "logs", description = "The daemon's log"),
    (name = "options", description = "The configuration and its files"),
    (name = "session", description = "Signing in and out, and browser sessions"),
    (name = "auth", description = "Signing in with an OpenID Connect provider"),
    (name = "tokens", description = "API tokens for scripts"),
    (name = "live", description = "The event stream"),
    (name = "searches", description = "Searches we start on the network"),
    (name = "transfers", description = "Downloads and uploads"),
    (name = "users", description = "Other users and the directory of users we know"),
    (name = "blocking", description = "Blocked users"),
    (name = "conversations", description = "Private messages"),
    (name = "rooms", description = "Chat rooms and their history"),
    (name = "messages", description = "Searching what was said"),
    (name = "shares", description = "What we share"),
    (name = "files", description = "The downloaded files, served to the browser"),
    (name = "analysis", description = "Checking whether a downloaded file is what it claims"),
    (name = "relay", description = "Relay controller and agents"),
    (name = "events", description = "The event log"),
    (name = "telemetry", description = "Metrics and reports over the transfer history")
  )
)]
struct ApiDoc;

/// Every route with its OpenAPI description.
fn routes() -> OpenApiRouter<AppState> {
  let api = OpenApiRouter::new()
    .merge(application::routes())
    .merge(searches::routes())
    .merge(transfers::routes())
    .merge(users::routes())
    .merge(messaging::routes())
    .merge(shares::routes())
    .merge(files::routes())
    .merge(analysis::routes())
    .merge(relay::routes())
    .merge(events::routes())
    .merge(telemetry::routes())
    .merge(options::routes())
    .merge(session::routes())
    .merge(oidc::routes())
    .merge(tokens::routes())
    .merge(live::routes());
  OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api/v0", api)
}

/// The API's routes for `state`, the diagnostic ones the document leaves out included, each
/// request authenticated first.
pub fn router(state: AppState) -> Router {
  let (router, _) = routes().split_for_parts();
  router
    .nest("/api/v0", application::hidden())
    .route_layer(middleware::from_fn_with_state(
      state.clone(),
      agent::refuse_in_agent_mode,
    ))
    .layer(middleware::from_fn_with_state(
      state.clone(),
      auth::authenticate,
    ))
    .with_state(state)
}

/// The OpenAPI document at `/swagger/v0/swagger.json` and the Swagger UI at `/swagger`, which
/// reads the document from under `url_base`.
pub fn swagger(url_base: &str) -> Router {
  let document = format!("{url_base}/swagger/v0/swagger.json");
  SwaggerUi::new("/swagger")
    .url("/swagger/v0/swagger.json", openapi())
    .config(Config::new([document]))
    .into()
}

/// The OpenAPI document of every route.
pub fn openapi() -> utoipa::openapi::OpenApi {
  routes().split_for_parts().1
}

#[cfg(test)]
mod tests {
  use axum::http::Method;
  use serde_json::{Value, json};

  use super::agent::DISABLED;
  use crate::testing::routes::{CONFIG, KEY, Operation, operations, probe};
  use crate::testing::{Answer, TestApp, app, request, with};

  #[test]
  fn the_committed_openapi_document_is_current() {
    let generated = super::openapi().to_pretty_json().unwrap() + "\n";
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../api/openapi.json");
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
      committed == generated,
      "api/openapi.json does not match the handlers; run `just api-spec`"
    );
  }

  /// Whether the daemon answers `operation` itself, rather than a service not built yet.
  fn answered_here(operation: &Operation) -> bool {
    let route = operation.route.as_str();
    let under = |prefix: &str| route == prefix || route.starts_with(&format!("{prefix}/"));
    under("/api/v0/application")
      || (route == "/api/v0/server" && operation.method == Method::GET)
      || route == "/api/v0/logs/live"
      || under("/api/v0/options")
      || (under("/api/v0/session") && route != "/api/v0/session/file-token")
      || under("/api/v0/sessions")
      || under("/api/v0/tokens")
      || under("/api/v0/auth")
      || route == "/api/v0/live"
      || (route == "/api/v0/relay" && operation.method == Method::GET)
  }

  /// Sends `operation`'s probe, the event stream read only for its status.
  async fn answer(app: &TestApp, operation: &Operation) -> Answer {
    if operation.route == "/api/v0/live" {
      let stream = app.stream_from("127.0.0.1:40000", probe(operation)).await;
      return Answer {
        status: stream.status,
        headers: stream.headers,
        body: Default::default(),
      };
    }
    app.send(probe(operation)).await
  }

  /// The operations in the order they are probed: stopping and restarting the daemon last.
  fn in_order() -> Vec<Operation> {
    let mut operations = operations();
    operations.sort_by_key(|op| op.route == "/api/v0/application" && op.method != Method::GET);
    operations
  }

  #[tokio::test]
  async fn every_route_is_answered_and_the_unbuilt_ones_say_so() {
    let app = app(CONFIG, &[]).await;
    let operations = in_order();
    assert!(operations.len() > 150, "{} operations", operations.len());
    for operation in &operations {
      let answer = answer(&app, operation).await;
      let routed = answer.status != 405 && !(answer.status == 404 && answer.body.is_empty());
      assert!(routed, "{operation:?} is not routed: {}", answer.status);
      if answered_here(operation) {
        continue;
      }
      assert_eq!(answer.status, 503, "{operation:?}: {:?}", answer.body);
      if operation.method == Method::HEAD {
        continue;
      }
      let body = answer.json();
      assert_eq!(body["error"], "soulseek_unavailable", "{operation:?}");
      let detail = body["detail"].as_str().unwrap_or_default();
      assert!(
        detail.ends_with(" not implemented yet"),
        "{operation:?}: {detail}"
      );
    }
  }

  #[tokio::test]
  async fn a_relay_agent_refuses_exactly_the_network_routes() {
    let agent = CONFIG.replace("mode = \"debug\"", "mode = \"agent\"")
      + "[relay.controller]\naddress = \"https://controller.lan:5031\"\n\
         api_key = \"controller-key-012345\"\nsecret = \"controller-secret-0123\"\n";
    let app = app(&agent, &[]).await;
    let operations = in_order();
    for (method, route) in DISABLED {
      let listed = operations
        .iter()
        .any(|op| op.method == *method && op.route == *route);
      assert!(listed, "{method} {route} is in no route");
    }
    for operation in &operations {
      let answer = answer(&app, operation).await;
      let refused = answer.status == 403
        && serde_json::from_slice::<Value>(&answer.body)
          .is_ok_and(|body| body["error"] == "relay_agent_mode");
      let disabled = DISABLED
        .iter()
        .any(|(method, route)| *method == operation.method && *route == operation.route);
      assert_eq!(refused, disabled, "{operation:?}: {}", answer.status);
    }
    let anonymous = app
      .send_from("192.0.2.1:1", request("GET", "/api/v0/searches", None))
      .await;
    assert_eq!(anonymous.status, 401, "who asks is settled first");
  }

  #[tokio::test]
  async fn each_route_takes_its_least_role() {
    let keys = "[web.authentication.api_keys.viewer]\nkey = \"viewer-key-0123456789\"\n\
                role = \"readonly\"\n[web.authentication.api_keys.editor]\n\
                key = \"editor-key-0123456789\"\nrole = \"readwrite\"\n";
    let app = app(&format!("{CONFIG}{keys}"), &[]).await;
    let roles = ["viewer-key-0123456789", "editor-key-0123456789", KEY];
    let (read, write, admin) = (0, 1, 2);
    let sample = [
      (Method::GET, "/api/v0/searches", read),
      (Method::POST, "/api/v0/searches", write),
      (Method::DELETE, "/api/v0/searches/{id}", write),
      (Method::GET, "/api/v0/transfers/downloads", read),
      (Method::POST, "/api/v0/transfers/downloads/batches", write),
      (
        Method::DELETE,
        "/api/v0/transfers/uploads/all/completed",
        write,
      ),
      (
        Method::POST,
        "/api/v0/transfers/uploads/{username}/{id}/bump",
        admin,
      ),
      (Method::GET, "/api/v0/users/{username}/browse", read),
      (Method::POST, "/api/v0/users/{username}/directory", read),
      (Method::PUT, "/api/v0/users/{username}/tags", write),
      (Method::PUT, "/api/v0/users/{username}/group", admin),
      (Method::DELETE, "/api/v0/users/{username}", admin),
      (Method::GET, "/api/v0/users/blocked", read),
      (Method::PUT, "/api/v0/users/{username}/block", write),
      (Method::POST, "/api/v0/users/blocked/import", admin),
      (Method::GET, "/api/v0/conversations", read),
      (Method::POST, "/api/v0/conversations/{username}", write),
      (Method::GET, "/api/v0/rooms/joined", read),
      (Method::POST, "/api/v0/rooms/joined", write),
      (Method::GET, "/api/v0/messages/search", read),
      (Method::GET, "/api/v0/shares", read),
      (Method::PUT, "/api/v0/shares", write),
      (Method::GET, "/api/v0/files/{root}/directories", read),
      (Method::DELETE, "/api/v0/files/{root}/files/{path}", write),
      (
        Method::POST,
        "/api/v0/files/{root}/files/{path}/analysis",
        write,
      ),
      (Method::GET, "/api/v0/events", read),
      (Method::POST, "/api/v0/events/{type}", admin),
      (
        Method::GET,
        "/api/v0/telemetry/reports/transfers/summary",
        read,
      ),
      (Method::GET, "/api/v0/server", read),
      (Method::PUT, "/api/v0/server", write),
      (Method::GET, "/api/v0/logs/live", admin),
      (Method::GET, "/api/v0/relay/agents", read),
      (Method::DELETE, "/api/v0/relay/agents/{name}", admin),
    ];
    for (method, route, least) in sample {
      let operation = Operation {
        method,
        route: route.to_string(),
      };
      let anonymous = app.send_from("192.0.2.1:1", probe(&operation)).await;
      assert_eq!(anonymous.status, 401, "{operation:?} without a credential");
      for (role, key) in roles.iter().enumerate() {
        let keyed = with(probe(&operation), "x-api-key", key);
        let status = app.send_from("192.0.2.1:1", keyed).await.status;
        if role < least {
          assert_eq!(status, 403, "{operation:?} with role {role}");
        } else {
          assert!(
            status != 401 && status != 403,
            "{operation:?} with role {role}: {status}"
          );
        }
      }
    }
    let force = |key: &str| {
      with(
        request(
          "GET",
          "/api/v0/application/version/latest?forceCheck=true",
          None,
        ),
        "x-api-key",
        key,
      )
    };
    assert_eq!(
      app
        .send_from("192.0.2.1:1", force(roles[read]))
        .await
        .status,
      403
    );
    assert_eq!(
      app
        .send_from("192.0.2.1:1", force(roles[write]))
        .await
        .status,
      503
    );
  }

  #[tokio::test]
  async fn requests_are_checked_before_any_service_is_asked() {
    let app = app(CONFIG, &[]).await;
    let batch = |extra: Value| {
      let mut body = json!({ "username": "alice", "files": [{ "filename": "a\\b", "size": 1 }] });
      body
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
      body
    };
    let histogram = "/api/v0/telemetry/reports/transfers/histogram";
    let leaderboard = "/api/v0/telemetry/reports/transfers/leaderboard";
    let cases: Vec<(&str, String, Option<Value>, u16)> = vec![
      ("GET", "/api/v0/searches/not-a-uuid".into(), None, 400),
      (
        "GET",
        "/api/v0/searches/{00000000-0000-4000-8000-000000000001}".into(),
        None,
        503,
      ),
      (
        "GET",
        "/api/v0/searches/0000000000004000800000000000000a".into(),
        None,
        503,
      ),
      ("GET", "/api/v0/searches?limit=0".into(), None, 400),
      (
        "POST",
        "/api/v0/searches".into(),
        Some(json!({ "searchText": "-live" })),
        400,
      ),
      (
        "POST",
        "/api/v0/searches".into(),
        Some(json!({ "searchText": "x", "searchTimeout": 15 })),
        400,
      ),
      (
        "POST",
        "/api/v0/searches".into(),
        Some(json!({ "searchText": "x", "searchTimeout": "20s" })),
        503,
      ),
      (
        "POST",
        "/api/v0/searches".into(),
        Some(json!({ "searchText": "x", "id": "" })),
        400,
      ),
      (
        "POST",
        "/api/v0/transfers/downloads/batches".into(),
        Some(batch(json!({ "id": "" }))),
        400,
      ),
      (
        "POST",
        "/api/v0/transfers/downloads/batches".into(),
        Some(batch(
          json!({ "id": "00000000-0000-0000-0000-000000000000" }),
        )),
        400,
      ),
      (
        "POST",
        "/api/v0/transfers/downloads/batches".into(),
        Some(batch(json!({ "options": { "destination": "/srv" } }))),
        400,
      ),
      (
        "POST",
        "/api/v0/transfers/downloads/batches".into(),
        Some(batch(json!({}))),
        503,
      ),
      (
        "POST",
        "/api/v0/transfers/downloads/alice".into(),
        Some(json!([null])),
        400,
      ),
      (
        "GET",
        "/api/v0/transfers/downloads/alice/42".into(),
        None,
        400,
      ),
      (
        "GET",
        "/api/v0/transfers/downloads/list?limit=1001".into(),
        None,
        400,
      ),
      (
        "GET",
        "/api/v0/transfers/downloads/list?state=failed".into(),
        None,
        400,
      ),
      (
        "GET",
        "/api/v0/transfers/downloads/list?state=Failed&state=Queued%2C%20Remotely".into(),
        None,
        503,
      ),
      ("GET", "/api/v0/users?limit=501".into(), None, 400),
      ("GET", "/api/v0/users?presence=online".into(), None, 400),
      (
        "GET",
        "/api/v0/users/alice/browse?username=".into(),
        None,
        400,
      ),
      (
        "PUT",
        "/api/v0/users/alice/tags".into(),
        Some(json!({ "tags": ["two words"] })),
        400,
      ),
      (
        "PUT",
        "/api/v0/users/alice/group".into(),
        Some(json!({ "group": "friends" })),
        400,
      ),
      (
        "PUT",
        "/api/v0/users/alice/block".into(),
        Some(json!({ "expiresIn": "1h", "expiresAt": "2030-01-01T00:00:00Z" })),
        400,
      ),
      (
        "PUT",
        "/api/v0/users/alice/block".into(),
        Some(json!({ "scopes": ["everything"] })),
        400,
      ),
      (
        "POST",
        "/api/v0/rooms/joined".into(),
        Some(json!({ "name": " indie" })),
        400,
      ),
      (
        "POST",
        "/api/v0/rooms/joined".into(),
        Some(json!("indie")),
        400,
      ),
      (
        "POST",
        "/api/v0/rooms/joined".into(),
        Some(json!({ "name": "indie", "private": true })),
        503,
      ),
      (
        "POST",
        "/api/v0/rooms/joined/indie/messages".into(),
        Some(json!("")),
        400,
      ),
      (
        "POST",
        "/api/v0/conversations/server".into(),
        Some(json!("hi")),
        400,
      ),
      ("PUT", "/api/v0/conversations/alice/seven".into(), None, 400),
      (
        "GET",
        "/api/v0/conversations/alice/messages?limit=1001".into(),
        None,
        400,
      ),
      (
        "GET",
        "/api/v0/files/elsewhere/directories".into(),
        None,
        404,
      ),
      (
        "GET",
        "/api/v0/files/downloads/directories/Li4v".into(),
        None,
        400,
      ),
      (
        "GET",
        "/api/v0/files/downloads/directories/%2Fw%3D%3D".into(),
        None,
        400,
      ),
      ("GET", "/api/v0/events?limit=0".into(), None, 400),
      ("GET", "/api/v0/events?type=Bogus".into(), None, 400),
      ("POST", "/api/v0/events/Any".into(), None, 400),
      (
        "GET",
        format!("{leaderboard}?direction=download&sortBy=totalbytes&sortOrder=asc"),
        None,
        503,
      ),
      (
        "GET",
        format!("{leaderboard}?direction=Download&sortBy=bogus"),
        None,
        400,
      ),
      ("GET", leaderboard.to_string(), None, 400),
      ("GET", format!("{histogram}?buckets=0"), None, 400),
      (
        "GET",
        format!("{histogram}?buckets=10&interval=60"),
        None,
        400,
      ),
      (
        "GET",
        format!("{histogram}?end=2020-01-01T00:00:00Z&start=2019-01-01T00:00:00Z"),
        None,
        400,
      ),
      (
        "GET",
        format!("{histogram}?start=2026-01-02T00:00:00Z&end=2026-01-01T00:00:00Z"),
        None,
        400,
      ),
      ("GET", format!("{histogram}?start=abc"), None, 400),
      ("GET", "/api/v0/logs/files/notes.txt".into(), None, 403),
      (
        "POST",
        "/api/v0/relay/controller/shares/not-a-token".into(),
        None,
        400,
      ),
    ];
    for (method, uri, body, status) in cases {
      let mut sent = request(method, &uri, body);
      if uri.starts_with("/api/v0/relay/") {
        sent = with(sent, "x-api-key", KEY);
      }
      let answer = app.send(sent).await;
      assert_eq!(answer.status, status, "{method} {uri}: {:?}", answer.body);
      if status == 400 {
        assert!(answer.json()["error"].is_string(), "{method} {uri}");
      }
    }
    let mut wrong_type = request("POST", "/api/v0/searches", None);
    *wrong_type.body_mut() = axum::body::Body::from("{\"searchText\":\"x\"}");
    let wrong_type = with(wrong_type, "content-type", "text/plain");
    assert_eq!(app.send(wrong_type).await.status, 415);
  }
}
