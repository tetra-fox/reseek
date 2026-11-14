//! The background work every request relies on. The application state and the event stream
//! follow the configuration and the log, the log filter follows its options, the OpenID Connect
//! providers are discovered, and the sessions of users a configuration that stands gives no role
//! are ended. The scheduler runs the rest: ending the sessions that expired and deleting old
//! ended sessions and tokens.

use std::sync::Arc;

use api_types::events::{Event, SessionRevokeReason, SessionRevoked};
use api_types::live::LiveFrame;
use chrono::Utc;
use soulseek_client::LogLimiter;
use tokio::sync::broadcast::error::RecvError;
use tracing::{Instrument, debug, info, info_span, warn};

use cascade::Watcher;
use tokio_util::sync::CancellationToken;

use super::AppState;
use crate::auth::{sessions, tokens};
use crate::config::{Options, view};
use crate::live::Topic;
use crate::logging::Logging;

/// How long ended sessions and tokens are kept for the audit view.
const KEPT: chrono::Duration = chrono::Duration::days(30);

static LOGS_MISSED: LogLimiter = LogLimiter::new();

/// How long a configuration stands unchanged before the sessions of users it gives no role are
/// ended, so a file read half written does not end them for good.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) fn spawn(app: &AppState) {
  tokio::spawn(follow_configuration(app.clone()).instrument(info_span!("state")));
  tokio::spawn(end_roleless_sessions(app.clone()).instrument(info_span!("sessions")));
  tokio::spawn(forward_logs(app.clone()).instrument(info_span!("log_stream")));
  tokio::spawn(crate::auth::oidc::discover_at_start(app.clone()));
}

/// Keeps the state's configuration part current and pushes the options whenever the
/// configuration changes: its values, a failed reload, the files read or what waits for a
/// reconnect.
async fn follow_configuration(app: AppState) {
  let mut loads = app.config.subscribe();
  let first = loads.borrow_and_update().clone();
  app.state.configured(&first);
  loop {
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      changed = loads.changed() => if changed.is_err() { return },
    }
    let loaded = loads.borrow_and_update().clone();
    app.state.configured(&loaded);
    // a principal's role is read from the configuration, so every stream checks its own again
    app.live.credentials_changed();
    app
      .live
      .publish(LiveFrame::ApplicationOptions(view::document(&loaded)));
  }
}

/// Ends the sessions of users the configuration gives no role, each time a configuration has
/// stood unchanged for [`SETTLE`], the one the daemon starts with included.
async fn end_roleless_sessions(app: AppState) {
  let mut loads = app.config.subscribe();
  loads.borrow_and_update();
  loop {
    // a change starts the wait over, also one that lands as the wait ends
    let settled = tokio::select! {
      biased;
      changed = loads.changed() => match changed {
        Ok(()) => false,
        Err(_) => return,
      },
      () = app.shutdown.cancelled() => return,
      () = tokio::time::sleep(SETTLE) => true,
    };
    if !settled {
      continue;
    }
    if let Err(e) = crate::auth::end_sessions_without_role(&app).await {
      warn!(error = ?e, "the sessions of users with no role now were not ended");
    }
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      changed = loads.changed() => if changed.is_err() { return },
    }
  }
}

/// Applies the live parts of each configuration change that are not read per request.
pub(super) async fn follow_changes(
  watcher: Watcher<Options>,
  logging: Arc<Logging>,
  cancel: CancellationToken,
) {
  let mut changes = watcher.changes();
  loop {
    let change = tokio::select! {
      () = cancel.cancelled() => return,
      change = changes.recv() => change,
    };
    match change {
      Ok(change) => {
        if change
          .changed
          .iter()
          .any(|k| k.starts_with("logging.") || k == "soulseek.distributed_network.logging")
        {
          logging.apply(&change.current.values);
        }
        if !change.by_apply.restart.is_empty() {
          info!(
            keys = change.by_apply.restart.join(", "),
            "One or more updated options requires an application restart to take effect."
          );
        }
      }
      Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
        warn!(
          missed,
          "configuration changes missed; applying the current values"
        );
        logging.apply(&watcher.current().values);
      }
      Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
    }
  }
}

/// Pushes every new log record to the streams following the log, while one does; without one
/// a record would only take a place in the resume window from the other topics.
async fn forward_logs(app: AppState) {
  let mut records = app.logs.subscribe();
  loop {
    let record = tokio::select! {
      () = app.shutdown.cancelled() => return,
      record = records.recv() => record,
    };
    match record {
      Ok(record) => {
        if app.live.followed(Topic::Logs) {
          app
            .live
            .publish(LiveFrame::LogsRecord(api_types::logs::LogRecord::clone(
              &record,
            )));
        }
      }
      Err(RecvError::Lagged(missed)) => {
        if let Some(suppressed) = LOGS_MISSED.allow() {
          warn!(
            missed,
            suppressed, "log records not pushed to the event stream"
          );
        }
      }
      Err(RecvError::Closed) => return,
    }
  }
}

/// Marks the sessions that expired or went unused as ended, raising `SessionRevoked` for each
/// and closing the event streams they opened.
pub(crate) async fn end_lapsed_sessions(app: &AppState) {
  let db = &app.auth.db;
  let ended = match sessions::lapsed(db, crate::auth::idle(app)).await {
    Ok(ids) => sessions::revoke(db, ids).await,
    Err(e) => Err(e),
  };
  match ended {
    Ok(ended) => {
      if !ended.is_empty() {
        app.live.credentials_changed();
      }
      for session in ended {
        debug!(subject = session.subject, "session ended by time");
        app.events.raise(Event::SessionRevoked(SessionRevoked {
          version: 0,
          session_id: session.id,
          subject: session.subject,
          by: None,
          reason: SessionRevokeReason::Expired,
        }));
      }
    }
    Err(e) => warn!(error = %e, "ended sessions not marked"),
  }
}

