//! Searches we start on the network: each is recorded before it is sent, followed while peers
//! answer, and stored with every response it took once it ends. Searches that a stop left
//! running are ended at the next start, and finished ones older than `retention.search` are
//! deleted every five minutes.
//!
//! [`Searches`] is the engine; it answers the API's search calls and the search topic's
//! snapshot, and tells the download engine what a download's search was for. A search of the
//! rooms we are in or of our buddies names, once it starts, the rooms or buddies it went to.

mod running;
pub(crate) mod store;
mod suspect;
mod wishlist;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::search::{
  Search, SearchListQuery, SearchOutcome, SearchRequest, SearchResponse, SearchScope,
  SearchScopeType, SearchState,
};
use async_trait::async_trait;
use cascade::Watcher;
use chrono::Utc;
use reseek_shares::lock;
use soulseek_client::{ClientError, SearchOptions, SearchStateChanged};
use soulseek_proto::SearchQuery;
use tokio::sync::oneshot;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, info_span, warn};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::config::Options;
use crate::db::{Database, DbError, Location, shifted};
use crate::live::Live;
use crate::network::Network;
use crate::services::{Canceled, SearchService};
use crate::state::StateOwner;
use crate::users::Directory;
use running::Running;

/// How long deleting a running search waits for it to end.
const ENDING: Duration = Duration::from_secs(5);

/// How long the stop waits for running searches to write how they ended.
const STOPPING: Duration = Duration::from_secs(5);

/// The answer to a search started while another start is under way.
const ONE_AT_A_TIME: &str =
  "Only one concurrent operation is permitted. Wait until the previous request completes";

/// The search engine.
pub struct Searches {
  this: Weak<Searches>,
  db: Database,
  network: Arc<Network>,
  /// where a responder's country is found, and our buddies
  users: Arc<Directory>,
  /// the rooms we are in
  state: StateOwner,
  live: Live,
  config: Watcher<Options>,
  /// held while a search starts; a second start meanwhile is refused
  starting: tokio::sync::Mutex<()>,
  running: Mutex<HashMap<Uuid, Arc<Running>>>,
  /// every search's task, so the stop waits for them
  tasks: TaskTracker,
}

fn internal(error: DbError) -> ApiError {
  ApiError::internal(error)
}

fn not_found(id: Uuid) -> ApiError {
  ApiError::not_found(format!("No search has the id {id}"))
}

/// `query` as it is sent: a term of one character matches too much to be sent, so it is left
/// out.
pub fn sendable(mut query: SearchQuery) -> SearchQuery {
  query.terms.retain(|term| term.chars().count() > 1);
  query
}

/// The text a search for `text` sends: the terms in order, then the exclusions.
fn normalized(text: &str) -> String {
  sendable(SearchQuery::parse(text)).search_text()
}

/// Where the client sends a search with `scope`, whose subjects name the rooms or users.
fn client_scope(scope: &SearchScope) -> soulseek_client::SearchScope {
  use soulseek_client::SearchScope as To;
  match scope.scope_type {
    SearchScopeType::Network => To::Network,
    SearchScopeType::Wishlist => To::Wishlist,
    SearchScopeType::User | SearchScopeType::Buddies => To::User(scope.subjects.clone()),
    SearchScopeType::Room | SearchScopeType::Rooms => To::Rooms(scope.subjects.clone()),
  }
}

/// The client's options for `request`, which waits `search_timeout` for another response.
fn options(
  request: &SearchRequest,
  search_timeout: Duration,
  state_changed: SearchStateChanged,
) -> SearchOptions {
  SearchOptions {
    search_timeout,
    response_limit: request.response_limit,
    file_limit: request.file_limit,
    filter_responses: request.filter_responses,
    minimum_response_file_count: request.minimum_response_file_count,
    maximum_peer_queue_length: request.maximum_peer_queue_length.unwrap_or(u32::MAX),
    minimum_peer_upload_speed: request.minimum_peer_upload_speed,
    remove_single_character_search_terms: true,
    state_changed: Some(state_changed),
    ..SearchOptions::default()
  }
}

/// A stored blob that could not be read back.
fn unreadable(error: std::io::Error) -> rusqlite::Error {
  rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, error.into())
}

