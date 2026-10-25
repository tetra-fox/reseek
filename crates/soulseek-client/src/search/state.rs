//! A search as callers see it: its state flags, the snapshot handed to callbacks and events, and
//! the options that bound and filter it.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::SearchQuery;
use soulseek_proto::peer::{File, SearchResponse};

use super::SearchScope;
use crate::state::{parse_flags, write_flags};

bitflags::bitflags! {
  /// Where a search is in its life. A search goes `Requested`, `Queued`, `InProgress`, then
  /// `Completed` with exactly one reason.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
  pub struct SearchState: u32 {
    /// accepted, its token taken
    const REQUESTED = 1;
    /// sent to the server; responses are taken
    const IN_PROGRESS = 2;
    /// over, with one of the reasons below
    const COMPLETED = 4;
    /// the caller cancelled, or the client disconnected
    const CANCELLED = 8;
    /// no response came for the search timeout: the usual end of a search
    const TIMED_OUT = 16;
    /// as many responses as the search wanted arrived
    const RESPONSE_LIMIT_REACHED = 32;
    /// as many files as the search wanted arrived
    const FILE_LIMIT_REACHED = 64;
    /// the request could not be sent
    const ERRORED = 128;
    /// waiting for one of the searches that may run at once
    const QUEUED = 256;
  }
}

const NAMES: [(SearchState, &str); 9] = [
  (SearchState::REQUESTED, "Requested"),
  (SearchState::IN_PROGRESS, "InProgress"),
  (SearchState::COMPLETED, "Completed"),
  (SearchState::CANCELLED, "Cancelled"),
  (SearchState::TIMED_OUT, "TimedOut"),
  (SearchState::RESPONSE_LIMIT_REACHED, "ResponseLimitReached"),
  (SearchState::FILE_LIMIT_REACHED, "FileLimitReached"),
  (SearchState::ERRORED, "Errored"),
  (SearchState::QUEUED, "Queued"),
];

/// the flag names joined with ", ", as in "Completed, TimedOut"; "None" for no flag
impl fmt::Display for SearchState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write_flags(f, *self, &NAMES)
  }
}

/// reads the strings `Display` writes
impl FromStr for SearchState {
  type Err = String;

  fn from_str(text: &str) -> Result<Self, Self::Err> {
    parse_flags(text, &NAMES).ok_or_else(|| format!("not a search state: {text}"))
  }
}

/// A search as it was at one moment
#[derive(Clone, Debug)]
pub struct Search {
  /// the query as sent, after short terms were dropped
  pub query: SearchQuery,
  pub scope: SearchScope,
  pub token: u32,
  pub state: SearchState,
  /// responses taken
  pub response_count: u64,
  /// downloadable files in the responses taken; this is what the file limit counts
  pub file_count: u64,
  /// files in the responses taken that their sharers keep to a few users
  pub locked_file_count: u64,
}

/// `(response)` to whether to keep it
pub type ResponseFilter = Arc<dyn Fn(&SearchResponse) -> bool + Send + Sync>;

/// `(file)` to whether to keep it in its response
pub type FileFilter = Arc<dyn Fn(&File) -> bool + Send + Sync>;

/// `(previous_state, search)` on every state change, before the client raises the event
pub type SearchStateChanged = Arc<dyn Fn(SearchState, &Search) + Send + Sync>;

/// `(search, response)` for every response taken, after the search's own handler and before the
/// client raises the event
pub type SearchResponseReceived = Arc<dyn Fn(&Search, &SearchResponse) + Send + Sync>;

/// What bounds and filters one search
#[derive(Clone)]
pub struct SearchOptions {
  /// the search ends once this long passes without a response taken
  pub search_timeout: Duration,
  /// the search ends once it took this many responses
  pub response_limit: u32,
  /// the search ends once the responses taken hold this many downloadable files
  pub file_limit: u32,
  /// apply the filters below; unset takes every response
  pub filter_responses: bool,
  /// drop a response with fewer files, locked ones included
  pub minimum_response_file_count: u32,
  /// drop a response from a peer with this many uploads queued or more
  pub maximum_peer_queue_length: u32,
  /// drop a response from a peer that uploads slower than this, in bytes/second
  pub minimum_peer_upload_speed: u32,
  /// leave terms of one character out of the query
  pub remove_single_character_search_terms: bool,
  pub response_filter: Option<ResponseFilter>,
  /// applied to both file lists of a response, which is then counted again
  pub file_filter: Option<FileFilter>,
  pub state_changed: Option<SearchStateChanged>,
  pub response_received: Option<SearchResponseReceived>,
}

impl Default for SearchOptions {
  fn default() -> Self {
    Self {
      search_timeout: Duration::from_secs(15),
      response_limit: 250,
      file_limit: 25_000,
      filter_responses: true,
      minimum_response_file_count: 1,
      maximum_peer_queue_length: u32::MAX,
      minimum_peer_upload_speed: 0,
      remove_single_character_search_terms: true,
      response_filter: None,
      file_filter: None,
      state_changed: None,
      response_received: None,
    }
  }
}

/// shows which callbacks are set; the callbacks themselves have nothing to print
impl fmt::Debug for SearchOptions {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("SearchOptions")
      .field("search_timeout", &self.search_timeout)
      .field("response_limit", &self.response_limit)
      .field("file_limit", &self.file_limit)
      .field("filter_responses", &self.filter_responses)
      .field(
        "minimum_response_file_count",
        &self.minimum_response_file_count,
      )
      .field("maximum_peer_queue_length", &self.maximum_peer_queue_length)
      .field("minimum_peer_upload_speed", &self.minimum_peer_upload_speed)
      .field(
        "remove_single_character_search_terms",
        &self.remove_single_character_search_terms,
      )
      .field("response_filter", &self.response_filter.is_some())
      .field("file_filter", &self.file_filter.is_some())
      .field("state_changed", &self.state_changed.is_some())
      .field("response_received", &self.response_received.is_some())
      .finish()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn search_states_are_written_as_their_flag_names_and_read_back() {
    let states = [
      (SearchState::REQUESTED, "Requested"),
      (SearchState::QUEUED, "Queued"),
      (SearchState::IN_PROGRESS, "InProgress"),
      (
        SearchState::COMPLETED | SearchState::CANCELLED,
        "Completed, Cancelled",
      ),
      (
        SearchState::COMPLETED | SearchState::TIMED_OUT,
        "Completed, TimedOut",
      ),
      (
        SearchState::COMPLETED | SearchState::RESPONSE_LIMIT_REACHED,
        "Completed, ResponseLimitReached",
      ),
      (
        SearchState::COMPLETED | SearchState::FILE_LIMIT_REACHED,
        "Completed, FileLimitReached",
      ),
      (
        SearchState::COMPLETED | SearchState::ERRORED,
        "Completed, Errored",
      ),
    ];
    for (state, text) in states {
      assert_eq!(state.to_string(), text);
      assert_eq!(text.parse::<SearchState>(), Ok(state));
    }
  }
}
