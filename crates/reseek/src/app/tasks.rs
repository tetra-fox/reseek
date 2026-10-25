//! The background work every request relies on: the application state and the event stream
//! follow the configuration and the log, ended sessions and tokens are pruned, and the
//! OpenID Connect providers are discovered.

use std::time::Duration;

use api_types::events::{Event, SessionRevokeReason, SessionRevoked};
use api_types::live::LiveFrame;
use api_types::options::OptionsDocument;
use chrono::Utc;
use soulseek_client::LogLimiter;
use tokio::sync::broadcast::error::RecvError;
use tracing::{Instrument, debug, info, info_span, warn};

use super::AppState;
use crate::auth::{sessions, tokens};
use crate::config::redact;

/// How often ended sessions are marked and old ones pruned.
const HOUSEKEEPING: Duration = Duration::from_secs(3600);

/// How long ended sessions and tokens are kept for the audit view.
const KEPT: chrono::Duration = chrono::Duration::days(30);

static LOGS_MISSED: LogLimiter = LogLimiter::new();

pub(super) fn spawn(app: &AppState) {
  tokio::spawn(follow_configuration(app.clone()).instrument(info_span!("state")));
  tokio::spawn(forward_logs(app.clone()).instrument(info_span!("log_stream")));
  tokio::spawn(housekeeping(app.clone()).instrument(info_span!("housekeeping")));
  crate::auth::oidc::discover_at_start(app);
}

/// Keeps the state's configuration part current, failed reloads included, and pushes the
/// options after every change to them.
async fn follow_configuration(app: AppState) {
  let mut loads = app.config.subscribe();
  let mut changes = app.config.changes();
  let first = loads.borrow_and_update().clone();
  app.state.configured(&first);
  loop {
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      changed = loads.changed() => {
        if changed.is_err() {
          return;
        }
        let loaded = loads.borrow_and_update().clone();
        app.state.configured(&loaded);
      }
      change = changes.recv() => match change {
        Ok(change) => app.live.publish(LiveFrame::ApplicationOptions(OptionsDocument(
          redact::values(&change.current),
        ))),
        Err(RecvError::Lagged(missed)) => {
          debug!(missed, "options changes missed; pushing the current options");
          app.live.publish(LiveFrame::ApplicationOptions(OptionsDocument(redact::values(
            &app.config.current(),
          ))));
        }
        Err(RecvError::Closed) => return,
      },
    }
  }
}

/// Pushes every new log record to the streams following the log.
async fn forward_logs(app: AppState) {
  let mut records = app.logs.subscribe();
  loop {
    let record = tokio::select! {
      () = app.shutdown.cancelled() => return,
      record = records.recv() => record,
    };
    match record {
      Ok(record) => app
        .live
        .publish(LiveFrame::LogsRecord(api_types::logs::LogRecord::clone(
          &record,
        ))),
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

/// Every hour: marks the sessions that expired or went unused as ended, raising
/// `SessionRevoked` for each, and deletes sessions and tokens that ended a month ago.
async fn housekeeping(app: AppState) {
  let mut every = tokio::time::interval(HOUSEKEEPING);
  loop {
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      _ = every.tick() => {}
    }
    let db = &app.auth.db;
    let ended = match sessions::lapsed(db, crate::auth::idle(&app)).await {
      Ok(ids) => sessions::revoke(db, ids).await,
      Err(e) => Err(e),
    };
    match ended {
      Ok(ended) => {
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
    let before = Utc::now() - KEPT;
    match (
      sessions::prune(db, before).await,
      tokens::prune(db, before).await,
    ) {
      (Ok(sessions), Ok(tokens)) if sessions + tokens > 0 => {
        info!(sessions, tokens, "pruned ended sessions and tokens");
      }
      (Ok(_), Ok(_)) => {}
      (Err(e), _) | (_, Err(e)) => warn!(error = %e, "ended sessions and tokens not pruned"),
    }
  }
}
