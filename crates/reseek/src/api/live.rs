//! The event stream route: `GET /live` as server-sent events, for the topics asked for.

use api_types::auth::{PrincipalSummary, Role};
use api_types::live::{ByeReason, Hello, Lagged, LiveFrame, LiveQuery};
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
use crate::auth::{AnyRole, Authorized, Check, Presented, identify, is_interactive};
use crate::live::{self, Opening, Revalidate, topics};

/// The event stream route.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(stream))
}

/// The event stream: `hello`, a snapshot of each topic that has one, then every frame of the
/// topics as it happens. A client reconnecting with `Last-Event-ID` gets what it missed when it
/// is still kept and the daemon has not restarted since, else fresh snapshots and `lagged`.
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
    return Err(ApiError::topic_forbidden(&topic.to_string()));
  }
  let asked = headers
    .get("last-event-id")
    .map(|value| value.to_str().ok().and_then(|id| app.live.seen(id.trim())));
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
      reopen: query.reopen,
      person: is_interactive(&caller.principal),
    })
    .await
    .map_err(|_| ApiError::too_many("Too many event streams are open"))?;
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
    epoch: app.live.epoch().to_string(),
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
    .map(|frame| registered.render(registered.at, frame))
    .collect::<Result<Vec<_>, _>>()
    .map_err(ApiError::internal)?;
  span.in_scope(|| {
    debug!(
      resumed = registered.resumed,
      at = registered.at,
      "event stream opened"
    )
  });
  // the topics were checked against the role it opens with, and the page against it too
  let opened = caller.principal.role;
  let revalidate = recheck(
    &app,
    presented,
    caller.principal.subject.clone(),
    move |role| {
      if role == opened {
        Ok(())
      } else {
        Err(ByeReason::RoleChanged)
      }
    },
  );
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

