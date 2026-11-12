//! Touching a user: marking that we dealt with them, with what we learned about them on the way.
//! Touches go through a bounded channel to one writer, which folds the touches of a user that
//! arrive within a second into one write. Only a reason to know the user makes their row: what
//! the server reports of them, their presence and statistics, fills in a row and never makes
//! one, so nothing it says of a user forgotten by hand brings them back, whenever it lands. A
//! row made starts with what the server last said of the user. Forgetting a user goes through
//! the same channel and deletes the row in order with the touches: when there was a row, the
//! touches queued before the forget are dropped with it.

use std::collections::BTreeMap;
use std::time::Duration;

use api_types::state::UserStatistics;
use api_types::users::UserPresence;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use soulseek_client::LogLimiter;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::group_cache::Reported;
use super::store::{ensure, presence_text};
use crate::db::timestamp;

/// How many touches may wait for the writer.
pub const CAPACITY: usize = 1024;

/// How long the writer gathers touches before writing them together.
pub const WINDOW: Duration = Duration::from_secs(1);

static DROPPED: LogLimiter = LogLimiter::new();

/// Why a user was dealt with, and what came with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Touch {
  /// a transfer with them was recorded
  Transfer,
  /// a private message from them was stored
  MessageIn,
  /// a private message to them was stored
  MessageOut,
  /// they said something in a room; only a user who has a row is touched, and is then online
  RoomMessage,
  /// we started browsing them
  Browse,
  /// the server reported their presence; fills in a row, never makes one
  Status {
    /// online, away or offline
    presence: UserPresence,
    /// whether they hold privileges
    is_privileged: bool,
  },
  /// the server gave their statistics; fills in a row, never makes one
  Statistics(UserStatistics),
  /// we asked about them for ourselves: their profile, their statistics, their upload groups or
  /// a watch through the API
  Asked,
}

/// One touch on its way to the writer.
#[derive(Debug)]
pub struct Touched {
  /// who
  pub username: String,
  /// why
  pub touch: Touch,
  /// when it was made, as stored
  pub at: DateTime<Utc>,
}

/// A user being forgotten: the writer deletes their row and, when there was one, drops their
/// touches queued before; it says whether there was a row, or why it could not tell.
#[derive(Debug)]
pub struct Forget {
  /// who
  pub username: String,
  /// told whether a row was deleted
  pub done: oneshot::Sender<Result<bool, String>>,
}

/// What goes to the writer, in order.
#[derive(Debug)]
pub enum Queued {
  /// a touch
  Touched(Touched),
  /// a user forgotten
  Forget(Forget),
}

/// The sending half of the touch channel.
#[derive(Clone)]
pub struct Toucher {
  sender: mpsc::Sender<Queued>,
}

impl Toucher {
  /// A channel for touches, with its receiving half for the writer.
  pub fn channel() -> (Self, mpsc::Receiver<Queued>) {
    let (sender, receiver) = mpsc::channel(CAPACITY);
    (Self { sender }, receiver)
  }

  /// Forgets `username` through the writer: their row is deleted and, when there was one, the
  /// touches of theirs sent before are dropped, in one write. Says whether there was a row.
  pub async fn forget(&self, username: &str) -> Result<bool, String> {
    const STOPPED: &str = "the user touch writer has stopped";
    let (done, answer) = oneshot::channel();
    let forget = Forget {
      username: username.to_string(),
      done,
    };
    if self.sender.send(Queued::Forget(forget)).await.is_err() {
      return Err(STOPPED.to_string());
    }
    answer.await.map_err(|_| STOPPED.to_string())?
  }

  /// Sends a touch of `username`. A room message or a presence report is dropped when the
  /// channel is full, since a later one says the same; any other touch waits for room. Returns
  /// whether it was sent.
  pub async fn send(&self, username: &str, touch: Touch) -> bool {
    let lossy = matches!(touch, Touch::RoomMessage | Touch::Status { .. });
    let touched = Queued::Touched(Touched {
      username: username.to_string(),
      touch,
      at: Utc::now(),
    });
    if lossy {
      return match self.sender.try_send(touched) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
          if let Some(suppressed) = DROPPED.allow() {
            warn!(suppressed, "user touch dropped; the writer is behind");
          }
          false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
      };
    }
    self.sender.send(touched).await.is_ok()
  }
}