impl Searches {
  /// Opens `search.db` in `location`; searches go out through `network`, their responders'
  /// countries and our buddies are found in `users`, the rooms we are in in `state`, and they
  /// are pushed to `live`.
  pub async fn open(
    location: &Location,
    network: Arc<Network>,
    users: Arc<Directory>,
    state: StateOwner,
    live: Live,
    config: Watcher<Options>,
  ) -> Result<Arc<Self>, DbError> {
    let db = Database::open(&store::SCHEMA, location).await?;
    Ok(Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db,
      network,
      users,
      state,
      live,
      config,
      starting: tokio::sync::Mutex::new(()),
      running: Mutex::new(HashMap::new()),
      tasks: TaskTracker::new(),
    }))
  }

  /// `search.db`.
  pub fn database(&self) -> &Database {
    &self.db
  }

  fn running(&self) -> MutexGuard<'_, HashMap<Uuid, Arc<Running>>> {
    lock(&self.running)
  }

  fn this(&self) -> Arc<Self> {
    self
      .this
      .upgrade()
      .expect("the engine is alive while a method runs on it")
  }

  /// Ends the searches the last run left running, before the client connects: their
  /// responses were only in memory, so they end timed out with none.
  pub async fn end_dangling(&self) {
    let now = Utc::now();
    match self
      .db
      .write(move |conn| store::end_dangling(conn, now))
      .await
    {
      Ok(0) => {}
      Ok(ended) => debug!(ended, "searches left running at the last stop ended"),
      Err(e) => warn!(error = %e, "searches left running at the last stop not ended"),
    }
  }

  /// Deletes the searches that ended longer ago than `retention.search`, pushing each.
  pub async fn prune(&self) -> Result<usize, DbError> {
    let Some(age) = self.config.current().values.retention.search else {
      return Ok(0);
    };
    // an age too long for chrono deletes nothing, as the earliest time stored bounds it
    let age = chrono::Duration::from_std(age).unwrap_or(chrono::Duration::MAX);
    let before = shifted(Utc::now(), -age);
    let pruned = self
      .db
      .write(move |conn| store::prune(conn, before))
      .await?;
    let count = pruned.len();
    for search in pruned {
      self.live.publish(LiveFrame::SearchDelete(search));
    }
    Ok(count)
  }

  /// Every search, oldest first, without responses: the search topic's snapshot.
  pub async fn snapshot(&self) -> Result<Vec<Search>, DbError> {
    self
      .db
      .read(|conn| store::list(conn, &SearchListQuery::default()))
      .await
  }

  /// What the search `id` searched for, when it is kept.
  pub async fn text_of(&self, id: Uuid) -> Result<Option<String>, DbError> {
    self.db.read(move |conn| store::text(conn, id)).await
  }

  /// Ends every running search as canceled at shutdown, before the client is stopped, and
  /// waits for each to write how it ended.
  pub async fn stop(&self) {
    for running in self.running().values() {
      running.canceled.store(true, Ordering::Relaxed);
      running.cancel.cancel();
    }
    self.tasks.close();
    if tokio::time::timeout(STOPPING, self.tasks.wait())
      .await
      .is_err()
    {
      warn!(
        running = self.tasks.len(),
        "searches still running at shutdown"
      );
    }
  }

  /// Writes a search the client refused before sending it as errored and pushes it.
  async fn refused(&self, mut record: Search) {
    record.state = SearchState::Completed(SearchOutcome::Errored);
    record.ended_at = Some(record.started_at);
    let written = record.clone();
    match self
      .db
      .write(move |conn| store::update(conn, &written))
      .await
    {
      Ok(_) => self.live.publish(LiveFrame::SearchUpdate(record)),
      Err(e) => warn!(error = %e, "refused search not written"),
    }
  }

  /// `scope` with the rooms or buddies a search with it goes to: every room we are in for a
  /// rooms scope that names none, and our buddies online now for a buddies scope, which names
  /// no one itself. A rooms scope that names a room we are not in is refused, and so is a scope
  /// that would reach no one.
  fn reached(&self, scope: &SearchScope) -> Result<SearchScope, ApiError> {
    let subjects = match scope.scope_type {
      SearchScopeType::Rooms => {
        let joined = self.state.current().rooms.clone();
        if let Some(room) = scope.subjects.iter().find(|room| !joined.contains(room)) {
          return Err(ApiError::bad_request(format!(
            "The room {room} is not joined"
          )));
        }
        if scope.subjects.is_empty() && joined.is_empty() {
          return Err(ApiError::conflict(
            "No room is joined",
            Some("Join a room to search the rooms you are in".to_string()),
          ));
        }
        if scope.subjects.is_empty() {
          joined
        } else {
          scope.subjects.clone()
        }
      }
      SearchScopeType::Buddies => {
        if !scope.subjects.is_empty() {
          return Err(ApiError::bad_request(
            "A buddies search names no one: it goes to every buddy online",
          ));
        }
        if self.users.buddy_names().is_empty() {
          return Err(ApiError::conflict("There are no buddies to search", None));
        }
        let online = self.users.buddies_online();
        if online.is_empty() {
          return Err(ApiError::conflict("No buddy is online", None));
        }
        online
      }
      _ => return Ok(scope.clone()),
    };
    Ok(SearchScope {
      scope_type: scope.scope_type,
      subjects,
    })
  }

  /// The answer to a search the client refused with `error`.
  fn refusal(&self, error: ClientError) -> ApiError {
    match error {
      ClientError::InvalidOperation(_) => ApiError::conflict(
        format!(
          "The server connection must be connected and logged in to perform a search \
           (currently: {})",
          self.network.client().state()
        ),
        None,
      ),
      ClientError::InvalidArgument(message) => ApiError::bad_request(message),
      error @ ClientError::DuplicateToken(_) => ApiError::bad_request(error.to_string()),
      other => ApiError::internal(other),
    }
  }
}

