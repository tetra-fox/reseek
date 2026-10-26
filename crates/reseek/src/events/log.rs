//! The event log: every raised event is written to `events.db` by a subscriber of the bus,
//! read back for the events API, and trimmed by age and by count every five minutes.
//!
//! The writer is a bus subscriber, so a raise never waits for the disk; when the writer falls
//! behind the bus's backlog, the oldest events not yet written are the ones lost.

use std::sync::Arc;
use std::time::Duration;

use api_types::Page;
use api_types::events::{Event, EventRecord, EventType, EventsQuery};
use async_trait::async_trait;
use cascade::Watcher;
use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, params, params_from_iter};
use soulseek_client::LogLimiter;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info_span, warn};

use super::{EventBus, samples};
use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::{self, Database, DbError, Location, Schema};
use crate::services::EventService;

/// `events.db`: one table of every event raised.
pub static SCHEMA: Schema = db::schema!("events", ["0001_initial"]);

/// How often events past their age or the row cap are deleted.
const SWEEP: Duration = Duration::from_secs(300);

static NOT_WRITTEN: LogLimiter = LogLimiter::new();
static NOT_READ: LogLimiter = LogLimiter::new();

/// The event log.
pub struct EventLog {
  db: Database,
  bus: EventBus,
}

impl EventLog {
  /// Opens `events.db` in `location`, writes every event raised on `bus` from now on into it,
  /// and deletes events past `retention.events` and `retention.events_max_rows` every five
  /// minutes until `shutdown`.
  pub async fn open(
    location: &Location,
    bus: &EventBus,
    config: Watcher<Options>,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, DbError> {
    let log = Self::subscribed(location, bus).await?;
    tokio::spawn(sweep(log.clone(), config, shutdown).instrument(info_span!("event_retention")));
    Ok(log)
  }

  /// Opens `events.db` and writes every event raised on `bus` into it, without the sweep.
  async fn subscribed(location: &Location, bus: &EventBus) -> Result<Arc<Self>, DbError> {
    let db = Database::open(&SCHEMA, location).await?;
    let writer = db.clone();
    bus.subscribe("event log", None, move |record: Arc<EventRecord>| {
      let db = writer.clone();
      async move {
        if let Err(e) = write(&db, &record).await
          && let Some(suppressed) = NOT_WRITTEN.allow()
        {
          warn!(id = %record.id, error = %e, suppressed, "Failed to add event record");
        }
        Ok(())
      }
    });
    Ok(Arc::new(Self {
      db,
      bus: bus.clone(),
    }))
  }

  /// A page of the events matching `query`, with how many match its type and username
  /// filters whatever the page and the time bounds.
  async fn page(&self, query: EventsQuery) -> Result<Page<EventRecord>, DbError> {
    self.db.read(move |conn| page(conn, &query)).await
  }

  /// Deletes the events older than `max_age` before `now` (none when it is zero) and the
  /// oldest beyond `max_rows` (none when it is zero); returns how many went each way.
  async fn prune(
    &self,
    now: DateTime<Utc>,
    max_age: Duration,
    max_rows: u32,
  ) -> Result<(usize, usize), DbError> {
    let cutoff = (!max_age.is_zero()).then(|| {
      let age = chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::MAX);
      db::timestamp(db::shifted(now, -age))
    });
    self
      .db
      .write(move |conn| {
        let aged = match &cutoff {
          Some(cutoff) => conn.execute("DELETE FROM Events WHERE Timestamp < ?1", [cutoff])?,
          None => 0,
        };
        let capped = if max_rows == 0 {
          0
        } else {
          conn.execute(
            "DELETE FROM Events WHERE Id IN (SELECT Id FROM Events \
             ORDER BY Timestamp DESC, Id DESC LIMIT -1 OFFSET ?1)",
            [max_rows],
          )?
        };
        Ok((aged, capped))
      })
      .await
  }
}

#[async_trait]
impl EventService for EventLog {
  async fn list(&self, query: EventsQuery) -> Result<Page<EventRecord>, ApiError> {
    self.page(query).await.map_err(|e| {
      warn!(error = %e, "Failed to list events");
      ApiError::internal(e).logged()
    })
  }

  async fn raise_sample(
    &self,
    event_type: EventType,
    disambiguator: String,
  ) -> Result<EventRecord, ApiError> {
    let event = samples::sample(event_type, &disambiguator, Utc::now()).ok_or_else(|| {
      ApiError::bad_request(format!(
        "Event type '{}' can not be raised",
        event_type.name()
      ))
    })?;
    Ok(EventRecord::clone(&self.bus.raise(event)))
  }
}

/// Writes one event.
async fn write(db: &Database, record: &EventRecord) -> Result<(), DbError> {
  let id = record.id.to_string();
  let timestamp = db::timestamp(record.timestamp);
  let kind = record.event.event_type().name();
  let data = data_of(&record.event);
  let username = username_of(&data);
  let text = data.to_string();
  db.write(move |conn| {
    conn
      .execute(
        "INSERT INTO Events (Id, Timestamp, Type, Username, Data) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, timestamp, kind, username, text],
      )
      .map(drop)
  })
  .await
}

