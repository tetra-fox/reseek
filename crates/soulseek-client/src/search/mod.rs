//! Our searches. A search goes to the server in the form its scope needs; peers that have matches
//! answer over a message connection, naming the search by its token. A search takes the responses
//! its options let through until none came for a while or it has enough, waiting first for one of
//! the searches that may run at once. Disconnecting from the server cancels every search.

mod responses;
mod state;

pub use state::{
  FileFilter, ResponseFilter, Search, SearchOptions, SearchResponseReceived, SearchState,
  SearchStateChanged,
};

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use soulseek_proto::peer::SearchResponse;
use soulseek_proto::server::{FileSearchRequest, RoomSearch, UserSearch, WishlistSearch};
use soulseek_proto::{SearchQuery, ServerRequest};
use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::client::{Client, failed, non_blank};
use crate::error::ClientError;
use crate::events::Event;
use crate::limit::Limiter;
use crate::lock;
use crate::token::TokenGenerator;
use responses::{Accepted, Counts};

/// Who a search goes to
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchScope {
  /// everyone, through the distributed tree
  Network,
  /// everyone, from the wishlist, which the server allows only at its wishlist interval
  Wishlist,
  /// the members of these rooms, one request per room
  Rooms(Vec<String>),
  /// these users
  User(Vec<String>),
}

impl SearchScope {
  fn validate(&self) -> Result<(), ClientError> {
    match self {
      Self::Network | Self::Wishlist => Ok(()),
      Self::Rooms(rooms) if rooms.is_empty() => Err(ClientError::InvalidArgument(
        "A room search needs at least one room".into(),
      )),
      Self::Rooms(rooms) => rooms
        .iter()
        .try_for_each(|room| non_blank("room name", room)),
      Self::User(usernames) if usernames.is_empty() => Err(ClientError::InvalidArgument(
        "A user search needs at least one username".into(),
      )),
      Self::User(usernames) => usernames
        .iter()
        .try_for_each(|username| non_blank("username", username)),
    }
  }

  /// the requests that send `text` with `token` to this scope
  fn requests(&self, token: u32, text: &str) -> Vec<ServerRequest> {
    let query = text.to_owned();
    match self {
      Self::Network => vec![ServerRequest::FileSearch(FileSearchRequest {
        token,
        query,
      })],
      Self::Wishlist => vec![ServerRequest::WishlistSearch(WishlistSearch {
        token,
        query,
      })],
      Self::Rooms(rooms) => rooms
        .iter()
        .map(|room| {
          ServerRequest::RoomSearch(RoomSearch {
            room: room.clone(),
            token,
            query: query.clone(),
          })
        })
        .collect(),
      Self::User(usernames) => usernames
        .iter()
        .map(|username| {
          ServerRequest::UserSearch(UserSearch {
            username: username.clone(),
            token,
            query: query.clone(),
          })
        })
        .collect(),
    }
  }
}

/// The handler a search hands its responses to
type Handler = Box<dyn FnMut(SearchResponse) + Send>;

/// One search while it runs
struct Running {
  token: u32,
  query: SearchQuery,
  scope: SearchScope,
  options: SearchOptions,
  counts: Mutex<Counts>,
  handler: Mutex<Handler>,
  /// set once, with the reason the search completed
  outcome: watch::Sender<Option<SearchState>>,
}

impl Running {
  fn snapshot(&self, counts: &Counts) -> Search {
    Search {
      query: self.query.clone(),
      scope: self.scope.clone(),
      token: self.token,
      state: counts.state,
      response_count: counts.responses,
      file_count: counts.files,
      locked_file_count: counts.locked_files,
    }
  }

  /// Ends the search with `reason` unless it ended already, and returns the reason it ended
  /// with.
  fn complete(&self, reason: SearchState) -> SearchState {
    let mut ended = reason;
    self.outcome.send_if_modified(|outcome| match outcome {
      Some(already) => {
        ended = *already;
        false
      }
      None => {
        *outcome = Some(reason);
        true
      }
    });
    ended
  }
}

/// What the client keeps for its searches
pub(crate) struct Searches {
  running: Mutex<HashMap<u32, Arc<Running>>>,
  slots: Limiter,
}

