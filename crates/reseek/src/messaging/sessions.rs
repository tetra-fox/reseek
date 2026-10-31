//! The session log: one row for each time we were logged in, so room history can say where it
//! has holes. A gap is the time between one session's end and the next one's login, or now
//! while we are not logged in. Gaps under a second are not reported.

use api_types::live::LiveFrame;
use api_types::messaging::rooms::HistoryGap;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};
use tracing::{debug, warn};

use super::Messaging;
use crate::db::{read_timestamp, timestamp};
use crate::sync::lock;

/// The shortest gap reported.
const SHORTEST: chrono::Duration = chrono::Duration::seconds(1);

/// One session as the log keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
  /// when we logged in
  pub logged_in_at: DateTime<Utc>,
  /// when it ended, `None` while it runs
  pub ended_at: Option<DateTime<Utc>>,
}

fn session(row: &Row) -> rusqlite::Result<Session> {
  let ended_at: Option<String> = row.get(1)?;
  Ok(Session {
    logged_in_at: read_timestamp(&row.get::<_, String>(0)?)?,
    ended_at: ended_at.as_deref().map(read_timestamp).transpose()?,
  })
}

/// Closes the session a run that stopped without saying so left open, at the last time it was
/// seen.
pub fn close_crashed(conn: &mut Connection) -> rusqlite::Result<()> {
  let closed = conn.execute(
    "UPDATE sessions SET ended_at = last_seen_at WHERE ended_at IS NULL",
    [],
  )?;
  if closed > 0 {
    debug!(closed, "session left open by the last run closed");
  }
  Ok(())
}

/// Every session, by when it started.
pub fn sessions(conn: &Connection) -> rusqlite::Result<Vec<Session>> {
  conn
    .prepare_cached("SELECT logged_in_at, ended_at FROM sessions ORDER BY id")?
    .query_map([], session)?
    .collect()
}

/// The gaps between `sessions`, oldest first; the last runs to `None` when it ended.
pub fn gaps(sessions: &[Session]) -> Vec<HistoryGap> {
  let mut gaps = Vec::new();
  for (index, earlier) in sessions.iter().enumerate() {
    let Some(from) = earlier.ended_at else {
      continue;
    };
    let to = sessions.get(index + 1).map(|later| later.logged_in_at);
    if to.is_some_and(|to| to - from < SHORTEST) {
      continue;
    }
    gaps.push(HistoryGap { from, to });
  }
  gaps
}

/// The gaps that lie after `oldest` and start within `from..to`, oldest first. Ranges that meet
/// at their ends list each gap once.
pub fn within(
  gaps: &[HistoryGap],
  oldest: DateTime<Utc>,
  from: DateTime<Utc>,
  to: DateTime<Utc>,
) -> Vec<HistoryGap> {
  gaps
    .iter()
    .filter(|gap| gap.from >= oldest && gap.from >= from && gap.from < to)
    .copied()
    .collect()
}