/// An event's data, its fields without the type it is tagged with.
fn data_of(event: &Event) -> serde_json::Value {
  match serde_json::to_value(event) {
    Ok(serde_json::Value::Object(mut tagged)) => tagged.remove("data").unwrap_or_default(),
    // an enum of plain data types is always written as its tagged object
    _ => serde_json::Value::Null,
  }
}

/// The user an event is about: its `username`, else its transfer's or its message's.
fn username_of(data: &serde_json::Value) -> Option<String> {
  [
    &data["username"],
    &data["transfer"]["username"],
    &data["message"]["username"],
  ]
  .into_iter()
  .find_map(|value| value.as_str().map(str::to_string))
}

/// The page and the count of [`EventLog::page`], read in one go.
fn page(conn: &Connection, query: &EventsQuery) -> rusqlite::Result<Page<EventRecord>> {
  let mut filters = Vec::new();
  let mut values: Vec<Value> = Vec::new();
  if !query.types.is_empty() {
    let marks = vec!["?"; query.types.len()].join(", ");
    filters.push(format!("Type IN ({marks})"));
    values.extend(
      query
        .types
        .iter()
        .map(|kind| Value::Text(kind.name().to_string())),
    );
  }
  if let Some(username) = &query.username {
    filters.push("Username = ?".to_string());
    values.push(Value::Text(username.clone()));
  }
  let total: i64 = conn.query_row(
    &format!("SELECT COUNT(*) FROM Events{}", clause(&filters)),
    params_from_iter(&values),
    |row| row.get(0),
  )?;
  if let Some(since) = query.since {
    filters.push("Timestamp > ?".to_string());
    values.push(Value::Text(db::timestamp(since)));
  }
  if let Some(before) = query.before {
    filters.push("Timestamp < ?".to_string());
    values.push(Value::Text(db::timestamp(before)));
  }
  // tailing after `since` reads forward; everything else reads back from the newest
  let order = if query.since.is_some() { "ASC" } else { "DESC" };
  values.push(Value::Integer(query.limit.into()));
  values.push(Value::Integer(query.offset.into()));
  let sql = format!(
    "SELECT Id, Timestamp, Type, Data FROM Events{} ORDER BY Timestamp {order}, Id {order} \
     LIMIT ? OFFSET ?",
    clause(&filters)
  );
  let mut statement = conn.prepare(&sql)?;
  let rows = statement.query_map(params_from_iter(&values), |row| {
    Ok((
      row.get::<_, String>(0)?,
      row.get::<_, String>(1)?,
      row.get::<_, String>(2)?,
      row.get::<_, Option<String>>(3)?,
    ))
  })?;
  let mut items = Vec::new();
  for row in rows {
    let (id, timestamp, kind, data) = row?;
    match record(&id, &timestamp, &kind, data.as_deref()) {
      Ok(record) => items.push(record),
      Err(e) => {
        if let Some(suppressed) = NOT_READ.allow() {
          warn!(
            id,
            kind,
            error = e,
            suppressed,
            "logged event not read back"
          );
        }
      }
    }
  }
  Ok(Page {
    items,
    total: total.unsigned_abs(),
  })
}

/// ` WHERE` and the filters joined by `AND`, or nothing without filters.
fn clause(filters: &[String]) -> String {
  if filters.is_empty() {
    String::new()
  } else {
    format!(" WHERE {}", filters.join(" AND "))
  }
}

/// An event as a row of the log stores it.
fn record(
  id: &str,
  timestamp: &str,
  kind: &str,
  data: Option<&str>,
) -> Result<EventRecord, String> {
  let data: serde_json::Value = match data {
    Some(text) => serde_json::from_str(text).map_err(|e| e.to_string())?,
    None => serde_json::Value::Null,
  };
  let event = serde_json::from_value(serde_json::json!({ "type": kind, "data": data }))
    .map_err(|e| e.to_string())?;
  Ok(EventRecord {
    id: id.parse().map_err(|e: uuid::Error| e.to_string())?,
    timestamp: db::read_timestamp(timestamp).map_err(|e| e.to_string())?,
    event,
  })
}