/// Checks again that what a long-lived request presented, the event stream's or an agent's
/// socket, still names the principal `subject` with a role `accept` takes. A session whose user
/// has no role right now is withheld rather than unauthorized: it goes on, and is let in again
/// once the configuration gives a role back.
pub(super) fn recheck(
  app: &AppState,
  presented: Presented,
  subject: String,
  accept: impl Fn(Role) -> Result<(), ByeReason> + Send + Sync + 'static,
) -> Revalidate {
  let app = app.clone();
  let presented = Arc::new(presented);
  let accept = Arc::new(accept);
  Arc::new(move || {
    let app = app.clone();
    let presented = presented.clone();
    let subject = subject.clone();
    let accept = accept.clone();
    Box::pin(async move {
      match identify(&app, &presented, Check::Again).await {
        Ok(identified) if identified.withheld => Err(ByeReason::Withheld),
        Ok(identified) => match identified.principal {
          Some(principal) if principal.subject == subject => accept(principal.role),
          _ => Err(ByeReason::Unauthorized),
        },
        // logged under the span of the stream or the agent's socket that checks it
        Err(e) => {
          debug!(
            error = e.status().as_u16(),
            "the credential checked again was refused"
          );
          Err(ByeReason::Unauthorized)
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

  use crate::testing::{OUTSIDE, TRUSTED, TestApp, app, request, with};

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
    assert!(live.seq() > hello.seq());
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
    let hello = first.next().await.unwrap();
    let epoch = hello.data["epoch"].as_str().unwrap();
    assert_eq!(
      hello.id,
      Some(format!("{epoch}-{}", hello.seq())),
      "an id is the run's epoch and the sequence number"
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
          &seen,
        ),
      )
      .await;
    let hello = again.next().await.unwrap();
    assert_eq!(hello.data["resumed"], true);
    assert_eq!(hello.id.as_ref(), Some(&seen));
    let b = again.next().await.unwrap();
    let c = again.next().await.unwrap();
    assert_eq!(
      (b.data["username"].clone(), c.data["username"].clone()),
      (json!("b"), json!("c"))
    );
    assert!(b.seq() > hello.seq() && c.seq() > b.seq());

    // a number a ring this long cannot reach, the same number from another run of the daemon,
    // and an id from before ids named the run
    let (epoch, number) = seen.rsplit_once('-').unwrap();
    for unknown in [
      format!("{epoch}-1000000"),
      format!("zzzzzzzzzzzzzzzz-{number}"),
      number.to_string(),
    ] {
      let mut late = app
        .stream_from(
          "127.0.0.1:1",
          with(
            request("GET", "/api/v0/live?topics=application,users", None),
            "last-event-id",
            &unknown,
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
        ],
        "{unknown}"
      );
      assert_eq!(frames[0].data["resumed"], false, "{unknown}");
    }
  }

  #[tokio::test]
  async fn a_stream_of_the_daemon_before_a_restart_is_not_resumed() {
    let before = app(TRUSTED, &[]).await;
    let mut stream = open(&before, "/api/v0/live?topics=users").await;
    let greeted = stream.next().await.unwrap();
    removed(&before, "a");
    let seen = stream.next().await.unwrap();
    let after = app(TRUSTED, &[]).await;
    // the new run has published past the number seen, so its ring holds that number
    for n in 0..=seen.seq() {
      removed(&after, &n.to_string());
    }
    let mut again = after
      .stream_from(
        "127.0.0.1:1",
        with(
          request("GET", "/api/v0/live?topics=users", None),
          "last-event-id",
          seen.id.as_deref().unwrap(),
        ),
      )
      .await;
    let frames = again.until("lagged").await;
    assert_eq!(frames[0].data["resumed"], false);
    assert_ne!(frames[0].data["epoch"], greeted.data["epoch"]);
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
      .send_from(
        OUTSIDE,
        request(
          "GET",
          &format!("/api/v0/live?topics=users,logs&access_token={viewer}"),
          None,
        ),
      )
      .await;
    assert_eq!(logs.status, 403, "the log is for administrators");
    assert_eq!(
      logs.json(),
      json!({ "error": "topic_forbidden", "detail": "logs" }),
      "the answer names the topic, so a client can ask again without it"
    );
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
      .stream_from(OUTSIDE, request("GET", "/api/v0/live", None))
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
        OUTSIDE,
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
      remote: Some(OUTSIDE.parse().unwrap()),
      transport: crate::http::Transport::Plain,
    };
    let uri: axum::http::Uri = format!("/api/v0/live?access_token={secret}")
      .parse()
      .unwrap();
    let presented =
      crate::auth::Presented::read(&peer, &uri, &axum::http::HeaderMap::new(), &current.values);
    let subject = format!("token:{}", created["token"]["id"].as_str().unwrap());
    let check = super::recheck(&app.state, presented, subject, |_| Ok(()));
    assert_eq!(check().await, Ok(()));
    let id = created["token"]["id"].as_str().unwrap();
    app
      .send(request("DELETE", &format!("/api/v0/tokens/{id}"), None))
      .await;
    assert_eq!(
      check().await,
      Err(api_types::live::ByeReason::Unauthorized),
      "a revoked token ends the stream"
    );
  }

  #[tokio::test]
  async fn a_revoked_token_or_session_ends_its_streams_at_once() {
    let app = app(TRUSTED, &[]).await;
    let mut bystander = open(&app, "/api/v0/live?topics=users").await;
    bystander.next().await.unwrap();
    let token = app
      .send(request(
        "POST",
        "/api/v0/tokens",
        Some(json!({ "name": "stream", "role": "ReadOnly" })),
      ))
      .await
      .json();
    let secret = token["secret"].as_str().unwrap();
    let mut by_token = app
      .stream_from(
        OUTSIDE,
        request(
          "GET",
          &format!("/api/v0/live?topics=users&access_token={secret}"),
          None,
        ),
      )
      .await;
    by_token.next().await.unwrap();
    let login = app
      .send_from(
        OUTSIDE,
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": "reseek", "password": "reseek" })),
        ),
      )
      .await;
    let cookie = crate::testing::set_cookie(&login.headers, "reseek_session").unwrap();
    let mut by_cookie = app
      .stream_from(
        OUTSIDE,
        with(
          request("GET", "/api/v0/live?topics=users", None),
          "cookie",
          &cookie,
        ),
      )
      .await;
    by_cookie.next().await.unwrap();

    // well inside the 30 s between the checks a stream makes by itself
    let started = tokio::time::Instant::now();
    let id = token["token"]["id"].as_str().unwrap();
    app
      .send(request("DELETE", &format!("/api/v0/tokens/{id}"), None))
      .await;
    let bye = by_token.until("bye").await.pop().unwrap();
    assert_eq!(bye.data, json!({ "reason": "unauthorized" }));
    let session = cookie.split_once('=').unwrap().1.split_once('.').unwrap().0;
    app
      .send(request(
        "DELETE",
        &format!("/api/v0/sessions/{session}"),
        None,
      ))
      .await;
    let bye = by_cookie.until("bye").await.pop().unwrap();
    assert_eq!(bye.data, json!({ "reason": "unauthorized" }));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));

    removed(&app, "alice");
    assert_eq!(
      bystander.next().await.unwrap().event,
      "users.removed",
      "a stream of another credential carries on"
    );
  }

  #[tokio::test]
  async fn a_role_changed_or_gone_in_the_configuration_ends_its_streams_at_once() {
    let account = |role: Option<&str>| match role {
      Some(role) => {
        format!("[web.authentication.local.users.alice]\npassword = \"pw\"\nrole = \"{role}\"\n")
      }
      None => String::new(),
    };
    let app = app(&account(Some("administrator")), &[]).await;
    let login = app
      .send_from(
        OUTSIDE,
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": "alice", "password": "pw" })),
        ),
      )
      .await;
    let cookie = crate::testing::set_cookie(&login.headers, "reseek_session").unwrap();
    let open = async || {
      let mut stream = app
        .stream_from(
          OUTSIDE,
          with(
            request("GET", "/api/v0/live?topics=users", None),
            "cookie",
            &cookie,
          ),
        )
        .await;
      assert_eq!(stream.status, 200);
      stream.next().await.unwrap();
      stream
    };
    // well inside the 30 s between the checks a stream makes by itself
    let ends_with = async |role: Option<&str>, reason: &str| {
      let mut stream = open().await;
      let started = tokio::time::Instant::now();
      std::fs::write(app.dir.path().join("config.toml"), account(role)).unwrap();
      app.state.config.reload().await.unwrap();
      let bye = stream.until("bye").await.pop().unwrap();
      assert_eq!(bye.data, json!({ "reason": reason }), "{role:?}");
      assert!(started.elapsed() < std::time::Duration::from_secs(5));
    };
    // the topics still fit the lower role, and the stream ends all the same
    ends_with(Some("readonly"), "roleChanged").await;
    ends_with(Some("readwrite"), "roleChanged").await;
    ends_with(None, "withheld").await;
  }

  // a session's uses are wall-clock times in the database, which paused time does not move, so
  // the session is moved back instead
  #[tokio::test(start_paused = true)]
  async fn a_session_holding_only_a_stream_goes_idle() {
    use chrono::{Duration, Utc};
    let idle = Duration::seconds(2);
    let app = app("[web.authentication]\nsession_idle = \"2s\"\n", &[]).await;
    let login = app
      .send_from(
        OUTSIDE,
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": "reseek", "password": "reseek" })),
        ),
      )
      .await;
    let cookie = crate::testing::set_cookie(&login.headers, "reseek_session").unwrap();
    let id = cookie.split_once('=').unwrap().1.split_once('.').unwrap().0;
    let last_seen = async || {
      crate::auth::sessions::find(&app.state.auth.db, id)
        .await
        .unwrap()
        .unwrap()
        .last_seen_at
    };
    // as if the session had last been used `ago`
    let used_ago = async |ago: Duration| {
      let (id, at) = (id.to_string(), crate::db::timestamp(Utc::now() - ago));
      app
        .state
        .auth
        .db
        .write(move |conn| {
          conn.execute(
            "UPDATE sessions SET last_seen_at = ?1 WHERE id = ?2",
            rusqlite::params![at, id],
          )
        })
        .await
        .unwrap();
    };
    // past the least gap between two recorded uses, a tenth of the idle limit
    used_ago(idle / 4).await;
    let before = last_seen().await;
    let mut stream = app
      .stream_from(
        OUTSIDE,
        with(request("GET", "/api/v0/live", None), "cookie", &cookie),
      )
      .await;
    assert_eq!(stream.next().await.unwrap().event, "hello");
    let peer = crate::http::Peer {
      remote: Some(OUTSIDE.parse().unwrap()),
      transport: crate::http::Transport::Plain,
    };
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    let presented = crate::auth::Presented::read(
      &peer,
      &"/api/v0/live".parse().unwrap(),
      &headers,
      &app.state.config.current().values,
    );
    let check = super::recheck(
      &app.state,
      presented,
      "local:reseek".to_string(),
      |_| Ok(()),
    );
    assert_eq!(check().await, Ok(()));
    // a use would be recorded by a task of its own. Paused time moves on only once every task
    // waits, a database call among them, so such a write is in before this ends
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
      last_seen().await,
      before,
      "neither the stream's opening nor its check is a use"
    );
    used_ago(idle + Duration::milliseconds(200)).await;
    assert_eq!(
      check().await,
      Err(api_types::live::ByeReason::Unauthorized),
      "the session went idle under the stream"
    );
  }

  // the clock's ticks run on paused time, which moves on whenever every task waits
  #[tokio::test(start_paused = true)]
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
          &seen,
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
    let started = tokio::time::Instant::now();
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