#[async_trait]
impl SearchService for Searches {
  async fn start(
    &self,
    request: SearchRequest,
    search_timeout: Duration,
  ) -> Result<Search, ApiError> {
    let Ok(_starting) = self.starting.try_lock() else {
      return Err(ApiError::too_many(ONE_AT_A_TIME));
    };
    let client = self.network.client().clone();
    let id = request.id.unwrap_or_else(Uuid::new_v4);
    let scope = self.reached(&request.scope)?;
    let record = Search {
      id,
      search_text: normalized(&request.search_text),
      token: client.next_token(),
      scope: scope.clone(),
      state: SearchState::Requested,
      started_at: Utc::now(),
      ended_at: None,
      response_count: 0,
      file_count: 0,
      locked_file_count: 0,
      response_bytes: 0,
      responses: None,
    };
    let written = record.clone();
    if !self
      .db
      .write(move |conn| store::insert(conn, &written))
      .await
      .map_err(internal)?
    {
      return Err(ApiError::conflict(
        format!("A search with the id {id} already exists"),
        None,
      ));
    }
    self.live.publish(LiveFrame::SearchCreate(record.clone()));

    let (accepted, acceptance) = oneshot::channel();
    let running = Arc::new(Running::new(record.clone(), accepted));
    self.running().insert(id, running.clone());
    let reported = running.clone();
    let state_changed: SearchStateChanged =
      Arc::new(move |_, snapshot| reported.state_changed(snapshot.state));
    let options = options(&request, search_timeout, state_changed);
    let scope = client_scope(&scope);
    let (text, token) = (record.search_text.clone(), record.token);
    let this = self.this();
    let span = info_span!("search", %id);
    self.tasks.spawn(
      async move {
        let taker = running.clone();
        let (live, users) = (this.live.clone(), this.users.clone());
        let search = client.search_with(
          &text,
          scope,
          Some(token),
          options,
          move |response| taker.response_taken(response, &live, &users),
          &running.cancel,
        );
        this.follow(running.clone(), search).await;
        this.running().remove(&id);
      }
      .instrument(span),
    );

    match acceptance.await {
      Ok(Ok(())) => {
        let record = self
          .running()
          .get(&id)
          .map_or(record, |running| running.record());
        self.live.publish(LiveFrame::SearchUpdate(record.clone()));
        Ok(record)
      }
      Ok(Err(error)) => {
        debug!(%id, %error, "search refused by the client");
        self.refused(record).await;
        Err(self.refusal(error))
      }
      Err(_) => {
        // the task ended without the client saying either; it holds the sender until then
        self.refused(record).await;
        Err(ApiError::internal("the search ended before it started"))
      }
    }
  }

  async fn list(&self, query: SearchListQuery) -> Result<Vec<Search>, ApiError> {
    self
      .db
      .read(move |conn| store::list(conn, &query))
      .await
      .map_err(internal)
  }

  async fn get(&self, id: Uuid, include_responses: bool) -> Result<Search, ApiError> {
    let mut search = self
      .db
      .read(move |conn| store::get(conn, id))
      .await
      .map_err(internal)?
      .ok_or_else(|| not_found(id))?;
    if include_responses {
      search.responses = Some(self.responses(id).await?);
    }
    Ok(search)
  }

  async fn responses(&self, id: Uuid) -> Result<Vec<SearchResponse>, ApiError> {
    self
      .db
      .read(move |conn| {
        let Some(stored) = store::responses(conn, id)? else {
          return Ok(None);
        };
        // a search still running, or one whose responses were lost, has none stored
        let responses = match stored {
          Some(blob) => store::decompress(&blob).map_err(unreadable)?,
          None => Vec::new(),
        };
        Ok(Some(responses))
      })
      .await
      .map_err(internal)?
      .ok_or_else(|| not_found(id))
  }

  async fn cancel(&self, id: Uuid) -> Result<Canceled, ApiError> {
    if let Some(running) = self.running().get(&id).cloned() {
      running.canceled.store(true, Ordering::Relaxed);
      running.cancel.cancel();
      return Ok(Canceled::Now);
    }
    let exists = self
      .db
      .read(move |conn| store::get(conn, id))
      .await
      .map_err(internal)?
      .is_some();
    if exists {
      Ok(Canceled::Already)
    } else {
      Err(not_found(id))
    }
  }

