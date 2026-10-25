//! Taking a response into a search: only while the search runs, only what its filters let
//! through, files filtered out of both lists, and the counts and the idle deadline moved on.
//! Locked files are counted apart and do not count towards the file limit, since they cannot be
//! downloaded.

use std::sync::Arc;

use soulseek_proto::peer::SearchResponse;
use tokio::time::Instant;

use super::{Running, Search, SearchOptions, SearchState};
use crate::lock;

/// A search's state and what it has taken so far
pub(super) struct Counts {
  pub(super) state: SearchState,
  pub(super) responses: u64,
  pub(super) files: u64,
  pub(super) locked_files: u64,
  /// the search times out at this instant unless a response is taken first
  pub(super) deadline: Instant,
}

impl Default for Counts {
  fn default() -> Self {
    Self {
      state: SearchState::empty(),
      responses: 0,
      files: 0,
      locked_files: 0,
      deadline: Instant::now(),
    }
  }
}

/// A response the search took, and the search right after it took it
pub(super) struct Accepted {
  pub(super) snapshot: Search,
  pub(super) response: Arc<SearchResponse>,
}

/// Takes `response` into `search` if the search runs and its filters let it through, or says
/// why not
pub(super) fn accept(search: &Running, response: SearchResponse) -> Result<Accepted, &'static str> {
  let response = filter(&search.options, response)?;
  let mut counts = lock(&search.counts);
  if counts.state != SearchState::IN_PROGRESS || search.outcome.borrow().is_some() {
    return Err("the search is not taking responses");
  }
  counts.responses += 1;
  counts.files += response.files.len() as u64;
  counts.locked_files += response.locked_files.len() as u64;
  counts.deadline = Instant::now() + search.options.search_timeout;
  Ok(Accepted {
    snapshot: search.snapshot(&counts),
    response: Arc::new(response),
  })
}

/// the response as the options let it through, or why they do not
fn filter(
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
    SearchResponse {
      username: "bob".into(),
      token: 1,
      files: files.iter().map(|name| file(name)).collect(),
      has_free_upload_slot: true,
      upload_speed: 1000,
      queue_length: 3,
      locked_files: locked.iter().map(|name| file(name)).collect(),
    }
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

  #[test]
  fn nothing_is_filtered_when_filtering_is_off() {
    let options = SearchOptions {
      filter_responses: false,
      minimum_response_file_count: 5,
      response_filter: Some(Arc::new(|_: &SearchResponse| false)),
      ..SearchOptions::default()
    };
    assert!(filter(&options, response(&[], &[])).is_ok());
  }
}
