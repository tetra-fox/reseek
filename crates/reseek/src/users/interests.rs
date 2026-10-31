//! What we like and hate: kept in `users.db`, told to the server at every login and when the
//! lists change. A change made while logged out reaches the server at the next login.

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

/// The lists as stored.
fn stored(conn: &Connection) -> rusqlite::Result<Interests> {
  let mut interests = Interests::default();
  let mut statement = conn.prepare_cached("SELECT interest, liked FROM interests ORDER BY rowid")?;
  let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)))?;
  for row in rows {
    let (interest, liked) = row?;
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

/// `list` trimmed, without repeats; a blank entry is refused.
fn cleaned(list: Vec<String>) -> Result<Vec<String>, ApiError> {
  let mut kept: Vec<String> = Vec::with_capacity(list.len());
  for entry in list {
    let entry = entry.trim();
    if entry.is_empty() {
      return Err(ApiError::bad_request("An interest may not be blank"));
    }
    if !kept.iter().any(|seen| seen == entry) {
      kept.push(entry.to_string());
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

/// Tells the server one change, which is logged when it cannot; the next login sends the lists
/// again.
async fn send(client: &Client, change: Change<'_>) {
  let sent = match change {
    Change::Like(interest) => client.add_liked_interest(interest).await,
    Change::Hate(interest) => client.add_hated_interest(interest).await,
    Change::StopLiking(interest) => client.remove_liked_interest(interest).await,
    Change::StopHating(interest) => client.remove_hated_interest(interest).await,
  };
  if let Err(e) = sent {
    warn!(error = %e, ?change, "interest not sent to the server");
  }
}

impl Directory {
  /// What we like and hate.
  pub async fn interests(&self) -> Result<Interests, ApiError> {
    self
      .db
      .read(|conn| stored(conn))
      .await
      .map_err(ApiError::internal)
  }

  /// Makes the lists `new` and tells the server what changed when logged in. An interest is in
  /// one list at most; blanks are refused and repeats dropped.
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
        for change in changes {
          send(client, change).await;
        }
      }
      _ => debug!("interests reach the server at the next login"),
    }
    Ok(new)
  }

  /// Tells the server everything we like and hate, which it does not keep between sessions.
  pub async fn send_interests(&self, client: &Client) {
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
    for interest in &interests.liked {
      send(client, Change::Like(interest)).await;
    }
    for interest in &interests.hated {
      send(client, Change::Hate(interest)).await;
    }
  }
}

#[cfg(test)]
mod tests {
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
  fn entries_are_trimmed_and_repeats_dropped_but_blanks_refused() {
    let list = vec![" jazz ".to_string(), "jazz".to_string(), "dub".to_string()];
    assert_eq!(cleaned(list).unwrap(), ["jazz", "dub"]);
    assert!(cleaned(vec!["  ".to_string()]).is_err());
  }
}
