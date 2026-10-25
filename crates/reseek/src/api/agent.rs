//! Relay agent mode. An agent never connects to the Soulseek server, so the routes that act on
//! the network answer 403 `relay_agent_mode` there instead; the rest of the API stays. This is
//! the one place that says which routes those are.

use axum::extract::{MatchedPath, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::error::ApiError;
use crate::app::AppState;
use crate::auth::Authentication;
use crate::config::Options;
use crate::config::enums::RelayMode;

/// The routes an agent does not serve, by method and route.
pub const DISABLED: &[(Method, &str)] = &[
  (Method::PUT, "/api/v0/server"),
  (Method::DELETE, "/api/v0/server"),
  (Method::GET, "/api/v0/conversations"),
  (Method::GET, "/api/v0/conversations/{username}"),
  (Method::POST, "/api/v0/conversations/{username}"),
  (Method::PUT, "/api/v0/conversations/{username}"),
  (Method::DELETE, "/api/v0/conversations/{username}"),
  (Method::GET, "/api/v0/conversations/{username}/messages"),
  (Method::PUT, "/api/v0/conversations/{username}/{id}"),
  (Method::GET, "/api/v0/rooms/available"),
  (Method::GET, "/api/v0/rooms/joined"),
  (Method::POST, "/api/v0/rooms/joined"),
  (Method::GET, "/api/v0/rooms/joined/{roomName}"),
  (Method::DELETE, "/api/v0/rooms/joined/{roomName}"),
  (Method::GET, "/api/v0/rooms/joined/{roomName}/messages"),
  (Method::POST, "/api/v0/rooms/joined/{roomName}/messages"),
  (Method::POST, "/api/v0/rooms/joined/{roomName}/ticker"),
  (Method::POST, "/api/v0/rooms/joined/{roomName}/members"),
  (Method::GET, "/api/v0/rooms/joined/{roomName}/users"),
  (Method::GET, "/api/v0/searches"),
  (Method::POST, "/api/v0/searches"),
  (Method::GET, "/api/v0/searches/{id}"),
  (Method::PUT, "/api/v0/searches/{id}"),
  (Method::DELETE, "/api/v0/searches/{id}"),
  (Method::GET, "/api/v0/searches/{id}/responses"),
  (Method::GET, "/api/v0/transfers/downloads"),
  (Method::DELETE, "/api/v0/transfers/downloads/all/completed"),
  (Method::POST, "/api/v0/transfers/downloads/batches"),
  (Method::GET, "/api/v0/transfers/downloads/batches/{id}"),
  (Method::GET, "/api/v0/transfers/downloads/{username}"),
  (Method::POST, "/api/v0/transfers/downloads/{username}"),
  (Method::GET, "/api/v0/transfers/downloads/{username}/{id}"),
  (
    Method::DELETE,
    "/api/v0/transfers/downloads/{username}/{id}",
  ),
  (
    Method::GET,
    "/api/v0/transfers/downloads/{username}/{id}/position",
  ),
  (Method::GET, "/api/v0/transfers/uploads"),
  (Method::DELETE, "/api/v0/transfers/uploads/all/completed"),
  (Method::GET, "/api/v0/transfers/uploads/{username}"),
  (Method::GET, "/api/v0/transfers/uploads/{username}/{id}"),
  (Method::DELETE, "/api/v0/transfers/uploads/{username}/{id}"),
  (Method::GET, "/api/v0/users/{username}/browse"),
  (Method::GET, "/api/v0/users/{username}/browse/status"),
  (Method::POST, "/api/v0/users/{username}/directory"),
  (Method::GET, "/api/v0/users/{username}/endpoint"),
  (Method::GET, "/api/v0/users/{username}/info"),
  (Method::GET, "/api/v0/users/{username}/status"),
];

/// Whether `options`, the ones the daemon started with, make this instance a relay agent.
pub fn is_agent(options: &Options) -> bool {
  options.relay.enabled && options.relay.mode == RelayMode::Agent
}

/// Answers an authenticated request to a route in [`DISABLED`] with 403 `relay_agent_mode`
/// when this instance is a relay agent. A request without a principal goes on, to be refused
/// with 401 by its handler.
pub async fn refuse_in_agent_mode(
  State(app): State<AppState>,
  request: Request,
  next: Next,
) -> Response {
  if !is_agent(&app.startup.values) {
    return next.run(request).await;
  }
  let authenticated = matches!(
    request.extensions().get::<Authentication>(),
    Some(Authentication(Some(_)))
  );
  let disabled = request
    .extensions()
    .get::<MatchedPath>()
    .is_some_and(|route| {
      DISABLED
        .iter()
        .any(|(method, path)| method == request.method() && *path == route.as_str())
    });
  if authenticated && disabled {
    return ApiError::disabled(
      "relay_agent_mode",
      "This instance is running as a relay agent; the Soulseek-facing API is disabled.",
    )
    .into_response();
  }
  next.run(request).await
}
