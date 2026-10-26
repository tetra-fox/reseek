//! Answering other users' searches from what we share. A request waits for one of a fixed
//! number of workers, and is dropped when too many wait already. A worker checks the requester
//! against the blocks it remembers, the request against the configured filters, and that it
//! names something to look for; then it queries the shares, leaving out what the request and
//! the server exclude. Files found are offered to a requester who is online and not blocked at
//! their address, with our upload speed and the place they would take in our upload queue.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};

use api_types::blocking::BlockScope;
use regex::Regex;
use soulseek_client::{ClientError, LogLimiter, SearchReply};
use soulseek_proto::SearchQuery;
use soulseek_proto::peer::{File, SearchResponse};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use super::{Edges, blocked, forecast, log_refused, millis};
use crate::config::{Options, patterns};
use crate::telemetry::catalogue::SearchMetrics;
use crate::telemetry::registry::Gauge;

/// How long the share query of one request may take before the request is dropped.
const QUERY_BUDGET: Duration = Duration::from_secs(10);

/// Requests dropped for too many waiting, which comes in bursts.
static CROWDED: LogLimiter = LogLimiter::new();
/// Requests dropped for a share query that took too long.
static SLOW: LogLimiter = LogLimiter::new();
/// Requests a request filter matched.
static FILTERED: LogLimiter = LogLimiter::new();
/// Our own statistics not fetched before a response.
static UNREFRESHED: LogLimiter = LogLimiter::new();

/// The workers that answer other users' searches, the requests waiting for one, and the
/// filters that turn requests away.
pub struct IncomingSearches {
  /// the workers, as many as the daemon started with
  slots: Semaphore,
  /// requests waiting for a worker
  waiting: AtomicUsize,
  /// the request filters as last compiled
  filters: RwLock<Arc<[Regex]>>,
  /// whether the filters match case, as the daemon started with
  case_sensitive: bool,
  /// ends the wait of every waiting request when the daemon stops
  shutdown: CancellationToken,
}

impl IncomingSearches {
  /// The workers and filters `options` give at startup; a waiting request gives up when
  /// `shutdown` is cancelled.
  pub fn new(options: &Options, shutdown: CancellationToken) -> Self {
    let case_sensitive = options.flags.case_sensitive_regex;
    Self {
      slots: Semaphore::new(options.throttling.search.incoming.concurrency as usize),
      waiting: AtomicUsize::new(0),
      filters: RwLock::new(compile(&options.filters.search.request, case_sensitive)),
      case_sensitive,
      shutdown,
    }
  }

  /// How many requests wait for a worker now.
  pub fn waiting(&self) -> usize {
    self.waiting.load(Ordering::Acquire)
  }

  /// Compiles the request filters again from `patterns`, matching case as at startup.
  pub fn refilter(&self, patterns: &[String]) {
    let compiled = compile(patterns, self.case_sensitive);
    *self.filters.write().unwrap_or_else(PoisonError::into_inner) = compiled;
    info!(
      filters = patterns.len(),
      "Updated and re-compiled search response filters"
    );
  }

  /// Whether a request filter matches `text`.
  fn filtered(&self, text: &str) -> bool {
    // the filters are replaced whole, so even a poisoned lock holds a complete set
    let filters = self
      .filters
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .clone();
    filters.iter().any(|filter| filter.is_match(text))
  }

  /// A place among the waiting requests, shown in `depth`, unless `limit` or more wait already.
  fn wait<'a>(&'a self, limit: u32, depth: &'a Gauge) -> Option<Waiting<'a>> {
    let limit = limit as usize;
    self
      .waiting
      .try_update(Ordering::AcqRel, Ordering::Acquire, |waiting| {
        (waiting < limit).then_some(waiting + 1)
      })
      .ok()?;
    Some(Waiting {
      searches: self,
      depth,
    })
  }
}

/// A request's place among those waiting for a worker. Dropping it leaves, whether the request
/// got a worker, was abandoned or the daemon stops, and shows how many still wait.
struct Waiting<'a> {
  searches: &'a IncomingSearches,
  depth: &'a Gauge,
}

impl Drop for Waiting<'_> {
  fn drop(&mut self) {
    let left = self.searches.waiting.fetch_sub(1, Ordering::AcqRel) - 1;
    self.depth.set(left as f64);
  }
}

/// The filters `patterns` make, matching case only when `case_sensitive`. Validation refuses a
/// pattern that does not compile, so one left out here is logged.
fn compile(patterns: &[String], case_sensitive: bool) -> Arc<[Regex]> {
  patterns
    .iter()
    .filter_map(|pattern| {
      patterns::compile(pattern, case_sensitive)
        .inspect_err(|e| warn!(filter = pattern, error = %e, "search request filter not used"))
        .ok()
    })
    .collect()
}

