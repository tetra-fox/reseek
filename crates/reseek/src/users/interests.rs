//! What we like and hate, the `soulseek.interests` options. The server keeps nothing between
//! sessions, so it is told both lists at every login, and what changed whenever they change
//! while logged in. A change made while logged out reaches the server at the next login.

use soulseek_client::Client;
use tracing::{debug, warn};

use super::Directory;
use crate::config::SoulseekInterests;

/// The option that holds what we like.
pub const LIKED: &str = "soulseek.interests.liked";
/// The option that holds what we hate.
pub const HATED: &str = "soulseek.interests.hated";

/// What the server is told, one message each.
#[derive(Debug, PartialEq, Eq)]
enum Change<'a> {
  Like(&'a str),
  Hate(&'a str),
  StopLiking(&'a str),
  StopHating(&'a str),
}

/// What the server must be told for `old` to become `new`: what left a list first, so an
/// interest moved from one list to the other ends up in the second.
fn changes<'a>(old: &'a SoulseekInterests, new: &'a SoulseekInterests) -> Vec<Change<'a>> {
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

/// Tells the server `changes`. What cannot be sent is logged once, since a lost connection
/// fails every message after it; the next login sends the lists again.
async fn send(client: &Client, changes: Vec<Change<'_>>) {
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
    warn!(error = %e, failed, "interests not sent to the server");
  }
}

impl Directory {
  /// Tells the server everything we like and hate, at login.
  pub async fn send_interests(&self, client: &Client) {
    let mut told = self.interests_told.lock().await;
    *told = SoulseekInterests::default();
    self.tell(client, &mut told).await;
  }

  /// Tells the server what changed in the lists, when logged in.
  pub(super) async fn interests_changed(&self) {
    match self.client.get() {
      Some(client) if client.state().is_logged_in() => {
        let mut told = self.interests_told.lock().await;
        self.tell(client, &mut told).await;
      }
      _ => debug!("interests reach the server at the next login"),
    }
  }

  /// Tells the server what takes it from `told` to the lists configured now, which `told`
  /// then holds.
  async fn tell(&self, client: &Client, told: &mut SoulseekInterests) {
    let wanted = self.config.current().values.soulseek.interests.clone();
    let changes = changes(told, &wanted);
    if changes.is_empty() {
      return;
    }
    debug!(
      changes = changes.len(),
      liked = wanted.liked.len(),
      hated = wanted.hated.len(),
      "telling the server our interests"
    );
    send(client, changes).await;
    *told = wanted;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lists(liked: &[&str], hated: &[&str]) -> SoulseekInterests {
    let owned = |list: &[&str]| list.iter().map(ToString::to_string).collect();
    SoulseekInterests {
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
}
