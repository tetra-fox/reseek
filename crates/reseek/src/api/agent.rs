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

/// The routes an agent does not serve: everything that searches, transfers, chats, joins rooms
/// or asks the server or a peer about a user, and connecting to the server. The user
/// directory, blocking, shares, files, events, telemetry, options, logs, sessions and tokens
/// are this instance's own data and stay.
const DISABLED: &[Disabled] = &[
  Disabled::Under("/api/v0/searches"),
  Disabled::Under("/api/v0/transfers"),
  Disabled::Under("/api/v0/conversations"),
  Disabled::Under("/api/v0/rooms"),
  Disabled::Under("/api/v0/messages"),
  Disabled::Route("/api/v0/users/{username}/endpoint"),
  Disabled::Route("/api/v0/users/{username}/browse"),
  Disabled::Route("/api/v0/users/{username}/browse/status"),
  Disabled::Route("/api/v0/users/{username}/directory"),
  Disabled::Route("/api/v0/users/{username}/info"),
  Disabled::Route("/api/v0/users/{username}/status"),
  Disabled::Route("/api/v0/users/{username}/statistics"),
  Disabled::Route("/api/v0/users/{username}/watch"),
  Disabled::Route("/api/v0/users/watched"),
  Disabled::Route("/api/v0/users/cache"),
  Disabled::Call(Method::PUT, "/api/v0/server"),
  Disabled::Call(Method::DELETE, "/api/v0/server"),
];

/// What one entry of [`DISABLED`] covers. A whole domain is a path prefix, so a route added to
/// it later is refused with the rest.
#[derive(Debug)]
enum Disabled {
  /// every route at or below this path, by any method
  Under(&'static str),
  /// this route, by any method
  Route(&'static str),
  /// this route by this method
  Call(Method, &'static str),
}

impl Disabled {
  fn covers(&self, method: &Method, route: &str) -> bool {
    match self {
      Self::Under(path) => route
        .strip_prefix(path)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
      Self::Route(path) => route == *path,
      Self::Call(by, path) => by == method && route == *path,
    }
  }
}

/// Whether an agent refuses `method` on `route`, the route as the router writes it, such as
/// `/api/v0/searches/{id}`.
fn disabled(method: &Method, route: &str) -> bool {
  DISABLED.iter().any(|entry| entry.covers(method, route))
}

/// Whether `options`, the ones the daemon started with, make this instance a relay agent.
pub fn is_agent(options: &Options) -> bool {
  options.relay.enabled && options.relay.mode == RelayMode::Agent
}

/// Answers an authenticated request to a route an agent does not serve with 403
/// `relay_agent_mode` when this instance is a relay agent. A request without a principal goes
/// on, to be refused with 401 by its handler.
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
  let refused = request
    .extensions()
    .get::<MatchedPath>()
    .is_some_and(|route| disabled(request.method(), route.as_str()));
  if authenticated && refused {
    return ApiError::disabled(
      "relay_agent_mode",
      "This instance is running as a relay agent; the Soulseek-facing API is disabled.",
    )
    .into_response();
  }
  next.run(request).await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::routes::{CONFIG, Operation, answer, in_order, operations};
  use crate::testing::{OUTSIDE, app, request};
  use serde_json::Value;

  #[test]
  fn every_entry_names_a_documented_route() {
    let operations = operations();
    for entry in DISABLED {
      let named = operations
        .iter()
        .any(|op| entry.covers(&op.method, &op.route));
      assert!(named, "{entry:?} covers no route");
    }
  }

  #[test]
  fn a_domain_prefix_stops_at_a_segment() {
    let searches = Disabled::Under("/api/v0/searches");
    assert!(searches.covers(&Method::GET, "/api/v0/searches"));
    assert!(searches.covers(&Method::DELETE, "/api/v0/searches/{id}"));
    assert!(!searches.covers(&Method::GET, "/api/v0/searchesx"));
  }

  /// The users routes that read or change only what this instance keeps about a user.
  const LOCAL_USERS: &[&str] = &[
    "/api/v0/users",
    "/api/v0/users/{username}",
    "/api/v0/users/{username}/summary",
    "/api/v0/users/{username}/profile",
    "/api/v0/users/{username}/picture",
    "/api/v0/users/{username}/transfers",
    "/api/v0/users/{username}/notes",
    "/api/v0/users/{username}/tags",
    "/api/v0/users/{username}/pin",
    "/api/v0/users/{username}/mute",
    "/api/v0/users/{username}/group",
  ];

  /// The tags whose every operation acts on the network.
  const NETWORK_TAGS: [&str; 5] = [
    "searches",
    "transfers",
    "conversations",
    "rooms",
    "messages",
  ];

  /// Whether a relay agent must refuse `operation`, read from the document: the operations of
  /// [`NETWORK_TAGS`], anything below `/transfers` whatever its tag, the users operations not in
  /// [`LOCAL_USERS`], and changing the server connection. A route added to one of these later is
  /// expected refused until it is listed here as local.
  fn needs_the_network(document: &Value, operation: &Operation) -> bool {
    let method = operation.method.as_str().to_lowercase();
    let tags = &document["paths"][&operation.route][&method]["tags"];
    let tagged = |name: &str| {
      tags
        .as_array()
        .is_some_and(|tags| tags.iter().any(|tag| tag == name))
    };
    NETWORK_TAGS.into_iter().any(tagged)
      || operation.route.starts_with("/api/v0/transfers/")
      || (tagged("users") && !LOCAL_USERS.contains(&operation.route.as_str()))
      || (tagged("server") && operation.method != Method::GET)
  }

  #[tokio::test]
  async fn a_relay_agent_refuses_exactly_the_network_routes() {
    let agent = CONFIG.replace("mode = \"debug\"", "mode = \"agent\"");
    let app = app(&agent, &[]).await;
    let document = serde_json::to_value(crate::api::openapi()).unwrap();
    let operations = in_order();
    for local in LOCAL_USERS {
      let listed = operations.iter().any(|op| op.route == *local);
      assert!(listed, "{local} is in no route");
    }
    for operation in &operations {
      let expected = needs_the_network(&document, operation);
      let answer = answer(&app, operation).await;
      // a HEAD answer has no body to name the refusal
      let refused = answer.status == 403
        && (operation.method == Method::HEAD
          || serde_json::from_slice::<Value>(&answer.body)
            .is_ok_and(|body| body["error"] == "relay_agent_mode"));
      assert_eq!(refused, expected, "{operation:?}: {}", answer.status);
    }
    let anonymous = app
      .send_from(OUTSIDE, request("GET", "/api/v0/searches", None))
      .await;
    assert_eq!(anonymous.status, 401, "who asks is settled first");
  }
}