/// Counts a request dropped for want of capacity.
fn dropped(metrics: &SearchMetrics) {
  metrics.requests_dropped.inc();
  metrics.request_drop_rate.count_up(1);
}

/// Another user's search reached us: the files we offer them, or `None`.
pub async fn search(
  edges: &Edges,
  username: &str,
  token: u32,
  query: &SearchQuery,
) -> Option<SearchReply> {
  let metrics = &edges.telemetry.metrics().search;
  metrics.requests_received.inc();
  metrics.request_receive_rate.count_up(1);
  let options = edges.config.current();
  let limit = options.values.throttling.search.incoming.circuit_breaker;
  let Some(waiting) = edges.searches.wait(limit, &metrics.request_queue_depth) else {
    dropped(metrics);
    if let Some(suppressed) = CROWDED.allow() {
      warn!(
        username,
        token, limit, suppressed, "too many searches waiting; search request dropped"
      );
    }
    return None;
  };
  let _worker = tokio::select! {
    worker = edges.searches.slots.acquire() => worker.expect("the workers are never closed"),
    () = edges.searches.shutdown.cancelled() => return None,
  };
  drop(waiting);
  let started = Instant::now();
  let response = answer(edges, &options.values, username, token, query, started).await;
  let elapsed = millis(started);
  metrics.response_latency.observe(elapsed);
  metrics.response_latency_current.update(elapsed);
  let response = response?;
  metrics.responses_sent.inc();
  metrics.response_send_rate.count_up(1);
  Some(SearchReply::Response(response))
}

/// The response to `username`'s search, worked out by a worker since `started`.
async fn answer(
  edges: &Edges,
  options: &Options,
  username: &str,
  token: u32,
  query: &SearchQuery,
  started: Instant,
) -> Option<SearchResponse> {
  let files = matches(edges, options, username, token, query, started).await?;
  // the network goes only when the daemon stops
  let network = edges.network.upgrade()?;
  let client = network.client();
  let Some(own) = client.username() else {
    trace!(username, token, "logged out before answering a search");
    return None;
  };
  if !answerable(edges, username, client.get_user_endpoint(username).await) {
    return None;
  }
  if let Err(e) = network.refresh_user_statistics(false).await
    && let Some(suppressed) = UNREFRESHED.allow()
  {
    warn!(error = %e, suppressed, "own statistics not refreshed; the last ones are sent");
  }
  let forecast = forecast(edges, username);
  let upload_speed = network.state.current().user.statistics.average_speed;
  trace!(
    username,
    token,
    files = files.len(),
    query = query.search_text(),
    "Sending search response"
  );
  Some(SearchResponse {
    username: own,
    token,
    files,
    has_free_upload_slot: forecast.free_slots > 0,
    upload_speed,
    queue_length: forecast.position,
    locked_files: Vec::new(),
  })
}

/// The files we would offer for `username`'s search, `None` when the requester is known to be
/// blocked, a request filter matches, the query names nothing to look for or nothing matches.
async fn matches(
  edges: &Edges,
  options: &Options,
  username: &str,
  token: u32,
  query: &SearchQuery,
  started: Instant,
) -> Option<Vec<File>> {
  let metrics = &edges.telemetry.metrics().search;
  if edges
    .users
    .remembers_blocked(username, BlockScope::Searches)
  {
    log_refused(username, BlockScope::Searches);
    return None;
  }
  let text = query.search_text();
  if edges.searches.filtered(&text) {
    if let Some(suppressed) = FILTERED.allow() {
      debug!(
        username,
        token,
        query = text,
        suppressed,
        "search request matches a request filter; not answered"
      );
    }
    return None;
  }
  if query.terms.is_empty() {
    trace!(
      username,
      token,
      query = text,
      "search names only exclusions"
    );
    return None;
  }
  let filtering = millis(started);
  metrics.filter_latency.observe(filtering);
  metrics.filter_latency_current.update(filtering);
  // the network goes only when the daemon stops
  let excluded = edges.network.upgrade()?.excluded_phrases();
  let limit = options.throttling.search.incoming.response_file_limit as usize;
  let files = query_shares(edges, &excluding(query, &excluded), limit, QUERY_BUDGET).await?;
  let querying = millis(started) - filtering;
  metrics.query_latency.observe(querying);
  metrics.query_latency_current.update(querying);
  if files.is_empty() {
    trace!(username, token, query = text, "no shared file matches");
    return None;
  }
  Some(files)
}

/// `query` with the phrases the server excludes added to its exclusions, each once.
fn excluding(query: &SearchQuery, phrases: &[String]) -> SearchQuery {
  let mut exclusions = query.exclusions.clone();
  for phrase in phrases {
    if !exclusions.contains(phrase) {
      exclusions.push(phrase.clone());
    }
  }
  SearchQuery {
    terms: query.terms.clone(),
    exclusions,
  }
}

