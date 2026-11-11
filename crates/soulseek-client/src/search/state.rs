//! A search as callers see it: its state flags, the snapshot handed to callbacks and events, and
//! the options that bound and filter it.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use soulseek_proto::SearchQuery;
use soulseek_proto::peer::{File, SearchResponse};

use super::SearchScope;

/// Where a search is in its life: `Requested` when made, `Queued`, `InProgress`, then
/// `Completed` with how it ended
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchState {
  /// accepted, its token taken
  Requested,
  /// waiting for one of the searches that may run at once
  Queued,
  /// being sent to the server or sent; responses are taken
  InProgress,
  /// over
  Completed(SearchOutcome),
}

/// How a search ended
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchOutcome {
  /// the caller canceled, or the client disconnected
  Cancelled,
  /// no response came for the search timeout: the usual end of a search
  TimedOut,
  /// as many responses as the search wanted arrived
  ResponseLimitReached,
  /// as many files as the search wanted arrived
  FileLimitReached,
  /// the request could not be sent
  Errored,
}

impl SearchState {
  /// The state's name as the logs and the API write it, such as `"Completed, TimedOut"`.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Requested => "Requested",
      Self::Queued => "Queued",
      Self::InProgress => "InProgress",
      Self::Completed(SearchOutcome::Cancelled) => "Completed, Cancelled",
      Self::Completed(SearchOutcome::TimedOut) => "Completed, TimedOut",
      Self::Completed(SearchOutcome::ResponseLimitReached) => "Completed, ResponseLimitReached",
      Self::Completed(SearchOutcome::FileLimitReached) => "Completed, FileLimitReached",
      Self::Completed(SearchOutcome::Errored) => "Completed, Errored",
    }
  }

  /// the search is over
  pub fn is_complete(self) -> bool {
    matches!(self, Self::Completed(_))
  }

  /// Whether a search moves from this state to `next`: in order, ending from the queue only
  /// when canceled, and never again once completed.
  pub(crate) fn leads_to(self, next: Self) -> bool {
    matches!(
      (self, next),
      (Self::Requested, Self::Queued)
        | (Self::Queued, Self::InProgress)
        | (Self::Queued, Self::Completed(SearchOutcome::Cancelled))
        | (Self::InProgress, Self::Completed(_))
    )
  }
}

impl fmt::Display for SearchState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
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
  fn a_search_moves_only_forward_and_ends_once() {
    use SearchOutcome::*;
    use SearchState::*;
    let states = [
      Requested,
      Queued,
      InProgress,
      Completed(Cancelled),
      Completed(TimedOut),
      Completed(ResponseLimitReached),
      Completed(FileLimitReached),
      Completed(Errored),
    ];
    let allowed: Vec<(SearchState, SearchState)> = std::iter::once((Requested, Queued))
      .chain([(Queued, InProgress), (Queued, Completed(Cancelled))])
      .chain(states[3..].iter().map(|end| (InProgress, *end)))
      .collect();
    for from in states {
      for to in states {
        assert_eq!(
          from.leads_to(to),
          allowed.contains(&(from, to)),
          "{from} to {to}"
        );
      }
    }
  }
}
