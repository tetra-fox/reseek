//! Taking a response into a search: only while the search runs, only what its filters let
//! through, at most one per user, files filtered out of both lists, the counts and the idle
//! deadline moved on, and the search ended once it reaches a limit.
//! Locked files are counted apart and do not count toward the file limit, since they cannot be
//! downloaded.

use std::collections::HashSet;
use std::sync::Arc;

use soulseek_proto::peer::SearchResponse;
use tokio::time::Instant;

use super::{Running, Search, SearchOptions, SearchOutcome, SearchState};
use crate::lock;

/// A search's state, what it has taken so far and how it ends once that is decided
pub(super) struct Counts {
  pub(super) state: SearchState,
  /// how the search ends, decided once by what comes first: a limit, the timeout, a cancel or a
  /// failed send; responses are taken only while it is undecided
  pub(super) outcome: Option<SearchOutcome>,
  pub(super) responses: u64,
  pub(super) files: u64,
  pub(super) locked_files: u64,
  /// the users whose response was taken
  pub(super) users: HashSet<String>,
  /// the search times out at this instant unless a response is taken first
  pub(super) deadline: Instant,
}

impl Default for Counts {
  fn default() -> Self {
    Self {
      state: SearchState::Requested,
      outcome: None,
      responses: 0,
      files: 0,
      locked_files: 0,
      users: HashSet::new(),
      deadline: Instant::now(),
    }
  }
}

/// A response the search took, and the search right after it took it
pub(super) struct Accepted {
  pub(super) snapshot: Search,
  pub(super) response: Arc<SearchResponse>,
}

/// Takes `response`, which the options let through, into `search` if the search runs and took
/// none from its user yet, and ends the search when it reaches a limit with it; otherwise why
/// not. The limit is decided with the counts, so no response is taken past it.
pub(super) fn accept(search: &Running, response: SearchResponse) -> Result<Accepted, &'static str> {
  let mut counts = lock(&search.counts);
  if counts.state != SearchState::InProgress || counts.outcome.is_some() {
    return Err("the search is not taking responses");
  }
  if !counts.users.insert(response.username.clone()) {
    return Err("the search has a response from this user");
  }
  counts.responses += 1;
  counts.files += response.files.len() as u64;
  counts.locked_files += response.locked_files.len() as u64;
  counts.deadline = Instant::now() + search.options.search_timeout;
  if counts.responses >= u64::from(search.options.response_limit) {
    search.decide(&mut counts, SearchOutcome::ResponseLimitReached);
  } else if counts.files >= u64::from(search.options.file_limit) {
    search.decide(&mut counts, SearchOutcome::FileLimitReached);
  }
  Ok(Accepted {
    snapshot: search.snapshot(&counts),
    response: Arc::new(response),
  })
}

