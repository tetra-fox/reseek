//! What we like and hate: kept in `users.db`, told to the server at every login and when the
//! lists change. A change made while logged out reaches the server at the next login. An
//! interest is trimmed and lowercased, as other clients keep theirs, so `Jazz` and `jazz` are one.

use api_types::profile::Interests;
use rusqlite::{Connection, params};
use soulseek_client::{Client, ClientState};
use tracing::{debug, warn};

use super::Directory;
use crate::api::error::ApiError;

/// What the server is told, one message each.
#[derive(Debug, PartialEq, Eq)]
enum Change<'a> {
  Like(&'a str),
  Hate(&'a str),
  StopLiking(&'a str),
  StopHating(&'a str),
}

/// An interest as it is kept and sent.
fn normalized(entry: &str) -> String {
  entry.trim().to_lowercase()
}

/// The lists as stored, each entry normalized. Rows kept in mixed case fold into one; the first
/// stored wins, so a liked one beats a hated one.
fn stored(conn: &Connection) -> rusqlite::Result<Interests> {
  let mut interests = Interests::default();
  let mut statement =
    conn.prepare_cached("SELECT interest, liked FROM interests ORDER BY rowid")?;
  let rows = statement.query_map([], |row| {
    Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
  })?;
  for row in rows {
    let (interest, liked) = row?;
    let interest = normalized(&interest);
    if interests.liked.contains(&interest) || interests.hated.contains(&interest) {
      continue;
    }
    let list = if liked {
      &mut interests.liked
    } else {
      &mut interests.hated
    };
    list.push(interest);
  }
  Ok(interests)
}

/// Replaces the lists with `new` and returns the ones before.
fn replace(conn: &mut Connection, new: &Interests) -> rusqlite::Result<Interests> {
  let writing = conn.transaction()?;
  let old = stored(&writing)?;
  writing.execute("DELETE FROM interests", [])?;
  {
    let mut insert =
      writing.prepare_cached("INSERT INTO interests (interest, liked) VALUES (?1, ?2)")?;
    for (list, liked) in [(&new.liked, true), (&new.hated, false)] {
      for interest in list {
        insert.execute(params![interest, liked])?;
      }
    }
  }
  writing.commit()?;
  Ok(old)
}

/// `list` normalized, without repeats; a blank entry is refused.
fn cleaned(list: Vec<String>) -> Result<Vec<String>, ApiError> {
  let mut kept: Vec<String> = Vec::with_capacity(list.len());
  for entry in list {
    let entry = normalized(&entry);
    if entry.is_empty() {
      return Err(ApiError::bad_request("An interest may not be blank"));
    }
    if !kept.contains(&entry) {
      kept.push(entry);
    }
  }
  Ok(kept)
}

/// What the server must be told for `old` to become `new`: what left a list first, so an
/// interest moved from one list to the other ends up in the second.
fn changes<'a>(old: &'a Interests, new: &'a Interests) -> Vec<Change<'a>> {
  let gone = |from: &'a [String], to: &'a [String]| from.iter().filter(|i| !to.contains(i));
  let stopped_liking = gone(&old.liked, &new.liked).map(|i| Change::StopLiking(i));
  let stopped_hating = gone(&old.hated, &new.hated).map(|i| Change::StopHating(i));
  let liked = gone(&new.liked, &old.liked).map(|i| Change::Like(i));
  let hated = gone(&new.hated, &old.hated).map(|i| Change::Hate(i));
  stopped_liking
    .chain(stopped_hating)
    .chain(liked)
    .chain(hated)
    .collect()
}

/// Tells the server `changes`, the `list` named in the log. What cannot be sent is logged once
/// for the list, since a lost connection fails every message after it; the next login sends the
/// lists again.
async fn send(client: &Client, list: &str, changes: Vec<Change<'_>>) {
  let mut failed = 0;
  let mut first = None;
  for change in changes {
    let sent = match change {
      Change::Like(interest) => client.add_liked_interest(interest).await,
      Change::Hate(interest) => client.add_hated_interest(interest).await,
      Change::StopLiking(interest) => client.remove_liked_interest(interest).await,
      Change::StopHating(interest) => client.remove_hated_interest(interest).await,
    };
    if let Err(e) = sent {
      failed += 1;
      first.get_or_insert(e);
    }
  }
  if let Some(e) = first {
    warn!(error = %e, failed, list, "interests not sent to the server");
  }
}

impl Directory {
  /// What we like and hate.
  pub async fn interests(&self) -> Result<Interests, ApiError> {
    self.db.read(stored).await.map_err(ApiError::internal)
  }

  /// Makes the lists `new` and tells the server what changed when logged in. An interest is in
  /// one list at most; entries are normalized, blanks refused and repeats dropped.
  pub async fn set_interests(&self, new: Interests) -> Result<Interests, ApiError> {
    let new = Interests {
      liked: cleaned(new.liked)?,
      hated: cleaned(new.hated)?,
    };
    if let Some(both) = new.liked.iter().find(|i| new.hated.contains(i)) {
      return Err(ApiError::bad_request(format!(
        "'{both}' cannot be both liked and hated"
      )));
    }
    let _sending = self.interests_sending.lock().await;
    let kept = new.clone();
    let old = self
      .db
      .write(move |conn| replace(conn, &kept))
      .await
      .map_err(ApiError::internal)?;
    let changes = changes(&old, &new);
    debug!(changes = changes.len(), "interests changed");
    match self.client.get() {
      Some(client) if client.state() == ClientState::READY => {
        send(client, "changes", changes).await;
      }
      _ => debug!("interests reach the server at the next login"),
    }
    Ok(new)
  }

  /// Tells the server everything we like and hate, which it does not keep between sessions.
  pub async fn send_interests(&self, client: &Client) {
    let _sending = self.interests_sending.lock().await;
    let interests = match self.interests().await {
      Ok(interests) => interests,
      Err(e) => {
        warn!(error = e.message(), "interests not read for the server");
        return;
      }
    };
    debug!(
      liked = interests.liked.len(),
      hated = interests.hated.len(),
      "sending our interests to the server"
    );
    let liked = interests.liked.iter().map(|i| Change::Like(i)).collect();
    send(client, "liked", liked).await;
    let hated = interests.hated.iter().map(|i| Change::Hate(i)).collect();
    send(client, "hated", hated).await;
  }
}

#[cfg(test)]
mod tests {
  use super::super::testing::users_db;
  use super::*;

  fn lists(liked: &[&str], hated: &[&str]) -> Interests {
    let owned = |list: &[&str]| list.iter().map(ToString::to_string).collect();
    Interests {
      liked: owned(liked),
      hated: owned(hated),
    }
  }

  #[test]
  fn an_interest_moved_between_lists_leaves_the_first_before_joining_the_second() {
    let old = lists(&["jazz", "folk"], &["noise"]);
    let new = lists(&["folk", "dub"], &["noise", "jazz"]);
    assert_eq!(
      changes(&old, &new),
      [
        Change::StopLiking("jazz"),
        Change::Like("dub"),
        Change::Hate("jazz")
      ]
    );
    assert!(changes(&new, &new).is_empty());
  }

  #[test]
  fn entries_are_trimmed_lowercased_and_repeats_dropped_but_blanks_refused() {
    let list = [" Jazz ", "jazz", "DUB", "Ambient Dub", "ambient dub"];
    let list = list.iter().map(ToString::to_string).collect();
    assert_eq!(cleaned(list).unwrap(), ["jazz", "dub", "ambient dub"]);
    assert!(cleaned(vec!["  ".to_string()]).is_err());
  }

  #[tokio::test]
  async fn entries_stored_in_mixed_case_are_read_lowercased_once() {
    let db = users_db().await;
    let read = db
      .write(|conn| {
        let mut insert = conn.prepare("INSERT INTO interests (interest, liked) VALUES (?1, ?2)")?;
        for (interest, liked) in [
          ("Jazz", true),
          ("jazz", true),
          ("JAZZ", false),
          ("Noise", false),
        ] {
          insert.execute(params![interest, liked])?;
        }
        stored(conn)
      })
      .await
      .unwrap();
    assert_eq!(read, lists(&["jazz"], &["noise"]));
  }
}
