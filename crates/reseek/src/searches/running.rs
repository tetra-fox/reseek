//! One search while it runs: the client's state reports and the responses it takes are kept in
//! memory, each state change written and pushed at once, the counts at most every 250 ms, and
//! the end written with every response in one statement before anyone is told the search
//! completed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use api_types::live::LiveFrame;
use api_types::search::{Search, SearchResponse, SearchResponseReceived, SearchStates};
use chrono::Utc;
use soulseek_client::{ClientError, SearchState};
use soulseek_proto::peer;
use tokio::sync::{Notify, oneshot, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace, warn};

use super::{Searches, store, suspect};
use crate::live::{Live, Topic};
use crate::users::Directory;

/// How often the counts of a running search are written and pushed at most.
const COUNTS_EVERY: Duration = Duration::from_millis(250);

/// A search while its task runs.
pub(super) struct Running {
  /// stops the search
  pub cancel: CancellationToken,
  /// the search was stopped by a cancel or a delete, not by a lost connection
  pub cancelled: AtomicBool,
  /// true once the end is written, or found not to be
  pub done: watch::Sender<bool>,
  progress: Mutex<Progress>,
  changed: Notify,
}

/// What the client reported so far.
struct Progress {
  record: Search,
  responses: Vec<SearchResponse>,
  /// a state change not yet written
  moved: bool,
  /// told once the client took the search, or with the error it refused it with
  accepted: Option<oneshot::Sender<Result<(), ClientError>>>,
}

fn lock(progress: &Mutex<Progress>) -> MutexGuard<'_, Progress> {
  // every change under the lock is made whole before it is let go
  progress.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The search's state as the API writes it, from the client's.
fn state(state: SearchState) -> SearchStates {
  SearchStates::from_bits_retain(state.bits())
}

/// `file` as a search result shows it, with the size hint.
fn file(file: peer::File, locked: bool) -> api_types::search::File {
  let mut shown = api_types::search::File::from(file);
  shown.is_locked = locked;
  shown.suspect = suspect::hint(&shown);
  shown
}

/// A peer's response as a search result shows it, with the peer's `country`.
fn response(response: peer::SearchResponse, country: Option<String>) -> SearchResponse {
  let files: Vec<_> = response.files.into_iter().map(|f| file(f, false)).collect();
  let locked_files: Vec<_> = response
    .locked_files
    .into_iter()
    .map(|f| file(f, true))
    .collect();
  SearchResponse {
    username: response.username,
    country,
    token: response.token,
    has_free_upload_slot: response.has_free_upload_slot,
    upload_speed: response.upload_speed,
    queue_length: response.queue_length,
    file_count: u32::try_from(files.len()).unwrap_or(u32::MAX),
    files,
    locked_file_count: u32::try_from(locked_files.len()).unwrap_or(u32::MAX),
    locked_files,
  }
}

impl Running {
  /// The search `record`, not yet taken by the client, which says so on `accepted`.
  pub fn new(record: Search, accepted: oneshot::Sender<Result<(), ClientError>>) -> Self {
    Self {
      cancel: CancellationToken::new(),
      cancelled: AtomicBool::new(false),
      done: watch::channel(false).0,
      progress: Mutex::new(Progress {
        record,
        responses: Vec::new(),
        moved: false,
        accepted: Some(accepted),
      }),
      changed: Notify::new(),
    }
  }

  /// The search as it stands, without its responses.
  pub fn record(&self) -> Search {
    lock(&self.progress).record.clone()
  }

  /// The client moved the search to `next`. The first report says the client took it; the
  /// end is not taken here, but written with the responses once the client returns.
  pub fn state_changed(&self, next: SearchState) {
    let mut progress = lock(&self.progress);
    if let Some(accepted) = progress.accepted.take() {
      // the request waiting for this may have given up already
      let _ = accepted.send(Ok(()));
    }
    if next.contains(SearchState::COMPLETED) {
      return;
    }
    progress.record.state = state(next);
    progress.moved = true;
    drop(progress);
    self.changed.notify_one();
  }