/// Every five minutes, deletes the events past the configured age and count.
async fn sweep(log: Arc<EventLog>, config: Watcher<Options>, shutdown: CancellationToken) {
  let mut every = tokio::time::interval_at(tokio::time::Instant::now() + SWEEP, SWEEP);
  loop {
    tokio::select! {
      () = shutdown.cancelled() => return,
      _ = every.tick() => {}
    }
    let retention = config.current().values.retention.clone();
    match log
      .prune(Utc::now(), retention.events, retention.events_max_rows)
      .await
    {
      Ok((0, 0)) => {}
      Ok((aged, capped)) => debug!(aged, capped, "old events deleted"),
      Err(e) => warn!(error = %e, "old events not deleted"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use api_types::blocking::BlockScope;
  use api_types::events::{NoFields, UserBlocked};

  async fn log() -> (Arc<EventLog>, EventBus) {
    let dir = tempfile::tempdir().unwrap();
    let location = Location {
      data: dir.path().to_path_buf(),
      backups: dir.path().to_path_buf(),
      volatile: true,
      log_sql: false,
    };
    let bus = EventBus::new(CancellationToken::new());
    let log = EventLog::subscribed(&location, &bus).await.unwrap();
    (log, bus)
  }

  fn query() -> EventsQuery {
    serde_json::from_str("{}").unwrap()
  }

  /// Waits until the log holds `count` events.
  async fn written(log: &EventLog, count: u64) {
    for _ in 0..200 {
      if log.page(query()).await.unwrap().total == count {
        return;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the log never held {count} events");
  }

  fn blocked(username: &str) -> Event {
    Event::UserBlocked(UserBlocked {
      version: 0,
      username: username.to_string(),
      reason: None,
      scopes: vec![BlockScope::Uploads],
      expires_at: None,
      by: "local:admin".to_string(),
    })
  }

  fn noop() -> Event {
    Event::Noop(NoFields { version: 0 })
  }

  #[tokio::test]
  async fn raised_events_are_written_with_the_id_and_time_of_their_raise() {
    let (log, bus) = log().await;
    let at = "2025-03-01T12:00:00Z".parse().unwrap();
    let raised = bus.raise_at(blocked("alice"), at);
    written(&log, 1).await;
    let page = log.page(query()).await.unwrap();
    assert_eq!(page.items, [EventRecord::clone(&raised)]);
  }

  #[tokio::test]
  async fn filters_count_without_the_cursor_and_pages_read_both_ways() {
    let (log, bus) = log().await;
    let base: DateTime<Utc> = "2025-03-01T12:00:00Z".parse().unwrap();
    let at = |minutes: i64| base + chrono::Duration::minutes(minutes);
    let ids: Vec<_> = [
      (blocked("alice"), 0),
      (noop(), 1),
      (blocked("bob"), 2),
      (blocked("alice"), 3),
    ]
    .into_iter()
    .map(|(event, minute)| bus.raise_at(event, at(minute)).id)
    .collect();
    written(&log, 4).await;

    let mut q = query();
    q.types = vec![EventType::UserBlocked];
    let page = log.page(q.clone()).await.unwrap();
    assert_eq!(page.total, 3);
    let newest_first: Vec<_> = page.items.iter().map(|r| r.id).collect();
    assert_eq!(newest_first, [ids[3], ids[2], ids[0]]);

    q.username = Some("alice".to_string());
    q.before = Some(at(3));
    let page = log.page(q.clone()).await.unwrap();
    assert_eq!(page.total, 2, "the count ignores the cursor");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, ids[0]);

    let mut tail = query();
    tail.since = Some(at(0));
    tail.limit = 2;
    let page = log.page(tail).await.unwrap();
    let oldest_first: Vec<_> = page.items.iter().map(|r| r.id).collect();
    assert_eq!(oldest_first, [ids[1], ids[2]], "after since, oldest first");
    assert_eq!(page.total, 4);
  }

  #[test]
  fn the_user_an_event_is_about_is_found_where_its_data_names_one() {
    let data = |value| username_of(&serde_json::json!(value));
    assert_eq!(
      data(serde_json::json!({ "username": "a" })),
      Some("a".into())
    );
    assert_eq!(
      data(serde_json::json!({ "transfer": { "username": "b" } })),
      Some("b".into())
    );
    assert_eq!(
      data(serde_json::json!({ "message": { "username": "c" } })),
      Some("c".into())
    );
    assert_eq!(data(serde_json::json!({ "version": 0 })), None);
  }

  #[tokio::test]
  async fn retention_deletes_by_age_then_by_count() {
    let (log, bus) = log().await;
    let now: DateTime<Utc> = "2025-03-31T00:00:00Z".parse().unwrap();
    for days in [40, 20, 2, 1, 0] {
      bus.raise_at(noop(), now - chrono::Duration::days(days));
    }
    written(&log, 5).await;
    let month = Duration::from_secs(30 * 24 * 3600);
    assert_eq!(log.prune(now, month, 0).await.unwrap(), (1, 0));
    assert_eq!(log.prune(now, Duration::ZERO, 2).await.unwrap(), (0, 2));
    let left = log.page(query()).await.unwrap();
    let ages: Vec<_> = left
      .items
      .iter()
      .map(|r| (now - r.timestamp).num_days())
      .collect();
    assert_eq!(ages, [0, 1], "the newest stay");
    assert_eq!(
      log.prune(now, Duration::ZERO, 0).await.unwrap(),
      (0, 0),
      "zero keeps everything"
    );
  }
}