  async fn delete(&self, id: Uuid) -> Result<(), ApiError> {
    let running = self.running().get(&id).cloned();
    if let Some(running) = running {
      running.canceled.store(true, Ordering::Relaxed);
      running.cancel.cancel();
      let mut done = running.done.subscribe();
      if tokio::time::timeout(ENDING, done.wait_for(|done| *done))
        .await
        .is_err()
      {
        warn!(%id, "search did not end in time; deleted while it runs");
      }
    }
    let deleted = self
      .db
      .write(move |conn| store::delete(conn, id))
      .await
      .map_err(internal)?
      .ok_or_else(|| not_found(id))?;
    self.live.publish(LiveFrame::SearchDelete(deleted));
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{TRUSTED, app, request};
  use serde_json::json;

  #[tokio::test]
  async fn a_search_refused_offline_is_kept_ended_and_its_id_stays_taken() {
    let app = app(TRUSTED, &[]).await;
    let id = "00000000-0000-4000-8000-0000000000aa";
    let start = || {
      request(
        "POST",
        "/api/v0/searches",
        Some(json!({ "id": id, "searchText": "pink  floyd -live" })),
      )
    };
    let refused = app.send(start()).await;
    assert_eq!(refused.status, 409, "Not logged in");
    assert!(
      refused.json()["error"]
        .as_str()
        .unwrap()
        .contains("must be connected and logged in")
    );
    let kept = app
      .send(request("GET", &format!("/api/v0/searches/{id}"), None))
      .await
      .json();
    assert_eq!(kept["state"], "Completed, Errored");
    assert_eq!(kept["searchText"], "pink floyd -live");
    assert_eq!(kept["endedAt"], kept["startedAt"]);
    assert_eq!(app.send(start()).await.status, 409, "the id is taken");

    let responses = app
      .send(request(
        "GET",
        &format!("/api/v0/searches/{id}/responses"),
        None,
      ))
      .await;
    assert_eq!(responses.status, 200);
    assert_eq!(responses.json(), json!([]), "none were ever stored");
    let cancel = request("PUT", &format!("/api/v0/searches/{id}"), None);
    assert_eq!(app.send(cancel).await.status, 304, "it ended already");
    let delete = || request("DELETE", &format!("/api/v0/searches/{id}"), None);
    assert_eq!(app.send(delete()).await.status, 204);
    assert_eq!(app.send(delete()).await.status, 404);
    let gone = request("PUT", &format!("/api/v0/searches/{id}"), None);
    assert_eq!(app.send(gone).await.status, 404);
  }

  #[test]
  fn the_text_sent_is_the_terms_then_the_exclusions() {
    assert_eq!(normalized("pink  floyd\t-live --live"), "pink floyd -live");
    assert_eq!(
      normalized("a animals -x"),
      "animals -x",
      "one character is too short"
    );
    assert_eq!(normalized("é pink"), "pink", "characters, not bytes");
  }

  #[tokio::test]
  async fn a_rooms_search_reaches_every_room_joined_and_a_buddies_search_the_buddies_online() {
    let app = app("", &[]).await;
    let searches = &app.state.searches;
    let reached = |scope_type, subjects: &[&str]| {
      searches.reached(&SearchScope {
        scope_type,
        subjects: subjects.iter().map(|s| s.to_string()).collect(),
      })
    };
    assert!(
      reached(SearchScopeType::Rooms, &[]).is_err(),
      "no room joined"
    );
    app
      .state
      .state
      .update(|state| state.rooms = vec!["indie".into(), "jazz".into()]);
    assert_eq!(
      reached(SearchScopeType::Rooms, &[]).unwrap().subjects,
      ["indie", "jazz"]
    );
    assert_eq!(
      reached(SearchScopeType::Rooms, &["jazz"]).unwrap().subjects,
      ["jazz"]
    );
    assert!(reached(SearchScopeType::Rooms, &["metal"]).is_err());

    let users = &app.state.users;
    assert!(reached(SearchScopeType::Buddies, &[]).is_err(), "no buddy");
    for name in ["alice", "bob"] {
      users.set_buddy(name, Default::default()).await.unwrap();
    }
    assert!(
      reached(SearchScopeType::Buddies, &[]).is_err(),
      "no buddy known online"
    );
    users.privileged_listed(&["alice".to_string(), "bob".to_string()]);
    users.status_received("alice", api_types::users::UserPresence::Away, false);
    users.status_received("bob", api_types::users::UserPresence::Offline, false);
    assert_eq!(
      reached(SearchScopeType::Buddies, &[]).unwrap().subjects,
      ["alice"]
    );
    assert!(
      reached(SearchScopeType::Buddies, &["alice"]).is_err(),
      "a buddies search names no one itself"
    );
  }
}