impl Messaging {
  /// A login as `username`: the name is noted for the room events that follow it, and a
  /// session row is opened.
  pub(super) async fn session_started(&self, username: String) {
    *lock(&self.session_user) = Some(username);
    let now = Utc::now();
    let opened = self
      .db
      .write(move |conn| {
        conn.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at) VALUES (?1, ?1)",
          [timestamp(now)],
        )?;
        Ok(conn.last_insert_rowid())
      })
      .await;
    match opened {
      Ok(id) => *lock(&self.session) = Some(id),
      Err(e) => warn!(error = %e, "session not logged; room history gaps may be missing"),
    }
    self.rejoin();
    self.watch_conversations();
  }

  /// The session ended, with the daemon stopping or with the connection.
  pub(super) async fn session_ended(&self) {
    let Some(id) = lock(&self.session).take() else {
      return;
    };
    let now = Utc::now();
    if let Err(e) = self
      .db
      .write(move |conn| {
        conn.execute(
          "UPDATE sessions SET ended_at = ?2, last_seen_at = ?2 WHERE id = ?1",
          params![id, timestamp(now)],
        )
      })
      .await
    {
      warn!(error = %e, "session end not logged");
    }
  }

  /// Every minute while logged in: the session was seen now, so a crash ends it here.
  pub async fn heartbeat(&self) {
    let Some(id) = *lock(&self.session) else {
      return;
    };
    let now = Utc::now();
    if let Err(e) = self
      .db
      .write(move |conn| {
        conn.execute(
          "UPDATE sessions SET last_seen_at = ?2 WHERE id = ?1",
          params![id, timestamp(now)],
        )
      })
      .await
    {
      debug!(error = %e, "session not marked seen");
    }
  }

  /// Every gap in the session log, oldest first.
  pub(super) async fn history_gaps(&self) -> Vec<HistoryGap> {
    match self.db.read(sessions).await {
      Ok(sessions) => gaps(&sessions),
      Err(e) => {
        warn!(error = %e, "session log not read; room history shows no gaps");
        Vec::new()
      }
    }
  }

  /// Pushes the gap that this session's login closed, when there was one.
  pub(super) async fn push_gap(&self) {
    let sessions = match self.db.read(sessions).await {
      Ok(sessions) => sessions,
      Err(e) => return warn!(error = %e, "session log not read; no gap pushed"),
    };
    let [.., earlier, current] = sessions.as_slice() else {
      return;
    };
    if current.ended_at.is_some() {
      return;
    }
    if let Some(gap) = gaps(&[earlier.clone(), current.clone()]).first() {
      self.live.publish(LiveFrame::RoomsGap(*gap));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{self, Database, Location, Schema};

  /// `messaging.db` as the first release of it was.
  static FIRST: Schema = db::schema!("messaging", ["0001_initial"]);

  fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
  }

  #[test]
  fn gaps_lie_between_sessions() {
    let sessions = [
      Session {
        logged_in_at: at("2026-10-26T08:00:00Z"),
        ended_at: Some(at("2026-10-26T09:00:00Z")),
      },
      Session {
        logged_in_at: at("2026-10-26T09:05:00Z"),
        ended_at: Some(at("2026-10-26T10:00:00Z")),
      },
      Session {
        logged_in_at: at("2026-10-26T10:00:00.500Z"),
        ended_at: Some(at("2026-10-26T11:00:00Z")),
      },
    ];
    let found = gaps(&sessions);
    assert_eq!(
      found,
      [
        HistoryGap {
          from: at("2026-10-26T09:00:00Z"),
          to: Some(at("2026-10-26T09:05:00Z")),
        },
        HistoryGap {
          from: at("2026-10-26T11:00:00Z"),
          to: None,
        },
      ],
      "half a second between two sessions is no gap; the last one is still open"
    );
    let running = [Session {
      ended_at: None,
      ..sessions[0].clone()
    }];
    assert!(gaps(&running).is_empty());
    assert_eq!(
      within(
        &found,
        at("2026-10-26T09:30:00Z"),
        at("2026-10-26T00:00:00Z"),
        at("2026-10-26T23:00:00Z")
      )
      .len(),
      1,
      "a gap before the room's oldest message is not the room's"
    );
  }

  #[tokio::test]
  async fn a_database_with_end_reasons_keeps_its_sessions_and_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location {
      data: dir.path().join("data"),
      backups: dir.path().join("backups"),
      volatile: false,
      log_sql: false,
    };
    std::fs::create_dir_all(&location.data).unwrap();
    std::fs::create_dir_all(&location.backups).unwrap();
    let first = Database::open(&FIRST, &location).await.unwrap();
    first
      .write(|conn| {
        conn.execute_batch(
          "INSERT INTO sessions (logged_in_at, last_seen_at, ended_at, end_reason) VALUES \
           ('2026-10-26T09:00:00.000000Z', '2026-10-26T10:00:00.000000Z', \
            '2026-10-26T10:00:00.000000Z', 'shutdown'), \
           ('2026-10-26T10:05:00.000000Z', '2026-10-26T10:05:00.000000Z', NULL, NULL)",
        )
      })
      .await
      .unwrap();
    first.close().await;
    drop(first);

    let latest = Database::open(&super::super::SCHEMA, &location)
      .await
      .unwrap();
    let found = gaps(&latest.read(sessions).await.unwrap());
    assert_eq!(
      found,
      [HistoryGap {
        from: at("2026-10-26T10:00:00Z"),
        to: Some(at("2026-10-26T10:05:00Z")),
      }]
    );
    let dropped = latest
      .read(|conn| conn.prepare("SELECT end_reason FROM sessions").map(drop))
      .await;
    assert!(dropped.is_err(), "the column is gone");
    let next = latest
      .write(|conn| {
        conn.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at) \
           VALUES ('2026-10-26T11:00:00.000000Z', '2026-10-26T11:00:00.000000Z')",
          [],
        )?;
        Ok(conn.last_insert_rowid())
      })
      .await
      .unwrap();
    assert_eq!(
      next, 3,
      "sessions are read by id, so ids go on from the old table's"
    );
  }

  #[test]
  fn ranges_that_meet_at_a_gap_list_it_once() {
    let gap = HistoryGap {
      from: at("2026-10-26T11:00:00Z"),
      to: Some(at("2026-10-26T12:00:00Z")),
    };
    let oldest = at("2026-10-26T08:00:00Z");
    let meeting = at("2026-10-26T11:00:00Z");
    let older = within(&[gap], oldest, at("2026-10-26T09:00:00Z"), meeting);
    let newer = within(&[gap], oldest, meeting, at("2026-10-26T13:00:00Z"));
    assert_eq!(
      (older.len(), newer.len()),
      (0, 1),
      "a gap starting where the ranges meet is the newer one's"
    );
  }
}