/// the response as the options let it through, or why they do not
pub(super) fn filter(
  options: &SearchOptions,
  mut response: SearchResponse,
) -> Result<SearchResponse, &'static str> {
  if !options.filter_responses {
    return Ok(response);
  }
  let enough = |response: &SearchResponse| {
    response.files.len() + response.locked_files.len()
      >= options.minimum_response_file_count as usize
  };
  if !enough(&response) {
    return Err("too few files");
  }
  if response.upload_speed < options.minimum_peer_upload_speed {
    return Err("the peer uploads too slowly");
  }
  if response.queue_length >= options.maximum_peer_queue_length {
    return Err("the peer's queue is too long");
  }
  if let Some(keep) = &options.response_filter
    && !keep(&response)
  {
    return Err("the response filter refused it");
  }
  if let Some(keep) = &options.file_filter {
    response.files.retain(|file| keep(file));
    response.locked_files.retain(|file| keep(file));
    if !enough(&response) {
      return Err("too few files after the file filter");
    }
  }
  Ok(response)
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use soulseek_proto::peer::File;

  use super::*;

  fn file(name: &str) -> File {
    File {
      code: 1,
      filename: name.into(),
      size: 1000,
      extension: String::new(),
      attributes: Vec::new(),
    }
  }

  fn response(files: &[&str], locked: &[&str]) -> SearchResponse {
    from("bob", files, locked)
  }

  fn from(username: &str, files: &[&str], locked: &[&str]) -> SearchResponse {
    SearchResponse {
      username: username.into(),
      token: 1,
      files: files.iter().map(|name| file(name)).collect(),
      has_free_upload_slot: true,
      upload_speed: 1000,
      queue_length: 3,
      locked_files: locked.iter().map(|name| file(name)).collect(),
    }
  }

  fn running(response_limit: u32) -> Running {
    let counts = Counts {
      state: SearchState::InProgress,
      ..Counts::default()
    };
    Running {
      token: 1,
      query: soulseek_proto::SearchQuery::parse("daft punk"),
      scope: super::super::SearchScope::Network,
      options: SearchOptions {
        response_limit,
        ..SearchOptions::default()
      },
      counts: std::sync::Mutex::new(counts),
      handler: std::sync::Mutex::new(Box::new(|_| {})),
      ended: tokio::sync::watch::channel(false).0,
    }
  }

  #[test]
  fn search_response_past_the_limit_is_not_taken() {
    let search = running(1);
    let first = accept(&search, response(&["a.mp3"], &[])).unwrap();
    assert_eq!(first.snapshot.response_count, 1);
    assert_eq!(
      lock(&search.counts).outcome,
      Some(SearchOutcome::ResponseLimitReached)
    );
    assert!(accept(&search, from("carol", &["b.mp3"], &[])).is_err());
    assert_eq!(lock(&search.counts).responses, 1);
  }

  #[test]
  fn a_second_response_from_one_user_is_not_taken_or_counted() {
    let search = running(2);
    accept(&search, response(&["a.mp3"], &[])).unwrap();
    assert!(accept(&search, response(&["b.mp3"], &[])).is_err());
    assert_eq!(lock(&search.counts).responses, 1);
    assert_eq!(lock(&search.counts).files, 1);
    assert!(lock(&search.counts).outcome.is_none());
    // another user still counts, and reaches the limit
    accept(&search, from("carol", &["c.mp3"], &[])).unwrap();
    assert_eq!(
      lock(&search.counts).outcome,
      Some(SearchOutcome::ResponseLimitReached)
    );
  }

  #[test]
  fn the_minimum_file_count_counts_locked_files_too() {
    let options = SearchOptions {
      minimum_response_file_count: 2,
      ..SearchOptions::default()
    };
    assert!(filter(&options, response(&["a.mp3"], &[])).is_err());
    assert!(filter(&options, response(&["a.mp3"], &["b.mp3"])).is_ok());
  }

  #[test]
  fn queue_length_and_speed_bounds() {
    let queue = SearchOptions {
      maximum_peer_queue_length: 3,
      ..SearchOptions::default()
    };
    // a queue as long as the bound is already too long
    assert!(filter(&queue, response(&["a.mp3"], &[])).is_err());
    let speed = SearchOptions {
      minimum_peer_upload_speed: 1001,
      ..SearchOptions::default()
    };
    assert!(filter(&speed, response(&["a.mp3"], &[])).is_err());
  }

  #[test]
  fn the_file_filter_rebuilds_both_lists_and_checks_the_count_again() {
    let options = SearchOptions {
      file_filter: Some(Arc::new(|file: &File| {
        file.filename.as_str().ends_with(".flac")
      })),
      ..SearchOptions::default()
    };
    let kept = filter(
      &options,
      response(&["a.mp3", "b.flac"], &["c.flac", "d.mp3"]),
    )
    .unwrap();
    assert_eq!(kept.files, [file("b.flac")]);
    assert_eq!(kept.locked_files, [file("c.flac")]);
    assert!(filter(&options, response(&["a.mp3"], &[])).is_err());
  }
}
