//! The public room feed: every message said in any public room. The server sends it only while
//! asked to, so it is asked for while an event stream follows the `public_rooms` topic, stopped
//! when the last one closes, and asked for again at each login while one does. Its messages are
//! pushed as they come and never stored.

use std::sync::Arc;

use api_types::live::LiveFrame;
use api_types::messaging::rooms::PublicRoomMessage;
use api_types::users::blocking::BlockScope;
use chrono::Utc;
use tracing::{debug, trace, warn};

use super::Messaging;
use crate::live::Topic;

/// Whether the feed is wanted and whether the server was asked for it this session.
#[derive(Default)]
pub(super) struct Feed {
  wanted: bool,
  on: bool,
}

impl Messaging {
  /// Asks the server for the feed while a stream follows it, until the daemon stops.
  pub(super) async fn follow_feed(self: Arc<Self>) {
    let mut audience = self.live.audience();
    loop {
      let wanted = audience.borrow_and_update().public_rooms > 0;
      self.feed.lock().await.wanted = wanted;
      self.ask_feed().await;
      tokio::select! {
        () = self.shutdown.cancelled() => return,
        changed = audience.changed() => if changed.is_err() { return },
      }
    }
  }

  /// Starts or stops the feed so the server sends it exactly while it is wanted, when logged in.
  async fn ask_feed(&self) {
    let mut feed = self.feed.lock().await;
    if feed.wanted == feed.on || !self.logged_in_now() {
      return;
    }
    let asked = if feed.wanted {
      self.client().start_public_chat().await
    } else {
      self.client().stop_public_chat().await
    };
    match asked {
      Ok(()) => {
        feed.on = feed.wanted;
        debug!(on = feed.on, "public room feed asked of the server");
      }
      Err(e) => warn!(error = %e, wanted = feed.wanted, "public room feed not asked of the server"),
    }
  }

  /// After a login: the new session sends no feed until asked.
  pub(super) async fn feed_at_login(&self) {
    self.feed.lock().await.on = false;
    self.ask_feed().await;
  }

  /// The connection ended, and the feed with it.
  pub(super) async fn feed_disconnected(&self) {
    self.feed.lock().await.on = false;
  }

  /// A message from the feed: pushed to the streams that follow it, unless its author is
  /// blocked for rooms.
  pub fn public_message(&self, room_name: String, username: String, message: String) {
    if !self.live.followed(Topic::PublicRooms) {
      return;
    }
    if self.users.is_blocked(&username, None, BlockScope::Rooms) {
      return trace!(
        room = room_name,
        username, "public room message from a blocked user"
      );
    }
    self
      .live
      .publish(LiveFrame::RoomsPublicMessage(PublicRoomMessage {
        room_name,
        username,
        message,
        received_at: Utc::now(),
      }));
  }
}
