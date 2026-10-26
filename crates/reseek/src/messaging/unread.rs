//! The unread counts of every conversation and every room we are in, pushed as `chat.unread`
//! after every change, at most twice a second. A conversation counts its unacknowledged
//! messages; a room its messages after the read marker that are not ours and not hidden, and
//! those of them that mention us. The total counts the conversations and the rooms' mentions
//! only, since plain room traffic is not unread in the same sense.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_types::chat::{ChatUnread, RoomUnread};
use api_types::live::LiveFrame;
use tokio::sync::Notify;
use tracing::warn;

use super::Messaging;
use super::conversations::store;
use super::rooms::history;
use crate::db::DbError;

/// The least time between two pushes.
const PACE: Duration = Duration::from_millis(500);

/// Asks for the unread counts to be pushed.
#[derive(Default)]
pub struct Unread {
  changed: Notify,
}

impl Unread {
  /// Something unread changed; the counts are pushed soon.
  pub fn changed(&self) {
    self.changed.notify_one();
  }
}

/// The unread counts from `conversations` and `rooms`.
fn counted(
  conversations: BTreeMap<String, u32>,
  rooms: BTreeMap<String, RoomUnread>,
) -> ChatUnread {
  let mentions: u32 = rooms.values().map(|room| room.mentions).sum();
  ChatUnread {
    total: conversations.values().sum::<u32>() + mentions,
    total_mentions: mentions,
    conversations,
    rooms,
  }
}

impl Messaging {
  /// The unread counts now.
  pub async fn unread_counts(&self) -> Result<ChatUnread, DbError> {
    let rooms: Vec<String> = self
      .rooms
      .tracker()
      .rooms()
      .iter()
      .map(|room| room.name.clone())
      .collect();
    self
      .db
      .read(move |conn| {
        let conversations = store::unread(conn)?;
        let rooms = rooms
          .into_iter()
          .map(|room| Ok((room.clone(), history::unread(conn, &room)?)))
          .collect::<rusqlite::Result<_>>()?;
        Ok(counted(conversations, rooms))
      })
      .await
  }

  /// Pushes the unread counts after each change, at most twice a second, until shutdown.
  pub(super) async fn push_unread(self: Arc<Self>) {
    loop {
      tokio::select! {
        () = self.shutdown.cancelled() => return,
        () = self.unread.changed.notified() => {}
      }
      match self.unread_counts().await {
        Ok(counts) => self.live.publish(LiveFrame::ChatUnread(counts)),
        Err(e) => warn!(error = %e, "unread counts not read"),
      }
      tokio::select! {
        () = self.shutdown.cancelled() => return,
        () = tokio::time::sleep(PACE) => {}
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_total_is_the_conversations_and_the_rooms_mentions() {
    let counts = counted(
      [("alice".to_string(), 2), ("bob".to_string(), 1)].into(),
      [(
        "indie".to_string(),
        RoomUnread {
          unread: 40,
          mentions: 3,
        },
      )]
      .into(),
    );
    assert_eq!((counts.total, counts.total_mentions), (6, 3));
  }
}