  /// The client took `taken` into the search: it is kept with the responder's country from
  /// `users`, counted, and pushed when a stream follows the search.
  pub fn response_taken(&self, taken: peer::SearchResponse, live: &Live, users: &Directory) {
    let country = users.country_of(&taken.username);
    let taken = response(taken, country);
    trace!(
      username = taken.username,
      files = taken.file_count,
      "search response kept"
    );
    let id = lock(&self.progress).record.id;
    if live.followed(Topic::SearchResponses(id)) {
      live.publish(LiveFrame::SearchResponse(SearchResponseReceived {
        search_id: id,
        response: taken.clone(),
      }));
    }
    let mut progress = lock(&self.progress);
    let record = &mut progress.record;
    record.response_count = record.response_count.saturating_add(1);
    record.file_count = record.file_count.saturating_add(taken.file_count);
    record.locked_file_count = record
      .locked_file_count
      .saturating_add(taken.locked_file_count);
    progress.responses.push(taken);
    drop(progress);
    self.changed.notify_one();
  }

  /// The client refused the search before taking it; false when it had taken it, and the
  /// error is how it ended.
  pub fn refused(&self, error: ClientError) -> bool {
    match lock(&self.progress).accepted.take() {
      Some(accepted) => {
        // the request waiting for this may have given up already
        let _ = accepted.send(Err(error));
        true
      }
      None => false,
    }
  }
}

impl Searches {
  /// Follows `running` until `search`, the client's operation, returns: a state change is
  /// written and pushed at once, the counts at most every 250 ms. Then writes the end.
  pub(super) async fn follow(
    &self,
    running: Arc<Running>,
    search: impl Future<Output = Result<soulseek_client::Search, ClientError>>,
  ) {
    tokio::pin!(search);
    let mut written: Option<Instant> = None;
    let mut staged = false;
    let flush = tokio::time::sleep(Duration::ZERO);
    tokio::pin!(flush);
    let result = loop {
      tokio::select! {
        result = &mut search => break result,
        () = running.changed.notified() => {
          let moved = std::mem::take(&mut lock(&running.progress).moved);
          match written {
            Some(at) if !moved && at.elapsed() < COUNTS_EVERY => {
              if !staged {
                staged = true;
                flush.as_mut().reset(at + COUNTS_EVERY);
              }
            }
            _ => {
              self.write_progress(&running).await;
              written = Some(Instant::now());
              staged = false;
            }
          }
        }
        () = &mut flush, if staged => {
          self.write_progress(&running).await;
          written = Some(Instant::now());
          staged = false;
        }
      }
    };
    if let Err(error) = &result
      && running.refused(error.clone())
    {
      // the request that started it writes the refusal
      running.done.send_replace(true);
      return;
    }
    self.finish(&running, result).await;
    running.done.send_replace(true);
  }

  /// Writes and pushes the search's state and counts as they stand.
  async fn write_progress(&self, running: &Running) {
    let record = running.record();
    let written = record.clone();
    match self
      .db
      .write(move |conn| store::update(conn, &written))
      .await
    {
      Ok(true) => self.live.publish(LiveFrame::SearchUpdate(record)),
      Ok(false) => debug!("search deleted while it ran; its progress is not written"),
      Err(e) => warn!(error = %e, "search progress not written"),
    }
  }

  /// Writes how the search ended with its responses, then pushes it as completed.
  async fn finish(&self, running: &Running, result: Result<soulseek_client::Search, ClientError>) {
    let (mut record, responses) = {
      let mut progress = lock(&running.progress);
      (
        progress.record.clone(),
        std::mem::take(&mut progress.responses),
      )
    };
    let reason = match result {
      Ok(search) => state(search.state) - SearchStates::Completed,
      Err(ClientError::Cancelled) if running.cancelled.load(Ordering::Relaxed) => {
        SearchStates::Cancelled
      }
      Err(error) => {
        warn!(%error, "search failed");
        SearchStates::Errored
      }
    };
    record.state = SearchStates::Completed | reason;
    record.is_complete = true;
    record.ended_at = Some(Utc::now());
    let written = record.clone();
    let stored = self
      .db
      .write(move |conn| {
        let blob = store::compress(&responses)
          .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
        let found = store::finish(conn, &written, &blob)?;
        Ok(found.then_some(blob.len()))
      })
      .await;
    match stored {
      Ok(Some(bytes)) => {
        record.response_bytes = bytes as u64;
        debug!(
          state = %record.state,
          responses = record.response_count,
          files = record.file_count,
          bytes,
          "search stored"
        );
        self.live.publish(LiveFrame::SearchUpdate(record));
      }
      Ok(None) => debug!("search deleted while it ran; its end is not written"),
      Err(e) => error!(
        error = %e,
        "search responses not stored; the search is ended at the next start"
      ),
    }
  }
}

#[cfg(test)]
mod tests {
  use api_types::search::{SearchScope, SearchScopeType};
  use soulseek_client::SearchScope as ClientScope;
  use soulseek_proto::SearchQuery;
  use uuid::Uuid;

