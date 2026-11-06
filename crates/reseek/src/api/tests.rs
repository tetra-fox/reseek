//! The router as a whole: the committed document is current, every route answers, every route
//! takes its least role, and requests are checked before any service is asked.
use axum::http::Method;
use serde_json::{Value, json};

use crate::testing::routes::{CONFIG, KEY, Operation, answer, in_order, probe};
use crate::testing::{app, request, with};

#[test]
fn the_committed_openapi_document_is_current() {
  let generated = crate::api::openapi().to_pretty_json().unwrap() + "\n";
  let problems = invalid(&serde_json::from_str(&generated).unwrap());
  assert!(problems.is_empty(), "{problems:#?}");
  let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../api/openapi.json");
  let committed = std::fs::read_to_string(path).unwrap_or_default();
  assert!(
    committed == generated,
    "api/openapi.json does not match the handlers; run `just api-spec`"
  );
}

/// What makes `document` unusable to a type generator: a `$ref` to a schema it does not list,
/// and an `operationId` two operations share.
fn invalid(document: &Value) -> Vec<String> {
  fn refs<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
      Value::Object(entries) => {
        for (key, v) in entries {
          match (key.as_str(), v) {
            ("$ref", Value::String(target)) => found.push(target),
            _ => refs(v, found),
          }
        }
      }
      Value::Array(items) => items.iter().for_each(|v| refs(v, found)),
      _ => {}
    }
  }
  let mut problems = Vec::new();
  let mut found = Vec::new();
  refs(document, &mut found);
  let schemas = &document["components"]["schemas"];
  for target in found {
    let listed = target
      .strip_prefix("#/components/schemas/")
      .is_some_and(|name| schemas.get(name).is_some());
    if !listed {
      problems.push(format!("{target} is not in the document"));
    }
  }
  let mut ids = std::collections::BTreeMap::<&str, Vec<String>>::new();
  let paths = document["paths"].as_object().into_iter().flatten();
  for (path, item) in paths {
    for (method, operation) in item.as_object().into_iter().flatten() {
      if let Some(id) = operation["operationId"].as_str() {
        ids.entry(id).or_default().push(format!("{method} {path}"));
      }
    }
  }
  for (id, operations) in ids {
    if operations.len() > 1 {
      problems.push(format!("operationId {id} names {}", operations.join(", ")));
    }
  }
  problems
}

#[test]
fn a_dangling_ref_and_a_repeated_operation_id_make_a_document_invalid() {
  let document = json!({
    "paths": {
      "/a": {
        "get": { "operationId": "list", "parameters": [{ "schema": { "$ref": "#/components/schemas/Sort" } }] },
        "head": { "operationId": "list" }
      },
      "/b": { "get": { "operationId": "list_b", "responses": { "200": { "$ref": "#/components/schemas/B" } } } }
    },
    "components": { "schemas": { "B": { "type": "string" } } }
  });
  assert_eq!(
    invalid(&document),
    [
      "#/components/schemas/Sort is not in the document",
      "operationId list names get /a, head /a"
    ]
  );
}

