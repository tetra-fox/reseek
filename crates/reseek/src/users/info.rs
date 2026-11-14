//! What a user's client says about itself, kept on their row as soon as it arrives rather than
//! with the touches gathered over a second: a refreshed profile is read back from the row, and
//! names the picture by a URL that has to serve it the moment the profile is read.

use chrono::Utc;
use rusqlite::{Connection, params};
use tracing::{debug, warn};

use super::Directory;
use super::group_cache::Reported;
use super::service::picture_tag;
use super::store::{Info, ensure};
use crate::db::timestamp;

/// What a peer said about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
  /// its free text
  pub description: String,
  /// the picture's bytes, when it sent one
  pub picture: Option<Vec<u8>>,
  /// its upload slots
  pub upload_slots: u32,
  /// how many uploads it has queued
  pub queue_length: u32,
  /// it has an upload slot free
  pub has_free_slot: bool,
}

impl Directory {
  /// Stores what `username`'s client said about itself, making their row when there is none
  /// and `creates`, and pushes their summary when it was stored. The picture is kept only while
  /// `users.cache_pictures` is on and it is no larger than `users.picture_max_bytes`, and its
  /// tag is stored with it.
  pub(super) async fn store_info(&self, username: &str, peer: PeerInfo, creates: bool) {
    let has_picture = peer.picture.is_some();
    let picture = {
      let options = self.config.current();
      let users = &options.values.users;
      peer
        .picture
        .filter(|p| users.cache_pictures && p.len() as u64 <= users.picture_max_bytes.as_u64())
    };
    let stored = Info {
      description: peer.description,
      has_picture,
      upload_slots: peer.upload_slots,
      queue_length: peer.queue_length,
      has_free_slot: peer.has_free_slot,
      at: Utc::now(),
      picture_tag: picture.as_deref().map(picture_tag),
    };
    let name = username.to_string();
    let reported = self.groups.reported();
    let written = self
      .db
      .write(move |conn| write(conn, &name, &stored, picture.as_deref(), &reported, creates))
      .await;
    match written {
      Ok(true) => self.changed(username).await,
      Ok(false) => debug!(username, "user info not stored; the user has no row"),
      Err(e) => warn!(username, error = %e, "user info not stored"),
    }
  }
}

/// Writes `info` and `picture` to `username`'s row, making the row when there is none and
/// `creates`, started with what `reported` holds of them; says whether there was a row to write.
fn write(
  conn: &mut Connection,
  username: &str,
  info: &Info,
  picture: Option<&[u8]>,
  reported: &Reported,
  creates: bool,
) -> rusqlite::Result<bool> {
  let tx = conn.transaction()?;
  if creates {
    ensure(&tx, username, info.at, reported)?;
  }
  let rows = tx.execute(
    "UPDATE users SET description = ?2, has_picture = ?3, picture = ?4, picture_tag = ?5, \
     upload_slots = ?6, queue_length = ?7, has_free_slot = ?8, info_at = ?9 WHERE username = ?1",
    params![
      username,
      info.description,
      info.has_picture,
      picture,
      info.picture_tag,
      info.upload_slots,
      info.queue_length,
      info.has_free_slot,
      timestamp(info.at)
    ],
  )?;
  tx.commit()?;
  Ok(rows > 0)
}