/// Deletes the sessions and tokens that ended a month ago; returns how many.
pub(crate) async fn prune_sessions(app: &AppState) -> Result<usize, crate::db::DbError> {
  let db = &app.auth.db;
  let before = Utc::now() - KEPT;
  let sessions = sessions::prune(db, before).await?;
  let tokens = tokens::prune(db, before).await?;
  Ok(sessions + tokens)
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use serde_json::json;

  use super::SETTLE;
  use crate::auth::sessions;
  use crate::testing::{Answer, OUTSIDE, TestApp, app, request, set_cookie, with};

  const VIEWER: &str = "[web.authentication.local.users.viewer]\npassword = \"look\"\n";

  async fn signed_in(app: &TestApp) -> String {
    let login = json!({ "username": "viewer", "password": "look" });
    let answer = app
      .send_from(OUTSIDE, request("POST", "/api/v0/session", Some(login)))
      .await;
    set_cookie(&answer.headers, "reseek_session").unwrap()
  }

  /// Loads `text` as the configuration and lets the scan see the change before the clock moves.
  async fn reload(app: &TestApp, text: &str) {
    std::fs::write(app.dir.path().join("config.toml"), text).unwrap();
    app.state.config.reload().await.unwrap();
    tokio::task::yield_now().await;
  }

  async fn me(app: &TestApp, cookie: &str) -> Answer {
    let me = with(request("GET", "/api/v0/session", None), "cookie", cookie);
    app.send_from(OUTSIDE, me).await
  }

  async fn status(app: &TestApp, cookie: &str) -> u16 {
    me(app, cookie).await.status.as_u16()
  }

  async fn ended(app: &TestApp, cookie: &str) -> bool {
    let id = cookie.split_once('=').unwrap().1.split_once('.').unwrap().0;
    let session = sessions::find(&app.state.auth.db, id).await.unwrap();
    session.unwrap().revoked_at.is_some()
  }

  /// Waits up to another [`SETTLE`] until the session of `cookie` ended, which the scan does on
  /// tasks of its own.
  async fn until_ended(app: &TestApp, cookie: &str) {
    let step = Duration::from_millis(100);
    for _ in 0..SETTLE.as_millis() / step.as_millis() {
      if ended(app, cookie).await {
        return;
      }
      tokio::time::sleep(step).await;
    }
    panic!("the session was not ended");
  }

  /// Lets the clock pass `wait` and whatever that wakes run to its end: a paused clock moves on
  /// by itself only once every task waits on a timer.
  async fn pass(wait: Duration) {
    tokio::time::advance(wait).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
  }

  #[tokio::test(start_paused = true)]
  async fn a_removal_that_does_not_stand_ends_no_session() {
    let app = app(VIEWER, &[]).await;
    let cookie = signed_in(&app).await;
    // a file read half written, then whole a moment later
    reload(&app, "").await;
    let withheld = me(&app, &cookie).await;
    assert_eq!(withheld.status, 403, "refused at once");
    assert_eq!(withheld.json()["error"], "access_withheld");
    assert!(
      set_cookie(&withheld.headers, "reseek_session").is_none(),
      "the browser keeps its cookie"
    );
    pass(Duration::from_secs(1)).await;
    reload(&app, VIEWER).await;
    pass(SETTLE).await;
    assert!(!ended(&app, &cookie).await);
    assert_eq!(status(&app, &cookie).await, 200);
  }

  #[tokio::test(start_paused = true)]
  async fn a_change_late_in_a_wait_starts_it_over() {
    let app = app(VIEWER, &[]).await;
    let cookie = signed_in(&app).await;
    let other = format!("{VIEWER}[web]\nhostnames = [\"reseek.lan\"]\n");
    reload(&app, &other).await;
    pass(SETTLE - Duration::from_secs(3)).await;
    // read half written near the end of that wait, then whole after it would have ended
    reload(&app, "").await;
    pass(Duration::from_secs(3)).await;
    reload(&app, VIEWER).await;
    pass(SETTLE).await;
    assert!(!ended(&app, &cookie).await);
  }

  #[tokio::test(start_paused = true)]
  async fn a_removal_that_stands_ends_the_session() {
    let app = app(VIEWER, &[]).await;
    let cookie = signed_in(&app).await;
    reload(&app, "").await;
    pass(SETTLE - Duration::from_secs(2)).await;
    assert!(
      !ended(&app, &cookie).await,
      "the removal has not stood long enough"
    );
    pass(Duration::from_secs(2)).await;
    until_ended(&app, &cookie).await;
    reload(&app, VIEWER).await;
    assert_eq!(status(&app, &cookie).await, 401, "it takes a new sign-in");
  }

  #[tokio::test(start_paused = true)]
  async fn a_removal_made_while_stopped_ends_the_session_after_the_start() {
    let app = app(VIEWER, &[]).await;
    let cookie = signed_in(&app).await;
    std::fs::write(app.dir.path().join("config.toml"), "").unwrap();
    let app = app.restarted(&[]).await;
    pass(SETTLE).await;
    until_ended(&app, &cookie).await;
  }
}