/// removes a search from the table when its operation ends
struct Registration<'a> {
  searches: &'a Searches,
  token: u32,
}

impl Drop for Registration<'_> {
  fn drop(&mut self) {
    self.searches.running().remove(&self.token);
  }
}

impl Searches {
  pub(crate) fn new(max_concurrent: u32) -> Self {
    Self {
      running: Mutex::new(HashMap::new()),
      slots: Limiter::new(max_concurrent),
    }
  }

  fn running(&self) -> MutexGuard<'_, HashMap<u32, Arc<Running>>> {
    lock(&self.running)
  }

  fn get(&self, token: u32) -> Option<Arc<Running>> {
    self.running().get(&token).cloned()
  }

  /// whether a search of ours with `token` is running
  pub(crate) fn is_running(&self, token: u32) -> bool {
    self.running().contains_key(&token)
  }

  /// Takes `token`, or a fresh one, for `search`, in one step so two callers cannot both take
  /// one token
  fn register(
    &self,
    tokens: &TokenGenerator,
    token: Option<u32>,
    search: impl FnOnce(u32) -> Running,
  ) -> Result<(Registration<'_>, Arc<Running>), ClientError> {
    let mut running = self.running();
    let token = match token {
      Some(token) if running.contains_key(&token) => {
        return Err(ClientError::DuplicateToken(token));
      }
      Some(token) => token,
      None => tokens.next_free(|token| running.contains_key(&token)),
    };
    let search = Arc::new(search(token));
    running.insert(token, search.clone());
    Ok((
      Registration {
        searches: self,
        token,
      },
      search,
    ))
  }

  /// whether `token` is a search of ours that asked `own` directly, the one way our own search
  /// may reach us and be answered
  pub(crate) fn is_self_search(&self, token: u32, own: &str) -> bool {
    self.get(token).is_some_and(|search| {
      matches!(&search.scope, SearchScope::User(usernames) if usernames.iter().any(|username| username == own))
    })
  }

  /// cancels every search, for when the server session ends
  pub(crate) fn cancel_all(&self) {
    let running: Vec<_> = self.running().values().cloned().collect();
    if !running.is_empty() {
      tracing::debug!(count = running.len(), "searches canceled");
    }
    for search in running {
      search.complete(SearchState::CANCELLED);
    }
  }

  /// follows a change of the bound on searches at once
  pub(crate) fn resize(&self, max_concurrent: u32) {
    self.slots.resize(max_concurrent);
  }
}

impl Client {
  /// Searches for `text` in `scope` and returns the search as it ended with every response it
  /// took, in the order they arrived. The search's own end, a timeout or a limit, is a success;
  /// a search canceled by the caller or by a disconnect fails with `Canceled`.
  pub async fn search(
    &self,
    text: &str,
    scope: SearchScope,
    token: Option<u32>,
    options: SearchOptions,
    cancel: &CancellationToken,
  ) -> Result<(Search, Vec<SearchResponse>), ClientError> {
    let responses = Arc::new(Mutex::new(Vec::new()));
    let collect = {
      let responses = responses.clone();
      move |response| lock(&responses).push(response)
    };
    let search = self
      .search_with(text, scope, token, options, collect, cancel)
      .await?;
    let responses = std::mem::take(&mut *lock(&responses));
    Ok((search, responses))
  }

  /// [`Client::search`] that hands each response to `handler` as it is taken instead of
  /// collecting them
  pub async fn search_with(
    &self,
    text: &str,
    scope: SearchScope,
    token: Option<u32>,
    options: SearchOptions,
    handler: impl FnMut(SearchResponse) + Send + 'static,
    cancel: &CancellationToken,
  ) -> Result<Search, ClientError> {
    let query = search_query(text, &options)?;
    scope.validate()?;
    if options.search_timeout.is_zero() {
      return Err(ClientError::InvalidArgument(
        "The search timeout must be greater than zero".into(),
      ));
    }
    let search_text = query.search_text();
    self.require_logged_in(&format!("search for {search_text}"))?;
    let searches = self.searches();
    let (_registration, search) = searches.register(self.tokens(), token, |token| Running {
      token,
      query,
      scope,
      options,
      counts: Mutex::new(Counts::default()),
      handler: Mutex::new(Box::new(handler)),
      outcome: watch::channel(None).0,
    })?;
    let span = tracing::info_span!("search", token = search.token);
    self
      .run_search(&search, &search_text, cancel)
      .instrument(span)
      .await
  }

