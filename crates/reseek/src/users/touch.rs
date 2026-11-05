//! Touching a user: marking that we dealt with them, with what we learned about them on the way.
//! Touches go through a bounded channel to one writer, which folds the touches of a user that
//! arrive within a second into one write.

use std::collections::BTreeMap;
use std::time::Duration;

use api_types::state::UserStatistics;
use api_types::users::UserPresence;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use soulseek_client::LogLimiter;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;

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
  /// the server reported their presence
  Status {
    /// online, away or offline
    presence: UserPresence,
    /// whether they hold privileges
    is_privileged: bool,
  },
  /// the server gave their statistics
  Statistics(UserStatistics),
}

/// One touch on its way to the writer: who, why and when.
pub type Touched = (String, Touch, DateTime<Utc>);

/// The sending half of the touch channel.
#[derive(Clone)]
pub struct Toucher {
  sender: mpsc::Sender<Touched>,
}

impl Toucher {
  /// A channel for touches, with its receiving half for the writer.
  pub fn channel() -> (Self, mpsc::Receiver<Touched>) {
    let (sender, receiver) = mpsc::channel(CAPACITY);
    (Self { sender }, receiver)
  }

  /// Sends a touch of `username`. A room message or a presence report is dropped when the
  /// channel is full, since a later one says the same; any other touch waits for room. Returns
  /// whether it was sent.
  pub async fn send(&self, username: &str, touch: Touch) -> bool {
    let touched = (username.to_string(), touch, Utc::now());
    if matches!(touched.1, Touch::RoomMessage | Touch::Status { .. }) {
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
      } => {
        self.creates = true;
        self.status = Some((presence, is_privileged, at));
      }
      Touch::Statistics(statistics) => {
        self.creates = true;
        self.statistics = Some((statistics, at));
      }
      Touch::Transfer | Touch::MessageIn | Touch::MessageOut | Touch::Browse => {
        self.creates = true;
      }
    }
  }
}

/// Waits for the next touch, then gathers every touch that arrives within [`WINDOW`] of it,
/// folded by user. `None` once the channel is closed and empty or `cancel` is canceled.
pub async fn gather(
  receiver: &mut mpsc::Receiver<Touched>,
  cancel: &CancellationToken,
) -> Option<BTreeMap<String, Pending>> {
  let first = tokio::select! {
    () = cancel.cancelled() => return None,
    first = receiver.recv() => first?,
  };
  let deadline = tokio::time::Instant::now() + WINDOW;
  let mut batch: BTreeMap<String, Pending> = BTreeMap::new();
  let (username, touch, at) = first;
  batch.entry(username).or_default().fold(touch, at);
  loop {
    tokio::select! {
      () = tokio::time::sleep_until(deadline) => break,
      next = receiver.recv() => match next {
        Some((username, touch, at)) => batch.entry(username).or_default().fold(touch, at),
        None => break,
      },
    }
  }
  Some(batch)
}

/// Writes a gathered batch in one transaction and returns the users whose row it changed.
pub fn write(
  conn: &mut Connection,
  batch: &BTreeMap<String, Pending>,
) -> rusqlite::Result<Vec<String>> {
  let tx = conn.transaction()?;
  let mut changed = Vec::new();
  for (username, pending) in batch {
    let Some(seen_at) = pending.seen_at else {
      continue;
    };
    if pending.creates {
      ensure(&tx, username, seen_at)?;
    }
    // the presence's time is when it last changed, so a report of the same presence keeps it
    if pending.room {
      let rows = tx.execute(
        "UPDATE users SET last_seen_at = max(last_seen_at, ?2), \
         presence_at = CASE WHEN last_presence = 'Online' THEN coalesce(presence_at, ?2) ELSE ?2 END, \
         last_presence = 'Online' WHERE username = ?1",
        params![username, timestamp(seen_at)],
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
    let batch = gather(&mut receiver, &cancel).await.unwrap();
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
    let later = gather(&mut receiver, &cancel).await.unwrap();
    assert_eq!(later.keys().collect::<Vec<_>>(), ["carol"], "a new window");

    let db = users_db().await;
    let written = db
      .write(move |conn| {
        let changed = write(conn, &batch)?;
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
          write(conn, &BTreeMap::from([("alice".to_string(), pending)]))?;
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
