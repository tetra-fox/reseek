//! The event stream route: `GET /live` as server-sent events, for the topics asked for.

use api_types::auth::PrincipalSummary;
use api_types::live::{Hello, Lagged, LiveFrame, LiveQuery};
use axum::Extension;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use std::sync::Arc;
use tracing::{debug, info_span};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, Check, Presented, identify};
use crate::live::{self, Opening, Revalidate, topics};

/// The event stream route.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(stream))
}

/// The event stream: `hello`, a snapshot of each topic that has one, then every frame of the
/// topics as it happens. A client reconnecting with `Last-Event-ID` gets what it missed when it
/// is still kept, else fresh snapshots and `lagged`.
#[utoipa::path(
  get, path = "/live", tag = "live", params(LiveQuery),
  responses(
    (status = 200, body = LiveFrame, content_type = "text/event-stream"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody)
  )
)]
async fn stream(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Extension(presented): Extension<Presented>,
  Query(query): Query<LiveQuery>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let topics = topics::parse(query.topics.as_deref()).map_err(ApiError::bad_request)?;
  if let Some(topic) = topics
    .iter()
    .find(|topic| caller.principal.role < topic.role())
  {
    return Err(ApiError::forbidden(format!(
      "the {topic} topic needs the {:?} role",
      topic.role()
    )));
  }
  let asked = headers.get("last-event-id").map(|value| {
    value
      .to_str()
      .ok()
      .and_then(|text| text.trim().parse::<u64>().ok())
  });
  let limit = app
    .config
    .current()
    .values
    .web
    .live
    .max_streams_per_principal;
  let registered = app
    .live
    .open(Opening {
      topics: topics.clone(),
      holder: holder(&caller.principal, &presented),
      limit,
      last_seen: asked.flatten(),
    })
    .map_err(|_| ApiError::too_many("too many event streams open"))?;
  let listed: Vec<String> = topics.iter().map(ToString::to_string).collect();
  let span = info_span!(
    "live",
    stream = registered.id,
    principal = caller.principal.subject,
    topics = listed.join(",")
  );
  let mut frames = vec![LiveFrame::Hello(Hello {
    server_time: Utc::now(),
    version: app.state.current().version.clone(),
    principal: PrincipalSummary {
      name: caller.principal.name.clone(),
      role: caller.principal.role,
    },
    topics: listed,
    resumed: registered.resumed,
  })];
  if !registered.resumed {
    for topic in &topics {
      frames.extend(live::snapshot(*topic, &app).await);
    }
    if asked.is_some() {
      frames.push(LiveFrame::Lagged(Lagged {}));
    }
  }
  let first = frames
    .iter()
    .map(|frame| live::render(registered.at, frame))
    .collect::<Result<Vec<_>, _>>()
    .map_err(ApiError::internal)?;
  span.in_scope(|| {
    debug!(
      resumed = registered.resumed,
      at = registered.at,
      "event stream opened"
    )
  });
  let revalidate = recheck(&app, presented, caller.principal.subject.clone(), &topics);
  let body = span.in_scope(|| live::body(registered, first, revalidate));
  Ok(
    (
      StatusCode::OK,
      [
        (
          header::CONTENT_TYPE,
          HeaderValue::from_static("text/event-stream"),
        ),
        (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (
          header::HeaderName::from_static("x-accel-buffering"),
          HeaderValue::from_static("no"),
        ),
      ],
      body,
    )
      .into_response(),
  )
}

/// Whom the stream cap counts a stream against: the principal's subject, and for a trusted
/// network also the client's address, so the browsers of one network do not share one cap.
fn holder(principal: &api_types::auth::Principal, presented: &Presented) -> String {
  match principal.via {
    api_types::auth::Via::TrustedNetwork { .. } => {
      format!("{} {}", principal.subject, presented.client.ip)
    }
    _ => principal.subject.clone(),
  }
}

/// Checks again that what the stream's request presented still names the same principal with
/// a role that may follow every topic.
fn recheck(
  app: &AppState,
  presented: Presented,
  subject: String,
  topics: &[topics::Topic],
) -> Revalidate {
  let app = app.clone();
  let presented = Arc::new(presented);
  let needed = topics
    .iter()
    .map(|topic| topic.role())
    .max()
    .unwrap_or(api_types::auth::Role::ReadOnly);
  Arc::new(move || {
    let app = app.clone();
    let presented = presented.clone();
    let subject = subject.clone();
    Box::pin(async move {
      match identify(&app, &presented, Check::Again).await {
        Ok(identified) => identified
          .principal
          .is_some_and(|principal| principal.subject == subject && principal.role >= needed),
        Err(e) => {
          debug!(
            error = e.status().as_u16(),
            "event stream credential refused"
          );
          false
        }
      }
    })
  })
}

#[cfg(test)]
mod tests {
  use api_types::live::LiveFrame;
  use api_types::users::UserRemoved;
  use serde_json::json;

  use crate::testing::{TestApp, app, request, with};

  const TRUSTED: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n";

  fn removed(app: &TestApp, username: &str) {
    app.state.live.publish(LiveFrame::UsersRemoved(UserRemoved {
      username: username.to_string(),
    }));
  }

  async fn open(app: &TestApp, uri: &str) -> crate::testing::Stream {
    app
      .stream_from("127.0.0.1:1", request("GET", uri, None))
      .await
  }

  #[tokio::test]
  async fn a_stream_opens_with_hello_and_the_snapshots_in_the_order_asked() {
    let app = app(TRUSTED, &[]).await;
    let mut stream = open(&app, "/api/v0/live?topics=logs,application,users").await;
    assert_eq!(stream.status, 200);
    assert_eq!(stream.headers["content-type"], "text/event-stream");
    assert_eq!(stream.headers["cache-control"], "no-cache");
    assert_eq!(stream.headers["x-accel-buffering"], "no");
    let hello = stream.next().await.unwrap();
    assert_eq!(hello.event, "hello");
    assert_eq!(hello.data["resumed"], false);
    assert_eq!(
      hello.data["topics"],
      json!(["logs", "application", "users"])
    );
    assert_eq!(hello.data["principal"]["name"], "trusted network");
    let mut seen = Vec::new();
    for _ in 0..3 {
      let sse = stream.next().await.unwrap();
      assert_eq!(
        sse.id, hello.id,
        "snapshots carry the id the stream starts at"
      );
      seen.push(sse.event);
    }
    assert_eq!(
      seen,
      ["logs.buffer", "application.state", "application.options"]
    );
    removed(&app, "alice");
    let live = stream.next().await.unwrap();
    assert_eq!(live.event, "users.removed");
    assert_eq!(live.data, json!({ "username": "alice" }));
    assert!(live.id > hello.id);
    let empty = open(&app, "/api/v0/live")
      .await
      .until("application.options")
      .await;
    assert_eq!(
      empty[0].data["topics"],
      json!(["application"]),
      "application by default"
    );
  }

  #[tokio::test]
  async fn a_reconnect_replays_what_it_missed_or_gets_snapshots_and_lagged() {
    let app = app(TRUSTED, &[]).await;
    let mut first = open(&app, "/api/v0/live?topics=users").await;
    let start = first.next().await.unwrap().id.unwrap();
    assert!(
      (1 << 32..1 << 48).contains(&start),
      "the sequence starts at a random number"
    );
    removed(&app, "a");
    let seen = first.next().await.unwrap().id.unwrap();
    drop(first);
    removed(&app, "b");
    removed(&app, "c");
    let mut again = app
      .stream_from(
        "127.0.0.1:1",
        with(
          request("GET", "/api/v0/live?topics=users", None),
          "last-event-id",
          &seen.to_string(),
        ),
      )
      .await;
    let hello = again.next().await.unwrap();
    assert_eq!(hello.data["resumed"], true);
    assert_eq!(hello.id, Some(seen));
    let b = again.next().await.unwrap();
    let c = again.next().await.unwrap();
    assert_eq!(
      (b.data["username"].clone(), c.data["username"].clone()),
      (json!("b"), json!("c"))
    );
    assert!(b.id.unwrap() > seen && c.id > b.id);

    let mut late = app
      .stream_from(
        "127.0.0.1:1",
        with(
          request("GET", "/api/v0/live?topics=application,users", None),
          "last-event-id",
          // below every number a sequence starts at, so older than anything the ring keeps
          &((1u64 << 32) - 1).to_string(),
        ),
      )
      .await;
    let frames = late.until("lagged").await;
    let kinds: Vec<&str> = frames.iter().map(|f| f.event.as_str()).collect();
    assert_eq!(
      kinds,
      [
        "hello",
        "application.state",
        "application.options",
        "lagged"
      ]
    );
    assert_eq!(frames[0].data["resumed"], false);
  }

  #[tokio::test]
  async fn a_stream_that_falls_too_far_behind_is_told_to_reconnect() {
    let app = app(TRUSTED, &[]).await;
    let mut stream = open(&app, "/api/v0/live?topics=users").await;
    stream.next().await.unwrap();
    for n in 0..1200 {
      removed(&app, &n.to_string());
    }
    let frames = stream.until("bye").await;
    assert_eq!(frames.last().unwrap().data, json!({ "reason": "lagged" }));
    assert!(frames.len() < 1200);
    assert!(stream.next().await.is_none(), "and it ends");
  }

  #[tokio::test]
  async fn topics_are_checked_before_the_stream_starts() {
    let app = app(
      &format!("{TRUSTED}[web.live]\nmax_streams_per_principal = 1\n"),
      &[],
    )
    .await;
    assert_eq!(open(&app, "/api/v0/live?topics=hubs").await.status, 400);
    assert_eq!(open(&app, "/api/v0/live?topics=search:7").await.status, 400);
    let viewer = app
      .send(request(
        "POST",
        "/api/v0/tokens",
        Some(json!({ "name": "viewer", "role": "ReadOnly" })),
      ))
      .await
      .json()["secret"]
      .as_str()
      .unwrap()
      .to_string();
    let logs = app
      .stream_from(
        "192.0.2.1:1",
        request(
          "GET",
          &format!("/api/v0/live?topics=logs&access_token={viewer}"),
          None,
        ),
      )
      .await;
    assert_eq!(logs.status, 403, "the log is for administrators");
    let mut held = open(&app, "/api/v0/live").await;
    assert_eq!(held.status, 200);
    held.next().await.unwrap();
    assert_eq!(open(&app, "/api/v0/live").await.status, 429);
    let neighbor = app
      .stream_from("127.0.0.2:1", request("GET", "/api/v0/live", None))
      .await;
    assert_eq!(
      neighbor.status, 200,
      "another address of the trusted network has its own cap"
    );
    drop(held);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
      open(&app, "/api/v0/live").await.status,
      200,
      "a closed stream frees its place"
    );
    let anonymous = app
      .stream_from("192.0.2.1:1", request("GET", "/api/v0/live", None))
      .await;
    assert_eq!(anonymous.status, 401);
  }

  #[tokio::test]
  async fn state_changes_events_and_shutdown_reach_the_stream() {
    let app = app(TRUSTED, &[]).await;
    let mut stream = open(&app, "/api/v0/live?topics=application,events").await;
    stream.until("application.options").await;
    std::fs::write(app.dir.path().join("config.toml"), "[web\n").unwrap();
    assert!(app.state.config.reload().await.is_err());
    // other changes, such as the health sampled as the clock starts, are states too
    loop {
      let state = stream.until("application.state").await.pop().unwrap();
      if state.data["configuration"]["lastReloadError"].is_string() {
        break;
      }
    }
    let failed = app
      .send_from(
        "192.0.2.1:1",
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": "x", "password": "y" })),
        ),
      )
      .await;
    assert_eq!(failed.status, 401);
    let raised = stream.until("events.raised").await.pop().unwrap();
    assert_eq!(raised.data["type"], "LoginFailed");
    assert_eq!(raised.data["data"]["provider"], "local");
    app.state.shutdown.cancel();
    let bye = stream.until("bye").await.pop().unwrap();
    assert_eq!(bye.data, json!({ "reason": "shutdown" }));
  }

  #[tokio::test]
  async fn a_streams_credential_is_checked_again() {
    let app = app(TRUSTED, &[]).await;
    let created = app
      .send(request(
        "POST",
        "/api/v0/tokens",
        Some(json!({ "name": "stream", "role": "ReadOnly" })),
      ))
      .await
      .json();
    let secret = created["secret"].as_str().unwrap();
    let current = app.state.config.current();
    let peer = crate::http::Peer {
      remote: Some("192.0.2.1:1".parse().unwrap()),
      transport: crate::http::Transport::Plain,
    };
    let uri: axum::http::Uri = format!("/api/v0/live?access_token={secret}")
      .parse()
      .unwrap();
    let presented =
      crate::auth::Presented::read(&peer, &uri, &axum::http::HeaderMap::new(), &current.values);
    let subject = format!("token:{}", created["token"]["id"].as_str().unwrap());
    let check = super::recheck(&app.state, presented, subject, &[crate::live::Topic::Users]);
    assert!(check().await);
    let id = created["token"]["id"].as_str().unwrap();
    app
      .send(request("DELETE", &format!("/api/v0/tokens/{id}"), None))
      .await;
    assert!(!check().await, "a revoked token ends the stream");
  }

  #[tokio::test]
  async fn transfer_figures_go_out_each_second_they_changed_while_a_stream_follows_them() {
    use std::time::Duration;
    let app = app(TRUSTED, &[]).await;
    let succeeded = |app: &TestApp| {
      app
        .state
        .telemetry
        .metrics()
        .transfers()
        .downloads
        .succeeded(100, 10.0);
    };
    let noop = |app: &TestApp| {
      app.state.events.raise(api_types::events::Event::Noop(
        api_types::events::NoFields { version: 0 },
      ))
    };
    let mut events = open(&app, "/api/v0/live?topics=events").await;
    let seen = events.next().await.unwrap().id.unwrap();
    succeeded(&app);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let before = noop(&app);
    let mut figures = app
      .stream_from(
        "127.0.0.1:1",
        with(
          request("GET", "/api/v0/live?topics=metrics,events", None),
          "last-event-id",
          &seen.to_string(),
        ),
      )
      .await;
    assert_eq!(figures.next().await.unwrap().data["resumed"], true);
    let replayed = figures.next().await.unwrap();
    assert_eq!(
      (replayed.event.as_str(), replayed.data["id"].clone()),
      ("events.raised", json!(before.id)),
      "nothing went out while no stream followed the figures"
    );
    let changed = figures.next().await.unwrap();
    assert_eq!(changed.event, "metrics.update");
    assert_eq!(changed.data["downloads"]["completed"]["succeeded"], 1);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let after = noop(&app);
    let next = figures.next().await.unwrap();
    assert_eq!(
      (next.event.as_str(), next.data["id"].clone()),
      ("events.raised", json!(after.id)),
      "figures that did not change are not sent again"
    );
    succeeded(&app);
    let started = std::time::Instant::now();
    let again = figures.next().await.unwrap();
    assert_eq!(again.event, "metrics.update");
    assert_eq!(again.data["downloads"]["completed"]["succeeded"], 2);
    assert!(
      started.elapsed() < Duration::from_millis(1500),
      "within the second"
    );

    let mut fresh = open(&app, "/api/v0/live?topics=metrics").await;
    let frames = fresh.until("metrics.update").await;
    assert_eq!(frames.len(), 2, "the figures are the topic's snapshot");
    assert_eq!(frames[1].data["downloads"]["completed"]["bytes"], 200);
  }
}