  async fn run_search(
    &self,
    search: &Arc<Running>,
    text: &str,
    cancel: &CancellationToken,
  ) -> Result<Search, ClientError> {
    self.set_search_state(search, SearchState::REQUESTED);
    self.set_search_state(search, SearchState::QUEUED);
    // another task can end the search while it waits for its turn or is being sent: a disconnect,
    // or, once it takes responses, a limit a response reached
    let mut outcome = search.outcome.subscribe();
    let limit = self.options().maximum_concurrent_searches;
    let slot = tokio::select! {
      biased;
      () = ended(&mut outcome) => {
        return Err(self.end_search(search, SearchState::CANCELLED, ClientError::Canceled));
      }
      slot = self.searches().slots.acquire(limit, cancel) => match slot {
        Ok(slot) => slot,
        Err(error) => return Err(self.end_search(search, SearchState::CANCELLED, error)),
      },
    };
    let requests = search.scope.requests(search.token, text);
    // an answer can come back as soon as the request is on its way, before the write is seen to
    // end, so the search takes responses from here
    self.set_search_state(search, SearchState::IN_PROGRESS);
    let sent = tokio::select! {
      biased;
      // ended meanwhile, by a disconnect or by a limit an early answer reached
      () = ended(&mut outcome) => None,
      sent = self.write_server_messages(&requests) => Some(sent),
    };
    match sent {
      Some(Ok(())) => {
        tracing::info!(text, scope = ?search.scope, "search sent");
        // the timeout counts from the send, however long the write waited
        lock(&search.counts).deadline = Instant::now() + search.options.search_timeout;
      }
      Some(Err(error)) => {
        let error = match error {
          error @ ClientError::Timeout(_) => error,
          error => failed(&format!("search for {text}"), error),
        };
        return Err(self.end_search(search, SearchState::ERRORED, error));
      }
      // the loop below ends at once with how it ended
      None => tracing::debug!("search ended while its request was sent"),
    }
    let reason = loop {
      let deadline = lock(&search.counts).deadline;
      tokio::select! {
        biased;
        () = cancel.cancelled() => break SearchState::CANCELLED,
        reason = outcome.wait_for(Option::is_some) => {
          break reason
            .expect("the search holds the sender")
            .expect("waited for a reason");
        }
        () = sleep_until(deadline) => {
          // a response taken meanwhile moved the deadline
          if lock(&search.counts).deadline <= Instant::now() {
            break SearchState::TIMED_OUT;
          }
        }
      }
    };
    let reason = search.complete(reason);
    // a response taken before the end is handed over before the search returns
    drop(lock(&search.handler));
    drop(slot);
    if reason == SearchState::CANCELLED {
      return Err(self.end_search(search, reason, ClientError::Canceled));
    }
    self.set_search_state(search, SearchState::COMPLETED | reason);
    let snapshot = search.snapshot(&lock(&search.counts));
    tracing::info!(
      state = %snapshot.state,
      responses = snapshot.response_count,
      files = snapshot.file_count,
      "search completed"
    );
    Ok(snapshot)
  }

  /// ends `search` with `reason` and returns `error`, which the caller fails with
  fn end_search(&self, search: &Running, reason: SearchState, error: ClientError) -> ClientError {
    search.complete(reason);
    self.set_search_state(search, SearchState::COMPLETED | reason);
    tracing::info!(state = %(SearchState::COMPLETED | reason), %error, "search ended");
    error
  }

  /// moves `search` to `state`, calling its callback and then raising the event
  fn set_search_state(&self, search: &Running, state: SearchState) {
    let (previous, snapshot) = {
      let mut counts = lock(&search.counts);
      let previous = counts.state;
      counts.state = state;
      (previous, search.snapshot(&counts))
    };
    tracing::debug!(%previous, %state, "search state changed");
    if let Some(callback) = &search.options.state_changed {
      callback(previous, &snapshot);
    }
    self.emit(Event::SearchStateChanged {
      previous_state: previous,
      search: snapshot,
    });
  }

