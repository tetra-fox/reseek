//! A user as the API shows them: what the directory holds, their group and block, with the
//! figures from the transfer history, as a summary or a whole profile.

use api_types::users::blocking::Block;
use api_types::users::{
  KnownUser, StoredInfo, StoredInfoSummary, StoredStatistics, SummaryFigures, UserFigures,
  UserProfile, UserSummary,
};

use super::groups;
use super::store::{Info, Stored};
use crate::config::Options;

/// What the directory holds about `username`, from their row when there is one, with the
/// block that applies to them.
pub fn known(
  options: &Options,
  username: &str,
  stored: Option<&Stored>,
  block: Option<Block>,
) -> KnownUser {
  KnownUser {
    username: username.to_string(),
    known: stored.is_some(),
    first_seen_at: stored.map(|s| s.first_seen_at),
    last_seen_at: stored.map(|s| s.last_seen_at),
    presence: stored.and_then(|s| s.presence.map(|(p, _)| p)),
    presence_at: stored.and_then(|s| s.presence.and_then(|(_, at)| at)),
    is_privileged: stored.is_some_and(|s| s.is_privileged),
    statistics: stored
      .and_then(|s| s.statistics.clone())
      .map(|(statistics, fetched_at)| StoredStatistics {
        statistics,
        fetched_at,
        stale: false,
      }),
    country_code: stored.and_then(|s| s.country_code.clone()),
    group: groups::effective(options, username, stored),
    group_override: stored.and_then(|s| s.group_override.clone()),
    block,
    tags: stored.map(|s| s.tags.clone()).unwrap_or_default(),
    muted: stored.is_some_and(|s| s.muted),
    pinned: stored.is_some_and(|s| s.pinned),
    endpoint: None,
    conversation: None,
  }
}

/// The path `username`'s stored picture with `tag` is fetched from. The tag changes with the
/// picture, so a copy cached under the path is never shown for another picture.
pub fn picture_url(username: &str, tag: &str) -> String {
  let mut encoded = String::new();
  for byte in username.bytes() {
    if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'~') {
      encoded.push(byte as char);
    } else {
      encoded.push_str(&format!("%{byte:02X}"));
    }
  }
  format!("/api/v0/users/{encoded}/picture?v={tag}")
}

fn info_summary(username: &str, info: &Info) -> StoredInfoSummary {
  StoredInfoSummary {
    has_picture: info.has_picture,
    picture_url: info
      .picture_tag
      .as_deref()
      .map(|tag| picture_url(username, tag)),
    upload_slots: info.upload_slots,
    queue_length: info.queue_length,
    has_free_upload_slot: info.has_free_slot,
    fetched_at: info.at,
    stale: false,
  }
}

/// The first `count` lines of `text`.
fn first_lines(text: &str, count: usize) -> String {
  text.lines().take(count).collect::<Vec<_>>().join("\n")
}

/// The summary of a user the directory lists.
pub fn summary(user: KnownUser, stored: Option<&Stored>, figures: &UserFigures) -> UserSummary {
  let stored_info = stored.and_then(|s| s.info.as_ref());
  let info = stored_info.map(|info| info_summary(&user.username, info));
  let active = |side: &api_types::users::TransferFigures| side.counts.active + side.counts.queued;
  UserSummary {
    info,
    description: stored_info.map(|info| first_lines(&info.description, 3)),
    notes: stored
      .filter(|s| !s.notes.is_empty())
      .map(|s| first_lines(&s.notes, 2)),
    figures: SummaryFigures {
      downloads: figures.downloads.counts,
      uploads: figures.uploads.counts,
      last_transfer_at: figures.last_transfer_at,
      messages: figures.messages,
      last_message_at: figures.last_message_at,
    },
    unread_messages: figures.messages.unread,
    active_transfers: active(&figures.downloads) + active(&figures.uploads),
    user,
  }
}

/// `username`'s info as a profile shows it.
pub fn stored_info(username: &str, info: &Info) -> StoredInfo {
  StoredInfo {
    description: info.description.clone(),
    info: info_summary(username, info),
  }
}

/// The whole profile of a user.
pub fn profile(user: KnownUser, stored: Option<&Stored>, figures: UserFigures) -> UserProfile {
  let info = stored
    .and_then(|s| s.info.as_ref())
    .map(|info| stored_info(&user.username, info));
  UserProfile {
    info,
    notes: stored.map(|s| s.notes.clone()).unwrap_or_default(),
    figures,
    user,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn figures() -> UserFigures {
    UserFigures {
      downloads: Default::default(),
      uploads: Default::default(),
      last_transfer_at: None,
      messages: Default::default(),
      last_message_at: None,
    }
  }

  #[test]
  fn the_summary_has_the_first_lines_of_the_description_and_the_notes() {
    assert_eq!(first_lines("one\r\ntwo\nthree\nfour", 3), "one\ntwo\nthree");
    assert_eq!(first_lines("one", 2), "one");
    assert_eq!(first_lines("", 3), "");
    assert_eq!(first_lines("\n\nthird", 2), "\n");

    let at = "2026-10-26T12:00:00Z".parse().unwrap();
    let stored = Stored {
      username: "alice".to_string(),
      first_seen_at: at,
      last_seen_at: at,
      presence: None,
      is_privileged: false,
      statistics: None,
      country_code: None,
      info: Some(Info {
        description: "Jazz.\nVinyl rips.\nAsk first.\nNo bots.".to_string(),
        has_picture: false,
        upload_slots: 2,
        queue_length: 0,
        has_free_slot: true,
        at,
        picture_tag: None,
      }),
      notes: "met in #indie\nfast\nslow at night".to_string(),
      tags: Vec::new(),
      group_override: None,
      muted: false,
      pinned: false,
    };
    let options = Options::default();
    let user = known(&options, "alice", Some(&stored), None);
    let alice = summary(user, Some(&stored), &figures());
    assert_eq!(
      alice.description.as_deref(),
      Some("Jazz.\nVinyl rips.\nAsk first.")
    );
    assert_eq!(alice.notes.as_deref(), Some("met in #indie\nfast"));
    let empty = summary(known(&options, "bob", None, None), None, &figures());
    assert_eq!((empty.description, empty.notes), (None, None));
  }

  #[test]
  fn picture_urls_escape_what_a_path_segment_can_not_hold() {
    assert_eq!(
      picture_url("alice", "ab"),
      "/api/v0/users/alice/picture?v=ab"
    );
    assert_eq!(picture_url("..", "ab"), "/api/v0/users/%2E%2E/picture?v=ab");
    assert_eq!(
      picture_url("a b/c", "ab"),
      "/api/v0/users/a%20b%2Fc/picture?v=ab"
    );
  }
}