/// The files `query` matches, at most `limit` of them; `None` when the shares take longer than
/// `budget` to answer, which drops the request.
async fn query_shares(
  edges: &Edges,
  query: &SearchQuery,
  limit: usize,
  budget: Duration,
) -> Option<Vec<File>> {
  match tokio::time::timeout(budget, edges.shares.search(query, Some(limit))).await {
    Ok(files) => Some(files),
    Err(_) => {
      dropped(&edges.telemetry.metrics().search);
      if let Some(suppressed) = SLOW.allow() {
        warn!(
          query = query.search_text(),
          budget_ms = budget.as_millis() as u64,
          suppressed,
          "share query took too long; search request dropped"
        );
      }
      None
    }
  }
}

/// Whether `username`, whose address the server gave in `lookup`, is online and not blocked
/// from searches at that address.
fn answerable(edges: &Edges, username: &str, lookup: Result<SocketAddr, ClientError>) -> bool {
  match lookup {
    Ok(endpoint) => !blocked(edges, username, Some(endpoint), BlockScope::Searches),
    Err(ClientError::UserOffline(_)) => {
      trace!(username, "the requester went offline; no search response");
      false
    }
    Err(e) => {
      debug!(username, error = %e, "the requester's address was not found; no search response");
      false
    }
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use serde_json::json;

  use super::*;
  use crate::network::resolvers::tests::{edges, exposed};
  use crate::testing::{TestApp, app, request};

  /// A daemon sharing one album from `dir`, with the request filters `filters`, one worker
  /// and the fewest waiting requests the options allow.
  async fn shared(dir: &Path, filters: &str) -> TestApp {
    let album = dir.join("Music").join("Abbey Road");
    std::fs::create_dir_all(&album).unwrap();
    std::fs::write(album.join("01 - Come Together.mp3"), "one").unwrap();
    std::fs::write(album.join("02 - Something.mp3"), "two").unwrap();
    let config = format!(
      "remote_configuration = true\n\
       [shares]\ndirectories = [\"{}\"]\n\
       [filters.search]\nrequest = [{filters}]\n\
       [throttling.search.incoming]\nconcurrency = 1\ncircuit_breaker = 100\n\
       [web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
       [transfers.groups.blacklisted]\ncidrs = [\"192.0.2.0/24\"]\n",
      dir.join("Music").display()
    );
    app(&config, &[]).await
  }

  async fn found(edges: &Edges, username: &str, text: &str) -> Option<Vec<String>> {
    let options = edges.config.current();
    let query = SearchQuery::parse(text);
    let files = matches(edges, &options.values, username, 1, &query, Instant::now()).await?;
    Some(
      files
        .into_iter()
        .map(|file| file.filename.as_str().to_string())
        .collect(),
    )
  }

  /// The value of the metric line that starts with `name `.
  fn metric(app: &TestApp, name: &str) -> f64 {
    let text = exposed(app);
    let line = text
      .lines()
      .find(|line| line.starts_with(&format!("{name} ")))
      .unwrap_or_else(|| panic!("no {name} in {text}"));
    line.rsplit(' ').next().unwrap().parse().unwrap()
  }

  #[tokio::test]
  async fn a_search_finds_what_nothing_turns_away() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path(), "\"^something$\"").await;
    let edges = edges(&app);
    assert_eq!(
      found(&edges, "alice", "abbey -something").await.unwrap(),
      ["Music\\Abbey Road\\01 - Come Together.mp3"]
    );
    assert_eq!(
      found(&edges, "alice", "help").await,
      None,
      "nothing matches"
    );
    assert_eq!(
      found(&edges, "alice", "-abbey").await,
      None,
      "only exclusions"
    );
    assert_eq!(
      found(&edges, "alice", "SOMETHING").await,
      None,
      "a filter ignores case"
    );
    assert_eq!(
      metric(&app, "reseek_search_incoming_filter_latency_count"),
      2.0
    );
    assert_eq!(
      metric(&app, "reseek_search_incoming_query_latency_count"),
      2.0
    );

    // the server's excluded phrases leave out what they cover
    app
      .state
      .network
      .exclude(vec!["come together".into(), "something".into()]);
    assert_eq!(found(&edges, "alice", "abbey").await, None);
    app.state.network.exclude(Vec::new());

    // a user known to be blocked is turned away before anything is looked up
    let block = json!({ "scopes": ["searches"] });
    let answer = app
      .send(request("PUT", "/api/v0/users/mallory/block", Some(block)))
      .await;
    assert_eq!(answer.status, 200);
    assert!(found(&edges, "mallory", "abbey").await.is_some());
    assert!(
      app
        .state
        .users
        .is_blocked("mallory", None, BlockScope::Searches)
    );
    assert_eq!(found(&edges, "mallory", "abbey").await, None);
  }

  #[tokio::test]
  async fn spaces_and_dashes_split_a_query_as_other_clients_send_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path(), "").await;
    let edges = edges(&app);
    let both = [
      "Music\\Abbey Road\\01 - Come Together.mp3",
      "Music\\Abbey Road\\02 - Something.mp3",
    ];
    assert_eq!(found(&edges, "alice", "abbey  road").await.unwrap(), both);
    assert_eq!(
      found(&edges, "alice", "abbey - road").await.unwrap(),
      both,
      "a lone dash is a term with no word, which is passed over"
    );
    assert_eq!(
      found(&edges, "alice", "abbey --come").await.unwrap(),
      [both[1]],
      "every leading dash of an exclusion goes"
    );
  }

  #[test]
  fn the_servers_phrases_join_the_exclusions_once() {
    let query = excluding(
      &SearchQuery::parse("abbey -live -Demo"),
      &["live".into(), "bootleg".into()],
    );
    assert_eq!(query.terms, ["abbey"]);
    assert_eq!(query.exclusions, ["live", "Demo", "bootleg"]);
  }

  #[tokio::test]
  async fn the_requester_must_be_online_and_not_blocked_at_their_address() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path(), "").await;
    let edges = edges(&app);
    let at =
      |ip: &str| -> Result<SocketAddr, ClientError> { Ok(format!("{ip}:2234").parse().unwrap()) };
    assert!(answerable(&edges, "alice", at("198.51.100.7")));
    assert!(!answerable(&edges, "alice", at("192.0.2.9")));
    let offline = ClientError::UserOffline("User alice appears to be offline".into());
    assert!(!answerable(&edges, "alice", Err(offline)));
  }

  #[tokio::test]
  async fn a_live_filter_change_still_ignores_case() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path(), "").await;
    let edges = edges(&app);
    assert!(found(&edges, "alice", "abbey").await.is_some());
    let change = json!({ "set": { "filters.search.request": ["^ABBEY$"] } });
    let answer = app
      .send(request("PATCH", "/api/v0/options", Some(change)))
      .await;
    assert_eq!(answer.status, 200, "{:?}", answer.body);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !edges.searches.filtered("ABBEY") {
      assert!(Instant::now() < deadline, "the filters never changed");
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(found(&edges, "alice", "abbey").await, None);
  }

  #[tokio::test]
  async fn the_breaker_drops_at_the_limit_and_an_abandoned_wait_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path(), "").await;
    let edges = edges(&app);
    let worker = edges.searches.slots.acquire().await.unwrap();
    let query = SearchQuery::parse("abbey");
    let waiting: Vec<_> = (0..100)
      .map(|n| {
        let edges = edges.clone();
        let query = query.clone();
        tokio::spawn(async move { search(&edges, "alice", n, &query).await })
      })
      .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    while edges.searches.waiting() < 100 {
      assert!(Instant::now() < deadline, "the requests never queued");
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
      metric(&app, "reseek_search_incoming_requests_dropped"),
      0.0,
      "the hundredth request waits"
    );
    assert_eq!(search(&edges, "alice", 100, &query).await, None);
    let metrics = &app.state.telemetry.metrics().search;
    assert_eq!(metrics.requests_dropped.get(), 1.0);
    assert_eq!(metrics.request_drop_rate.current(), 1);
    assert_eq!(metrics.requests_received.get(), 101.0);

    for request in &waiting {
      request.abort();
    }
    for request in waiting {
      assert!(request.await.unwrap_err().is_cancelled());
    }
    assert_eq!(edges.searches.waiting(), 0);
    assert_eq!(metrics.request_queue_depth.get(), 0.0);
    drop(worker);
  }

  #[tokio::test]
  async fn a_share_query_that_takes_too_long_drops_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path(), "").await;
    let edges = edges(&app);
    // the index lives in memory and is read through its writer, held here until released
    let (release, held) = std::sync::mpsc::channel::<()>();
    let index = app.state.shares.database();
    let holding = tokio::spawn(async move {
      index
        .write(move |_| {
          // a message or a dropped sender both end the hold
          let _ = held.recv();
          Ok(())
        })
        .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let query = SearchQuery::parse("abbey");
    let files = query_shares(&edges, &query, 10, Duration::from_millis(100)).await;
    assert_eq!(files, None);
    assert_eq!(
      app.state.telemetry.metrics().search.requests_dropped.get(),
      1.0
    );
    release.send(()).unwrap();
    holding.await.unwrap().unwrap();
  }
}