/// The touches of one user folded together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
  /// the latest touch
  pub seen_at: Option<DateTime<Utc>>,
  /// a touch that makes a row when there is none
  pub creates: bool,
  /// a room message was among them
  pub room: bool,
  /// the last presence report, with when
  pub status: Option<(UserPresence, bool, DateTime<Utc>)>,
  /// the last statistics, with when
  pub statistics: Option<(UserStatistics, DateTime<Utc>)>,
}

impl Pending {
  /// Adds `touch`, made at `at`, to the touches folded so far.
  pub fn fold(&mut self, touch: Touch, at: DateTime<Utc>) {
    self.seen_at = Some(self.seen_at.map_or(at, |seen| seen.max(at)));
    match touch {
      Touch::RoomMessage => self.room = true,
      Touch::Status {
        presence,
        is_privileged,
      } => self.status = Some((presence, is_privileged, at)),
      Touch::Statistics(statistics) => self.statistics = Some((statistics, at)),
      Touch::Transfer | Touch::MessageIn | Touch::MessageOut | Touch::Browse | Touch::Asked => {
        self.creates = true;
      }
    }
  }
}

/// The touches gathered for one write, and the forget that ended the gathering early.
#[derive(Debug, Default)]
pub struct Gathered {
  /// the touches, folded by user
  pub batch: BTreeMap<String, Pending>,
  /// a user forgotten after them
  pub forget: Option<Forget>,
}

/// Waits for the next touch, then gathers every touch that arrives within [`WINDOW`] of it,
/// folded by user, until a forget comes, which ends the gathering. `None` once the channel is
/// closed and empty or `cancel` is canceled.
pub async fn gather(
  receiver: &mut mpsc::Receiver<Queued>,
  cancel: &CancellationToken,
) -> Option<Gathered> {
  let mut next = tokio::select! {
    () = cancel.cancelled() => return None,
    first = receiver.recv() => first?,
  };
  let deadline = tokio::time::Instant::now() + WINDOW;
  let mut gathered = Gathered::default();
  loop {
    match next {
      Queued::Touched(touched) => {
        let pending = gathered.batch.entry(touched.username).or_default();
        pending.fold(touched.touch, touched.at);
      }
      Queued::Forget(forget) => {
        gathered.forget = Some(forget);
        return Some(gathered);
      }
    }
    next = tokio::select! {
      () = tokio::time::sleep_until(deadline) => return Some(gathered),
      next = receiver.recv() => match next {
        Some(next) => next,
        None => return Some(gathered),
      },
    };
  }
}

/// Writes a gathered batch in one transaction and returns the users whose row it changed. A
/// presence or statistics only fills in a row that is there; a row made here starts with what
/// the server last said of the user, as `reported` holds it.
pub fn write(
  conn: &mut Connection,
  batch: &BTreeMap<String, Pending>,
  reported: &Reported,
) -> rusqlite::Result<Vec<String>> {
  let tx = conn.transaction()?;
  let mut changed = Vec::new();
  for (username, pending) in batch {
    let Some(seen_at) = pending.seen_at else {
      continue;
    };
    if pending.creates {
      ensure(&tx, username, seen_at, reported)?;
    } else if pending.status.is_some() || pending.statistics.is_some() {
      // what the server reports fills in a row that is there, and makes none
      let rows = tx.execute(
        "UPDATE users SET last_seen_at = max(last_seen_at, ?2) WHERE username = ?1",
        params![username, timestamp(seen_at)],
      )?;
      if rows == 0 {
        continue;
      }
    }
    // the presence's time is when it last changed, so a report of the same presence keeps it
    if pending.room {
      let rows = tx.execute(
        "UPDATE users SET last_seen_at = max(last_seen_at, ?2), \
         presence_at = CASE WHEN last_presence = ?3 THEN coalesce(presence_at, ?2) ELSE ?2 END, \
         last_presence = ?3 WHERE username = ?1",
        params![
          username,
          timestamp(seen_at),
          presence_text(UserPresence::Online)
        ],
      )?;
      if rows == 0 {
        continue;
      }
    }
    if let Some((presence, privileged, at)) = pending.status {
      tx.execute(
        "UPDATE users SET \
         presence_at = CASE WHEN last_presence IS ?2 THEN coalesce(presence_at, ?3) ELSE ?3 END, \
         last_presence = ?2, is_privileged = ?4 WHERE username = ?1",
        params![username, presence_text(presence), timestamp(at), privileged],
      )?;
    }
    if let Some((statistics, at)) = &pending.statistics {
      tx.execute(
        "UPDATE users SET average_speed = ?2, upload_count = ?3, file_count = ?4, \
         directory_count = ?5, statistics_at = ?6 WHERE username = ?1",
        params![
          username,
          statistics.average_speed,
          // SQLite integers are signed; a count past them is kept at the largest
          i64::try_from(statistics.upload_count).unwrap_or(i64::MAX),
          statistics.file_count,
          statistics.directory_count,
          timestamp(*at)
        ],
      )?;
    }
    changed.push(username.clone());
  }
  tx.commit()?;
  Ok(changed)
}

