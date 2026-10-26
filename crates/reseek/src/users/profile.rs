//! A user as the API shows them: what the directory holds, their group and block, with the
//! figures from the transfer history, as a summary or a whole profile.

use api_types::blocking::Block;
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
    block,
    tags: stored.map(|s| s.tags.clone()).unwrap_or_default(),
    muted: stored.is_some_and(|s| s.muted),
    pinned: stored.is_some_and(|s| s.pinned),
    endpoint: None,
    conversation: None,
  }
}

/// The path a user's stored picture is fetched from.
pub fn picture_url(username: &str) -> String {
  let mut encoded = String::new();
  for byte in username.bytes() {
    if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'~') {
      encoded.push(byte as char);
    } else {
      encoded.push_str(&format!("%{byte:02X}"));
    }
  }
  format!("/api/v0/users/{encoded}/picture")
}

fn info_summary(username: &str, info: &Info) -> StoredInfoSummary {
  StoredInfoSummary {
    has_picture: info.has_picture,
    picture_url: info.has_picture.then(|| picture_url(username)),
    upload_slots: info.upload_slots,
    queue_length: info.queue_length,
    has_free_upload_slot: info.has_free_slot,
    fetched_at: info.at,
    stale: false,
  }
}

/// The summary of a user the directory lists.
pub fn summary(user: KnownUser, stored: Option<&Stored>, figures: &UserFigures) -> UserSummary {
  let info = stored
    .and_then(|s| s.info.as_ref())
    .map(|info| info_summary(&user.username, info));
  let active = |side: &api_types::users::TransferFigures| side.counts.active + side.counts.queued;
  UserSummary {
    info,
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

/// The whole profile of a user.
pub fn profile(user: KnownUser, stored: Option<&Stored>, figures: UserFigures) -> UserProfile {
  let info = stored.and_then(|s| s.info.as_ref()).map(|info| StoredInfo {
    description: info.description.clone(),
    info: info_summary(&user.username, info),
  });
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

  #[test]
  fn picture_urls_escape_what_a_path_segment_can_not_hold() {
    assert_eq!(picture_url("alice"), "/api/v0/users/alice/picture");
    assert_eq!(picture_url(".."), "/api/v0/users/%2E%2E/picture");
    assert_eq!(picture_url("a b/c"), "/api/v0/users/a%20b%2Fc/picture");
  }
}