  /// Sends a search for `text` with `token` to everyone in `scope`, without waiting for
  /// responses. A search of several users is one request per user, written together.
  pub async fn send_search_request(
    &self,
    scope: &SearchScope,
    token: u32,
    text: &str,
  ) -> Result<(), ClientError> {
    non_blank("search text", text)?;
    scope.validate()?;
    let verb = format!("send search request with token {token}");
    self.require_logged_in(&verb)?;
    self
      .write_server_messages(&scope.requests(token, text))
      .await
      .map_err(|error| failed(&verb, error))?;
    tracing::debug!(token, ?scope, "search request sent");
    Ok(())
  }

  /// hands `response` to the search it names, which takes it if its options let it through
  pub(crate) fn search_response(&self, response: SearchResponse) {
    let Some(search) = self.searches().get(response.token) else {
      tracing::trace!(
        token = response.token,
        "search response for no active search, dropped"
      );
      return;
    };
    let response = match responses::filter(&search.options, response) {
      Ok(response) => response,
      Err(reason) => {
        return tracing::trace!(token = search.token, reason, "search response dropped");
      }
    };
    // taking a response and handing it over are one step, so a search that ends waits for the
    // responses it took
    let mut handler = lock(&search.handler);
    let Accepted { snapshot, response } = match responses::accept(&search, response) {
      Ok(accepted) => accepted,
      Err(reason) => {
        return tracing::trace!(token = search.token, reason, "search response dropped");
      }
    };
    tracing::trace!(
      username = response.username,
      files = response.files.len(),
      "search response taken"
    );
    handler(response.as_ref().clone());
    if let Some(received) = &search.options.response_received {
      received(&snapshot, &response);
    }
    self.emit(Event::SearchResponseReceived {
      search: snapshot,
      response,
    });
  }
}

/// Resolves once the search was ended by another task: a disconnect, or a limit a response
/// reached. The value's read guard is let go before this returns, since ending the search writes
/// the value.
async fn ended(outcome: &mut watch::Receiver<Option<SearchState>>) {
  if outcome.wait_for(Option::is_some).await.is_err() {
    // the search holds the sender, so this cannot end without a value
    std::future::pending::<()>().await;
  }
}

/// The query `text` makes, with short terms dropped when the options say so; it needs a term
fn search_query(text: &str, options: &SearchOptions) -> Result<SearchQuery, ClientError> {
  let mut query = SearchQuery::parse(text);
  if query.search_text().trim().is_empty() {
    return Err(ClientError::InvalidArgument(
      "The search text must not be empty".into(),
    ));
  }
  if query.terms.is_empty() {
    return Err(ClientError::InvalidArgument(
      "The search must contain at least one non-exclusion term".into(),
    ));
  }
  if options.remove_single_character_search_terms {
    query.terms.retain(|term| term.chars().count() > 1);
    if query.terms.is_empty() {
      return Err(ClientError::InvalidArgument(
        "The search must contain at least one non-exclusion term with length greater than 1".into(),
      ));
    }
  }
  Ok(query)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_query_needs_a_term_longer_than_one_character() {
    let options = SearchOptions::default();
    let query = search_query("a  daft\tpunk -live", &options).unwrap();
    assert_eq!(query.search_text(), "daft punk -live");
    assert!(search_query("-live -bootleg", &options).is_err());
    assert!(search_query("a b", &options).is_err());
    assert!(search_query("   ", &options).is_err());
    let keep_short = SearchOptions {
      remove_single_character_search_terms: false,
      ..SearchOptions::default()
    };
    assert_eq!(
      search_query("a b", &keep_short).unwrap().search_text(),
      "a b"
    );
  }

  #[test]
  fn a_user_search_is_one_request_per_user() {
    let scope = SearchScope::User(vec!["alice".into(), "bob".into()]);
    let requests = scope.requests(7, "daft punk");
    assert_eq!(requests.len(), 2);
    assert!(SearchScope::User(Vec::new()).validate().is_err());
    assert!(SearchScope::Rooms(vec![" ".into()]).validate().is_err());
    assert!(SearchScope::Rooms(Vec::new()).validate().is_err());
  }
}