  use super::*;
  use crate::services::SearchService;
  use crate::testing::app;

  fn record() -> Search {
    Search {
      id: Uuid::new_v4(),
      search_text: "animals".into(),
      token: 7,
      scope: SearchScope {
        scope_type: SearchScopeType::Network,
        subjects: Vec::new(),
      },
      state: SearchStates::Requested,
      is_complete: false,
      started_at: Utc::now(),
      ended_at: None,
      response_count: 0,
      file_count: 0,
      locked_file_count: 0,
      response_bytes: 0,
      responses: None,
    }
  }

  #[tokio::test]
  async fn a_completed_report_changes_nothing_until_the_end_is_written() {
    let app = app("", &[]).await;
    let searches = app.state.searches.clone();
    let record = record();
    let (id, written) = (record.id, record.clone());
    searches
      .db
      .write(move |conn| store::insert(conn, &written))
      .await
      .unwrap();
    let (accepted, acceptance) = oneshot::channel();
    let running = Arc::new(Running::new(record, accepted));
    running.state_changed(SearchState::IN_PROGRESS);
    acceptance.await.unwrap().unwrap();
    lock(&running.progress).moved = false;

    let ended = SearchState::COMPLETED | SearchState::TIMED_OUT;
    running.state_changed(ended);
    {
      let progress = lock(&running.progress);
      assert_eq!(progress.record.state, SearchStates::InProgress);
      assert!(!progress.record.is_complete);
      assert!(!progress.moved, "nothing is staged to be written");
    }
    assert!(!*running.done.borrow());
    let stored = searches.get(id, false).await.unwrap();
    assert!(!stored.is_complete, "the end is not written yet");

    let search = soulseek_client::Search {
      query: SearchQuery::parse("animals"),
      scope: ClientScope::Network,
      token: 7,
      state: ended,
      response_count: 0,
      file_count: 0,
      locked_file_count: 0,
    };
    searches.follow(running.clone(), async { Ok(search) }).await;
    assert!(*running.done.borrow());
    let stored = searches.get(id, true).await.unwrap();
    assert_eq!(
      stored.state,
      SearchStates::Completed | SearchStates::TimedOut
    );
    assert!(stored.is_complete);
    assert_eq!(stored.responses, Some(Vec::new()));
  }

  #[tokio::test]
  async fn a_search_running_at_shutdown_ends_cancelled() {
    let app = app("", &[]).await;
    let searches = app.state.searches.clone();
    let record = record();
    let (id, written) = (record.id, record.clone());
    searches
      .db
      .write(move |conn| store::insert(conn, &written))
      .await
      .unwrap();
    let (accepted, _acceptance) = oneshot::channel();
    let running = Arc::new(Running::new(record, accepted));
    running.state_changed(SearchState::IN_PROGRESS);
    searches.running().insert(id, running.clone());
    let cancel = running.cancel.clone();
    let following = searches.clone();
    searches.tasks.spawn(async move {
      let search = async move {
        cancel.cancelled().await;
        Err(ClientError::Cancelled)
      };
      following.follow(running, search).await;
    });

    searches.stop().await;
    let stored = searches.get(id, false).await.unwrap();
    assert_eq!(
      stored.state,
      SearchStates::Completed | SearchStates::Cancelled
    );
  }

  #[tokio::test]
  async fn a_response_carries_the_country_the_directory_has_for_its_peer() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let notes = crate::testing::request(
      "PUT",
      "/api/v0/users/alice/notes",
      Some(serde_json::json!({ "notes": "has a row" })),
    );
    assert_eq!(app.send(notes).await.status, 204);
    let users = &app.state.users;
    users
      .countries_seen(vec![
        ("alice".into(), "NL".into()),
        ("bob".into(), "DE".into()),
      ])
      .await;
    let (accepted, _acceptance) = oneshot::channel();
    let running = Running::new(record(), accepted);
    for username in ["alice", "bob"] {
      let taken = peer::SearchResponse {
        username: username.into(),
        token: 7,
        files: Vec::new(),
        has_free_upload_slot: true,
        upload_speed: 0,
        queue_length: 0,
        locked_files: Vec::new(),
      };
      running.response_taken(taken, &app.state.live, users);
    }
    let countries: Vec<_> = lock(&running.progress)
      .responses
      .iter()
      .map(|response| response.country.clone())
      .collect();
    assert_eq!(countries, [Some("NL".to_string()), None], "bob has no row");
  }
}
