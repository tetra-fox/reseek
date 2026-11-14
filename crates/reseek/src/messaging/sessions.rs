//! The session log: one row for each time we were logged in, so room history can say where it
//! has holes. A gap is the time between one session's end and the next one's login, or now
//! while we are not logged in. Gaps under a second are not reported, except where client events
//! were lost on their way to the engine: the session is split there, and the gap stands however
//! short it is, since messages went missing in it.

use api_types::live::LiveFrame;
use api_types::messaging::rooms::{HistoryGap, RoomHistoryGap};
use chrono::{DateTime, Utc};
use reseek_shares::lock;
use rusqlite::{Connection, Row, params};
use tracing::{debug, warn};

use super::Messaging;
use super::rooms::history;
use crate::db::{read_timestamp, timestamp};

/// The shortest gap reported.
const SHORTEST: chrono::Duration = chrono::Duration::seconds(1);

/// One session as the log keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
  /// when we logged in, or took events in again after some were lost
  pub logged_in_at: DateTime<Utc>,
  /// when it ended, `None` while it runs
  pub ended_at: Option<DateTime<Utc>>,
  /// it began where client events were lost, not at a login
  pub after_loss: bool,
}

fn session(row: &Row) -> rusqlite::Result<Session> {
  let ended_at: Option<String> = row.get(1)?;
  Ok(Session {
    logged_in_at: read_timestamp(&row.get::<_, String>(0)?)?,
    ended_at: ended_at.as_deref().map(read_timestamp).transpose()?,
    after_loss: row.get(2)?,
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
    .prepare_cached("SELECT logged_in_at, ended_at, after_loss FROM sessions ORDER BY id")?
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
    let later = sessions.get(index + 1);
    let to = later.map(|later| later.logged_in_at);
    let lost = later.is_some_and(|later| later.after_loss);
    if !lost && to.is_some_and(|to| to - from < SHORTEST) {
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
    *lock(&self.session_ends) = self.shutdown.child_token();
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
    // before the rooms are joined, so the server lists us in them as we are
    self.status_at_login().await;
    self.rejoin();
    self.watch_conversations();
    let this = self.this();
    self.tasks.spawn(async move { this.feed_at_login().await });
  }

  /// The session ended at `now`, with the daemon stopping or with the connection.
  pub(super) async fn session_ended(&self, now: DateTime<Utc>) {
    let Some(id) = lock(&self.session).take() else {
      return;
    };
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

  /// Client events were lost on their way here after the one handled at `from`, so the room
  /// messages among them are in no history: the session running is ended there and a new one
  /// begins now, the gap between them is pushed to the rooms we are in, and history pages list
  /// it from now on. Nothing is marked while we are not logged in, as the next login's gap
  /// covers the loss.
  pub(super) async fn events_missed(&self, from: DateTime<Utc>) {
    let Some(id) = *lock(&self.session) else {
      return debug!("client events lost while not logged in; the next login's gap covers them");
    };
    let to = Utc::now();
    let split = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        tx.execute(
          "UPDATE sessions SET ended_at = ?2, last_seen_at = ?2 WHERE id = ?1",
          params![id, timestamp(from)],
        )?;
        tx.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at, after_loss) VALUES (?1, ?1, 1)",
          [timestamp(to)],
        )?;
        let next = tx.last_insert_rowid();
        tx.commit()?;
        Ok(next)
      })
      .await;
    match split {
      Ok(next) => {
        *lock(&self.session) = Some(next);
        debug!(%from, %to, "room history marks where client events were lost");
        self.push_gap().await;
      }
      Err(e) => warn!(error = %e, "room messages lost; the gap was not logged"),
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

  /// Pushes, for each room we are in, the gap this session's start closed, when there was one
  /// and the room has history from before it, as a page of its history would list it.
  pub(super) async fn push_gap(&self) {
    let rooms = self.rooms.tracker().joined_names();
    let closed = self
      .db
      .read(move |conn| {
        let sessions = sessions(conn)?;
        let [.., earlier, current] = sessions.as_slice() else {
          return Ok(Vec::new());
        };
        if current.ended_at.is_some() {
          return Ok(Vec::new());
        }
        let Some(gap) = gaps(&[earlier.clone(), current.clone()]).first().copied() else {
          return Ok(Vec::new());
        };
        let now = Utc::now();
        let mut closed = Vec::new();
        for room_name in rooms {
          let Some(oldest) = history::oldest(conn, &room_name)? else {
            continue;
          };
          if !within(&[gap], oldest, oldest, now).is_empty() {
            closed.push(RoomHistoryGap { room_name, gap });
          }
        }
        Ok(closed)
      })
      .await;
    match closed {
      Ok(closed) => {
        for gap in closed {
          debug!(room = gap.room_name, from = %gap.gap.from, "room history gap pushed");
          self.live.publish(LiveFrame::RoomsGap(gap));
        }
      }
      Err(e) => warn!(error = %e, "session log not read; no gap pushed"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn at(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
  }

  #[test]
  fn gaps_lie_between_sessions() {
    let sessions = [
      Session {
        logged_in_at: at("2026-10-26T08:00:00Z"),
        ended_at: Some(at("2026-10-26T09:00:00Z")),
        after_loss: false,
      },
      Session {
        logged_in_at: at("2026-10-26T09:05:00Z"),
        ended_at: Some(at("2026-10-26T10:00:00Z")),
        after_loss: false,
      },
      Session {
        logged_in_at: at("2026-10-26T10:00:00.500Z"),
        ended_at: Some(at("2026-10-26T11:00:00Z")),
        after_loss: false,
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
    let mut lost = sessions.clone();
    lost[2].after_loss = true;
    assert_eq!(
      gaps(&lost)[1],
      HistoryGap {
        from: at("2026-10-26T10:00:00Z"),
        to: Some(at("2026-10-26T10:00:00.500Z")),
      },
      "where events were lost, half a second is a gap"
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

  #[tokio::test]
  async fn events_lost_while_logged_in_leave_a_gap_however_short() {
    let app = crate::testing::app("", &[]).await;
    let messaging = &app.state.messaging;
    messaging
      .handle(soulseek_client::Event::LoggedIn {
        session: 1,
        username: "me".into(),
      })
      .await;
    let from = Utc::now();
    messaging.events_missed(from).await;
    let gaps = messaging.history_gaps().await;
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(timestamp(gaps[0].from), timestamp(from));
    assert!(gaps[0].to.is_some_and(|to| to - from < SHORTEST));
  }

  #[tokio::test]
  async fn a_login_pushes_its_gap_to_each_room_we_are_in_with_history_from_before_it() {
    let app = crate::testing::app(crate::testing::TRUSTED, &[]).await;
    let messaging = &app.state.messaging;
    let now = Utc::now();
    let ended = now - chrono::Duration::hours(1);
    let said = |room: &str, at: DateTime<Utc>| history::Arrived {
      room: room.to_string(),
      timestamp: at,
      username: "alice".to_string(),
      message: "hi".to_string(),
      is_self: false,
      is_mention: false,
      hidden: false,
    };
    // "indie" has history from before the gap, "fresh" only from this login, "left" is one we
    // are no longer in
    let stored = [
      said("indie", ended - chrono::Duration::minutes(5)),
      said("fresh", now),
      said("left", ended - chrono::Duration::minutes(5)),
    ];
    messaging
      .database()
      .write(move |conn| {
        conn.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at, ended_at) VALUES (?1, ?2, ?2)",
          params![
            timestamp(ended - chrono::Duration::hours(1)),
            timestamp(ended)
          ],
        )?;
        conn.execute(
          "INSERT INTO sessions (logged_in_at, last_seen_at) VALUES (?1, ?1)",
          [timestamp(now)],
        )?;
        for arrived in &stored {
          history::insert(conn, arrived)?;
        }
        Ok(())
      })
      .await
      .unwrap();
    for room in ["indie", "fresh"] {
      let data = soulseek_proto::server::RoomData {
        name: room.into(),
        users: Vec::new(),
        private: None,
      };
      messaging.rooms.tracker().joined(&data, "me");
    }
    let mut stream = app
      .stream_from(
        "127.0.0.1:1",
        crate::testing::request("GET", "/api/v0/live?topics=rooms", None),
      )
      .await;
    stream.until("rooms.list").await;
    messaging.push_gap().await;
    // a frame of the same topic after them, so every gap pushed is read before it
    app.state.live.publish(LiveFrame::RoomsList(Vec::new()));
    let gaps: Vec<serde_json::Value> = stream
      .until("rooms.list")
      .await
      .into_iter()
      .filter(|frame| frame.event == "rooms.gap")
      .map(|frame| frame.data)
      .collect();
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(gaps[0]["roomName"], "indie");
    assert_eq!(gaps[0]["gap"]["from"], timestamp(ended));
  }
}