#[tokio::test]
async fn every_route_is_answered() {
  let app = app(CONFIG, &[]).await;
  let operations = in_order();
  assert!(operations.len() > 150, "{} operations", operations.len());
  for operation in &operations {
    let answer = answer(&app, operation).await;
    // a HEAD answer never has a body, so its 404 for a missing file looks like no route; its
    // GET twin is checked on its own
    let unrouted_404 =
      answer.status == 404 && answer.body.is_empty() && operation.method != Method::HEAD;
    let routed = answer.status != 405 && !unrouted_404;
    assert!(routed, "{operation:?} is not routed: {}", answer.status);
  }
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
    (Method::POST, "/api/v0/transfers/downloads/jobs", write),
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
    // messaging alice comes before the block of alice below refuses it
    (Method::GET, "/api/v0/conversations", read),
    (Method::POST, "/api/v0/conversations/{username}", write),
    (Method::GET, "/api/v0/users/{username}/browse", read),
    (Method::POST, "/api/v0/users/{username}/directory", read),
    (Method::PUT, "/api/v0/users/{username}/tags", write),
    (Method::PUT, "/api/v0/users/{username}/group", admin),
    (Method::DELETE, "/api/v0/users/{username}", admin),
    (Method::GET, "/api/v0/users/blocked", read),
    (Method::PUT, "/api/v0/users/{username}/block", write),
    (Method::POST, "/api/v0/users/blocked/import", admin),
    (Method::GET, "/api/v0/chat/pins", read),
    (Method::PUT, "/api/v0/chat/pins", write),
    (Method::GET, "/api/v0/rooms/joined", read),
    (Method::POST, "/api/v0/rooms/joined", write),
    (Method::GET, "/api/v0/messages/search", read),
    (Method::GET, "/api/v0/shares", read),
    (Method::GET, "/api/v0/profile", read),
    (Method::GET, "/api/v0/interests", read),
    (Method::PUT, "/api/v0/interests", admin),
    (Method::PUT, "/api/v0/application/password", admin),
    (Method::PUT, "/api/v0/shares", write),
    (Method::GET, "/api/v0/shares/folders", admin),
    (Method::GET, "/api/v0/files/{root}/directories", read),
    (Method::DELETE, "/api/v0/files/{root}/files/{path}", write),
    (
      Method::POST,
      "/api/v0/files/{root}/files/{path}/analysis",
      write,
    ),
    (Method::DELETE, "/api/v0/analysis/jobs", write),
    (Method::GET, "/api/v0/jobs", read),
    (Method::GET, "/api/v0/jobs/schedules", read),
    (Method::GET, "/api/v0/jobs/{id}", read),
    (Method::POST, "/api/v0/jobs/{id}/cancel", write),
    (Method::POST, "/api/v0/jobs/{id}/pause", write),
    (Method::DELETE, "/api/v0/jobs", write),
    (Method::POST, "/api/v0/jobs/schedules/{name}/run", write),
    (Method::GET, "/api/v0/searches/wishlist", read),
    (Method::POST, "/api/v0/searches/wishlist", write),
    (Method::POST, "/api/v0/transfers/uploads/{username}", write),
    (Method::GET, "/api/v0/events", read),
    (Method::GET, "/api/v0/activity", read),
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
    200,
    "a development build is not checked and answers its version"
  );
}

#[tokio::test]
async fn requests_are_checked_before_any_service_is_asked() {
  let app = app(CONFIG, &[]).await;
  let job = |extra: Value| {
    let mut group = json!({ "username": "alice", "files": [{ "filename": "a\\b", "size": 1 }] });
    group
      .as_object_mut()
      .unwrap()
      .extend(extra.as_object().unwrap().clone());
    json!({ "groups": [group] })
  };
  let histogram = "/api/v0/telemetry/reports/transfers/histogram";
  let leaderboard = "/api/v0/telemetry/reports/transfers/leaderboard";
  let cases: Vec<(&str, String, Option<Value>, u16)> = vec![
    ("GET", "/api/v0/searches/not-a-uuid".into(), None, 400),
    (
      "GET",
      "/api/v0/searches/{00000000-0000-4000-8000-000000000001}".into(),
      None,
      404,
    ),
    (
      "GET",
      "/api/v0/searches/0000000000004000800000000000000a".into(),
      None,
      404,
    ),
    ("GET", "/api/v0/searches?limit=0".into(), None, 400),
    (
      "POST",
      "/api/v0/searches".into(),
      Some(json!({ "searchText": "animals", "searchTimeout": "20s" })),
      409,
    ),
    (
      "POST",
      "/api/v0/searches".into(),
      Some(json!({ "searchText": "animals", "id": "" })),
      400,
    ),
    (
      "POST",
      "/api/v0/transfers/downloads/jobs".into(),
      Some(job(json!({ "searchId": "" }))),
      400,
    ),
    (
      "POST",
      "/api/v0/transfers/downloads/jobs".into(),
      Some(job(json!({ "destination": "/srv" }))),
      400,
    ),
    (
      "POST",
      "/api/v0/transfers/downloads/jobs".into(),
      Some(job(json!({}))),
      200,
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
      200,
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
      "PUT",
      "/api/v0/chat/pins".into(),
      Some(json!([{ "kind": "user", "name": "server" }])),
      400,
    ),
    (
      "PUT",
      "/api/v0/chat/pins".into(),
      Some(json!([
        { "kind": "room", "name": "jazz" },
        { "kind": "room", "name": "jazz" }
      ])),
      400,
    ),
    (
      "PUT",
      "/api/v0/chat/pins".into(),
      Some(json!([{ "kind": "user", "name": "a".repeat(501) }])),
      400,
    ),
    (
      "PUT",
      "/api/v0/chat/pins".into(),
      Some(json!(
        (0..101)
          .map(|n| json!({ "kind": "room", "name": format!("room{n}") }))
          .collect::<Vec<_>>()
      )),
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
      409,
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
      200,
    ),
    (
      "GET",
      format!("{leaderboard}?direction=Download&sortBy=bogus"),
      None,
      400,
    ),
    ("GET", format!("{histogram}?buckets=0"), None, 400),
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
