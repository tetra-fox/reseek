//! The background work every request relies on: the application state and the event stream
//! follow the configuration and the log, the log filter follows its options, and the OpenID
//! Connect providers are discovered; and the sessions that expired are ended and old ended
//! sessions and tokens deleted, which the scheduler runs.

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

pub(super) fn spawn(app: &AppState) {
  tokio::spawn(follow_configuration(app.clone()).instrument(info_span!("state")));
  tokio::spawn(forward_logs(app.clone()).instrument(info_span!("log_stream")));
  crate::auth::oidc::discover_at_start(app);
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
    app
      .live
      .publish(LiveFrame::ApplicationOptions(view::document(&loaded)));
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
        app.live.credentials_ended();
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