#[cfg(test)]
mod tests {
  use super::super::store::get;
  use super::super::testing::users_db;
  use super::*;

  #[tokio::test(start_paused = true)]
  async fn touches_within_a_second_fold_into_one_write() {
    let (toucher, mut receiver) = Toucher::channel();
    let cancel = CancellationToken::new();
    toucher.send("alice", Touch::Transfer).await;
    toucher
      .send(
        "alice",
        Touch::Status {
          presence: UserPresence::Away,
          is_privileged: false,
        },
      )
      .await;
    toucher.send("bob", Touch::RoomMessage).await;
    toucher
      .send(
        "alice",
        Touch::Status {
          presence: UserPresence::Online,
          is_privileged: true,
        },
      )
      .await;
    let batch = gather(&mut receiver, &cancel).await.unwrap().batch;
    assert_eq!(batch.len(), 2);
    let alice = &batch["alice"];
    assert!(alice.creates);
    assert_eq!(
      alice.status.map(|(p, privileged, _)| (p, privileged)),
      Some((UserPresence::Online, true)),
      "the last report wins"
    );
    tokio::time::sleep(WINDOW * 2).await;
    toucher.send("carol", Touch::Browse).await;
    let later = gather(&mut receiver, &cancel).await.unwrap().batch;
    assert_eq!(later.keys().collect::<Vec<_>>(), ["carol"], "a new window");

    let db = users_db().await;
    let written = db
      .write(move |conn| {
        let changed = write(conn, &batch, &Reported::default())?;
        Ok((changed, get(conn, "alice")?, get(conn, "bob")?))
      })
      .await
      .unwrap();
    assert_eq!(
      written.0,
      ["alice"],
      "bob has no row for a room message to touch"
    );
    let alice = written.1.unwrap();
    assert!(alice.is_privileged);
    assert_eq!(alice.presence.map(|(p, _)| p), Some(UserPresence::Online));
    assert_eq!(written.2, None);
  }

  #[tokio::test(start_paused = true)]
  async fn a_forget_ends_the_window_its_touches_were_gathered_in() {
    let (toucher, mut receiver) = Toucher::channel();
    let cancel = CancellationToken::new();
    toucher.send("alice", Touch::MessageIn).await;
    toucher.send("bob", Touch::Transfer).await;
    let forgetting = tokio::spawn({
      let toucher = toucher.clone();
      async move { toucher.forget("alice").await }
    });
    // the forget is queued behind the touches; a touch after it is the next window's
    while receiver.len() < 3 {
      tokio::task::yield_now().await;
    }
    toucher.send("alice", Touch::MessageOut).await;
    let gathered = gather(&mut receiver, &cancel).await.unwrap();
    assert_eq!(gathered.batch.keys().collect::<Vec<_>>(), ["alice", "bob"]);
    let forget = gathered.forget.unwrap();
    assert_eq!(forget.username, "alice");
    forget.done.send(Ok(true)).unwrap();
    assert_eq!(forgetting.await.unwrap(), Ok(true));
    let next = gather(&mut receiver, &cancel).await.unwrap();
    assert_eq!(next.batch.keys().collect::<Vec<_>>(), ["alice"]);
  }

