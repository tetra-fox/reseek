//! Every operation of the API document, and a request for each that its checks let through:
//! the path parameters, the queries and the bodies a route needs, with the relay's headers.

use axum::body::Body;
use axum::http::{Method, Request};
use serde_json::{Value, json};

use super::{request, with};

/// The API key [`CONFIG`] gives an administrator, for the routes an agent calls.
pub const KEY: &str = "robot-key-0123456789";

/// A configuration every route answers under: loopback is a trusted administrator, the relay
/// runs as both of its sides, and every switch a route checks is on.
pub const CONFIG: &str = "debug = true\nremote_configuration = true\nremote_file_management = \
                          true\n[relay]\nenabled = true\nmode = \"debug\"\n\
                          [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                          [web.authentication.api_keys.robot]\nkey = \"robot-key-0123456789\"\n\
                          role = \"administrator\"\n";

const ID: &str = "00000000-0000-4000-8000-000000000001";

/// One operation: its method and its route as the document writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
  /// the method
  pub method: Method,
  /// the route, such as `/api/v0/searches/{id}`
  pub route: String,
}

/// Every operation in the API document.
pub fn operations() -> Vec<Operation> {
  let document = serde_json::to_value(crate::api::openapi()).unwrap();
  let mut operations = Vec::new();
  for (route, item) in document["paths"].as_object().unwrap() {
    for method in ["get", "put", "post", "delete", "patch", "head"] {
      if item.get(method).is_some() {
        operations.push(Operation {
          method: method.to_uppercase().parse().unwrap(),
          route: route.clone(),
        });
      }
    }
  }
  operations
}

/// A request for `operation` that the route's checks let through.
pub fn probe(operation: &Operation) -> Request<Body> {
  let route = &operation.route;
  let mut uri = String::new();
  for segment in route.split('/').skip(1) {
    uri.push('/');
    uri.push_str(&match segment {
      "{username}" => "alice".to_string(),
      "{roomName}" => "indie".to_string(),
      "{root}" => "downloads".to_string(),
      "{path}" => "QWxidW0".to_string(),
      "{filename}" => "reseek.log".to_string(),
      "{type}" => "Noop".to_string(),
      "{name}" => "home".to_string(),
      "{key}" => "rooms".to_string(),
      "{id}" if route.starts_with("/api/v0/conversations/") => "7".to_string(),
      "{id}" | "{token}" => ID.to_string(),
      other => other.to_string(),
    });
  }
  if let Some(query) = query(route) {
    uri.push('?');
    uri.push_str(query);
  }
  let method = operation.method.as_str();
  let mut request = match body(&operation.method, route) {
    Some(Payload::Json(value)) => request(method, &uri, Some(value)),
    Some(Payload::Text(text)) => {
      let mut r = request(method, &uri, None);
      *r.body_mut() = Body::from(text);
      with(r, "content-type", "text/plain")
    }
    Some(Payload::Form) => {
      let form = "--x\r\nContent-Disposition: form-data; name=\"shares\"\r\n\r\n[]\r\n--x--\r\n";
      let mut r = request(method, &uri, None);
      *r.body_mut() = Body::from(form);
      with(r, "content-type", "multipart/form-data; boundary=x")
    }
    None => request(method, &uri, None),
  };
  if route.starts_with("/api/v0/relay/controller/") || route == "/api/v0/relay/ws" {
    request = with(request, "x-api-key", KEY);
    request = with(request, "x-relay-agent", "nas");
    request = with(request, "x-relay-credential", "c0ffee");
    request = with(request, "x-relay-filename-base64", "QWxidW0vMDEuZmxhYw==");
  }
  request
}

/// What a request carries.
enum Payload {
  Json(Value),
  Text(&'static str),
  Form,
}

/// The query a route needs to be let through.
fn query(route: &str) -> Option<&'static str> {
  match route {
    "/api/v0/telemetry/reports/transfers/leaderboard"
    | "/api/v0/telemetry/reports/transfers/exceptions"
    | "/api/v0/telemetry/reports/transfers/exceptions/pareto" => Some("direction=Download"),
    "/api/v0/messages/search" => Some("q=hello"),
    route if route.ends_with("/export") => Some("format=json"),
    _ => None,
  }
}

/// The body a route needs to be let through.
fn body(method: &Method, route: &str) -> Option<Payload> {
  let json = |value| Some(Payload::Json(value));
  let downloads = "/api/v0/transfers/downloads";
  match (method.as_str(), route) {
    ("POST", "/api/v0/searches") => json(json!({ "searchText": "animals" })),
    ("POST", r) if r == format!("{downloads}/{{username}}") => {
      json(json!([{ "filename": "a\\b.flac", "size": 1 }]))
    }
    ("POST", r) if r == format!("{downloads}/batches") => json(json!({
      "username": "alice", "files": [{ "filename": "a\\b.flac", "size": 1 }]
    })),
    ("POST", r) if r.ends_with("/bulk") => json(json!({ "ids": [ID], "action": "cancel" })),
    ("POST", "/api/v0/users/{username}/directory") => json(json!({ "directory": "@@a\\Music" })),
    ("PUT", "/api/v0/users/{username}/notes") => json(json!({ "notes": "kind" })),
    ("PUT", "/api/v0/users/{username}/tags") => json(json!({ "tags": ["jazz"] })),
    ("PUT", "/api/v0/users/{username}/group") => json(json!({ "group": null })),
    ("PUT", "/api/v0/users/{username}/block") => json(json!({})),
    ("POST", "/api/v0/users/blocked/import") => Some(Payload::Text("alice\n")),
    ("POST", "/api/v0/conversations/{username}") => json(json!("hello")),
    ("PUT", r) if r.ends_with("/notify") => json(json!({ "notify": "all" })),
    ("PUT", "/api/v0/conversations/{username}/draft") => json(json!({ "draft": "hel" })),
    ("POST", "/api/v0/rooms/joined") => json(json!({ "name": "indie" })),
    ("POST", "/api/v0/rooms/joined/{roomName}/messages") => json(json!("hello")),
    ("POST", "/api/v0/rooms/joined/{roomName}/ticker") => json(json!("listening")),
    ("POST", r) if r.ends_with("/members") || r.ends_with("/operators") => json(json!("bob")),
    ("PUT", "/api/v0/rooms/joined/{roomName}/read") => json(json!({ "localId": 1 })),
    ("PUT", r) if r.ends_with("/analysis/override") => json(json!({ "verdict": "legit" })),
    ("POST", "/api/v0/session/file-token") => json(json!({ "transferId": ID })),
    ("POST", r) if r.starts_with("/api/v0/relay/controller/") => Some(Payload::Form),
    ("PATCH", "/api/v0/options") => json(json!({})),
    ("POST", "/api/v0/options/file/validate") => json(json!({ "text": "" })),
    ("POST", "/api/v0/session") => json(json!({ "username": "admin", "password": "x" })),
    ("POST", "/api/v0/tokens") => json(json!({ "name": "script", "role": "ReadOnly" })),
    ("PUT", "/api/v0/tokens/{id}") => json(json!({})),
    _ => None,
  }
}