  #[tokio::test]
  async fn a_row_made_starts_with_what_the_server_last_said() {
    let db = users_db().await;
    let reported = Reported::default();
    reported.update("alice", true, |facts| {
      facts.presence = Some(UserPresence::Away);
      facts.privileged = true;
      facts.statistics = Some(api_types::users::StatisticsSnapshot {
        statistics: UserStatistics {
          average_speed: 7,
          directory_count: 1,
          file_count: 1,
          upload_count: 1,
        },
        updated_at: Utc::now(),
      });
    });
    let mut pending = Pending::default();
    pending.fold(Touch::MessageIn, Utc::now());
    let batch = BTreeMap::from([("alice".to_string(), pending)]);
    let alice = db
      .write(move |conn| {
        write(conn, &batch, &reported)?;
        get(conn, "alice")
      })
      .await
      .unwrap()
      .unwrap();
    assert_eq!(alice.presence.map(|(p, _)| p), Some(UserPresence::Away));
    assert!(alice.is_privileged);
    assert_eq!(alice.statistics.map(|(s, _)| s.average_speed), Some(7));
  }

  #[tokio::test]
  async fn a_presence_or_statistics_fills_in_a_row_and_makes_none() {
    let db = users_db().await;
    let reported = || {
      let now = Utc::now();
      let mut pending = Pending::default();
      let status = Touch::Status {
        presence: UserPresence::Away,
        is_privileged: false,
      };
      pending.fold(status, now);
      let statistics = UserStatistics {
        average_speed: 1,
        directory_count: 1,
        file_count: 1,
        upload_count: 1,
      };
      pending.fold(Touch::Statistics(statistics), now);
      BTreeMap::from([("alice".to_string(), pending)])
    };
    let (none, filled) = db
      .write(move |conn| {
        let known = Reported::default();
        let none = write(conn, &reported(), &known)?;
        ensure(conn, "alice", Utc::now(), &known)?;
        write(conn, &reported(), &known)?;
        Ok((none, get(conn, "alice")?.unwrap()))
      })
      .await
      .unwrap();
    assert!(none.is_empty(), "no row for what the server reports alone");
    assert_eq!(filled.presence.map(|(p, _)| p), Some(UserPresence::Away));
    assert!(filled.statistics.is_some());
  }

  #[tokio::test]
  async fn a_full_channel_drops_room_messages_and_presence_but_holds_the_rest() {
    let (toucher, _receiver) = Toucher::channel();
    for _ in 0..CAPACITY {
      assert!(toucher.send("alice", Touch::Transfer).await);
    }
    assert!(!toucher.send("alice", Touch::RoomMessage).await);
    let status = Touch::Status {
      presence: UserPresence::Online,
      is_privileged: false,
    };
    assert!(!toucher.send("alice", status).await);
    let waiting = tokio::time::timeout(
      Duration::from_millis(50),
      toucher.send("alice", Touch::MessageIn),
    )
    .await;
    assert!(waiting.is_err(), "a message touch waits for room");
  }

  #[tokio::test]
  async fn the_presence_keeps_the_time_it_last_changed() {
    let db = users_db().await;
    let at = |text: &str| -> DateTime<Utc> { text.parse().unwrap() };
    let status = |presence, when: &str| Pending {
      seen_at: Some(at(when)),
      creates: true,
      status: Some((presence, false, at(when))),
      ..Pending::default()
    };
    let room = |when: &str| Pending {
      seen_at: Some(at(when)),
      room: true,
      ..Pending::default()
    };
    let since = |pending: Pending| {
      let db = db.clone();
      async move {
        db.write(move |conn| {
          write(
            conn,
            &BTreeMap::from([("alice".to_string(), pending)]),
            &Reported::default(),
          )?;
          Ok(get(conn, "alice")?.unwrap().presence)
        })
        .await
        .unwrap()
      }
    };
    let online = since(status(UserPresence::Online, "2026-11-02T08:00:00Z")).await;
    assert_eq!(
      since(status(UserPresence::Online, "2026-11-04T09:00:00Z")).await,
      online,
      "the same presence again, as after a login, keeps when it began"
    );
    assert_eq!(
      since(room("2026-11-04T10:00:00Z")).await,
      online,
      "a room message from someone online changes nothing"
    );
    assert_eq!(
      since(status(UserPresence::Away, "2026-11-04T11:00:00Z")).await,
      Some((UserPresence::Away, Some(at("2026-11-04T11:00:00Z"))))
    );
    assert_eq!(
      since(room("2026-11-04T12:00:00Z")).await,
      Some((UserPresence::Online, Some(at("2026-11-04T12:00:00Z")))),
      "a room message from someone away puts them online from then"
    );
  }
}
