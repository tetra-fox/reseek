//! Every metric the daemon keeps, registered once at startup with its name and help: incoming
//! searches, browses and enqueue requests, both directions of transfers, the distributed
//! network, and a relay controller's agents. The engines hold the handles and update them where
//! things happen; until then every value is zero. Only the relay's requests carry labels.

use std::sync::{Mutex, MutexGuard};

use reseek_shares::lock;

use super::derived::{Ema, TimedCounter};
use super::registry::{Counter, Gauge, Histogram, LabeledCounter, Registry, exponential};
use super::transfers::{TransferFamily, TransferMetrics};

/// The prefix of every metric of the daemon's own.
pub(super) const PREFIX: &str = "reseek";

/// Latencies of small steps, in milliseconds: 0.1 to 102.4.
fn small_latency() -> Vec<f64> {
  exponential(0.1, 2.0, 11)
}

/// Browse latencies, in milliseconds: 1 to 512.
fn browse_latency() -> Vec<f64> {
  exponential(1.0, 2.0, 10)
}

/// Enqueue latencies, in milliseconds: 1 to 32768.
fn enqueue_latency() -> Vec<f64> {
  exponential(1.0, 2.0, 16)
}

/// Other users' searches that reach us.
#[derive(Debug)]
pub struct SearchMetrics {
  /// every request received, before any filter
  pub requests_received: Counter,
  /// requests received in the last minute
  pub request_receive_rate: TimedCounter,
  /// requests dropped because too many were waiting
  pub requests_dropped: Counter,
  /// requests dropped in the last minute
  pub request_drop_rate: TimedCounter,
  /// requests waiting to be answered
  pub request_queue_depth: Gauge,
  /// milliseconds spent on the filters
  pub filter_latency: Histogram,
  /// the moving average of `filter_latency`
  pub filter_latency_current: Ema,
  /// milliseconds spent querying the shares
  pub query_latency: Histogram,
  /// the moving average of `query_latency`
  pub query_latency_current: Ema,
  /// milliseconds from receiving a request to answering it, or not
  pub response_latency: Histogram,
  /// the moving average of `response_latency`
  pub response_latency_current: Ema,
  /// responses with results sent
  pub responses_sent: Counter,
  /// responses sent in the last minute
  pub response_send_rate: TimedCounter,
  /// responses kept for searchers that could not be reached yet
  pub response_cache_size: Gauge,
}

/// Other users browsing our shares.
#[derive(Debug)]
pub struct BrowseMetrics {
  /// every request received
  pub requests_received: Counter,
  /// milliseconds taken to produce a response
  pub response_latency: Histogram,
  /// the moving average of `response_latency`
  pub response_latency_current: Ema,
  /// responses sent, the empty ones of blocked users not counted
  pub responses_sent: Counter,
}

/// Other users asking to queue a download from us.
#[derive(Debug)]
pub struct EnqueueMetrics {
  /// every request received
  pub requests_received: Counter,
  /// requests dropped because too many were waiting
  pub requests_dropped: Counter,
  /// requests waiting to be decided
  pub queue_depth: Gauge,
  /// milliseconds taken to decide a request against the limits
  pub decision_latency: Histogram,
  /// the moving average of `decision_latency`
  pub decision_latency_current: Ema,
  /// milliseconds taken to decide and queue an accepted request
  pub latency: Histogram,
  /// the moving average of `latency`
  pub latency_current: Ema,
  /// requests accepted
  pub requests_accepted: Counter,
  /// requests refused
  pub requests_rejected: Counter,
}

/// Our place in the distributed search network.
#[derive(Debug)]
pub struct DistributedMetrics {
  /// connected children
  pub children: Gauge,
  /// how many children we take
  pub child_limit: Gauge,
  /// 1 while a parent is connected, else 0
  pub has_parent: Gauge,
  /// our branch level
  pub branch_level: Gauge,
  /// the client's average broadcast latency, observed once a minute
  pub broadcast_latency: Histogram,
  /// the client's average broadcast latency as last read
  pub broadcast_latency_current: Gauge,
}

/// A relay controller's agents and what it asks of them.
#[derive(Debug)]
pub struct RelayMetrics {
  /// agents logged in
  pub agents_connected: Gauge,
  /// peers' downloads of agents' files whose bytes are flowing through the controller
  pub uploads_in_flight: Gauge,
  /// what the controller was asked by or asked of its agents, by `type` and `outcome`
  pub requests: LabeledCounter,
}

/// Every metric of the daemon, and the registry they are exposed from.
#[derive(Debug)]
pub struct Catalog {
  /// incoming searches
  pub search: SearchMetrics,
  /// incoming browses
  pub browse: BrowseMetrics,
  /// incoming enqueue requests
  pub enqueue: EnqueueMetrics,
  /// both directions of transfers, taken under their lock
  transfers: Mutex<TransferFamily>,
  /// the distributed network
  pub distributed: DistributedMetrics,
  /// a relay controller's agents
  pub relay: RelayMetrics,
  /// where they are all registered
  pub registry: Registry,
}

impl Default for Catalog {
  /// Every metric registered at zero.
  fn default() -> Self {
    let mut r = Registry::default();
    let name = |rest: &str| format!("{PREFIX}_{rest}");
    let search = SearchMetrics {
      requests_received: r.counter(
        name("search_incoming_requests_received"),
        "Total number of search requests received",
      ),
      request_receive_rate: TimedCounter::new(r.gauge(
        name("search_incoming_request_receive_rate_current"),
        "Number of search requests received in the last minute",
      )),
      requests_dropped: r.counter(
        name("search_incoming_requests_dropped"),
        "Total number of search requests dropped due to processing pressure",
      ),
      request_drop_rate: TimedCounter::new(r.gauge(
        name("search_incoming_request_drop_rate_current"),
        "Number of search requests dropped in the last minute",
      )),
      request_queue_depth: r.gauge(
        name("search_incoming_request_queue_depth_current"),
        "The number of incoming search requests waiting to be processed",
      ),
      filter_latency: r.histogram(
        name("search_incoming_filter_latency"),
        "The time taken to apply filters to an incoming search request, in milliseconds",
        small_latency(),
      ),
      filter_latency_current: Ema::new(r.gauge(
        name("search_incoming_filter_latency_current"),
        "The average time taken to apply filters to an incoming search request, in milliseconds",
      )),
      query_latency: r.histogram(
        name("search_incoming_query_latency"),
        "The time taken to query share database(s) for results, in milliseconds",
        small_latency(),
      ),
      query_latency_current: Ema::new(r.gauge(
        name("search_incoming_query_latency_current"),
        "The average time taken to query share database(s) for results, in milliseconds",
      )),
      response_latency: r.histogram(
        name("search_incoming_response_latency"),
        "The time taken to resolve and return a response to an incoming search request, in \
         milliseconds",
        small_latency(),
      ),
      response_latency_current: Ema::new(r.gauge(
        name("search_incoming_response_latency_current"),
        "The average time taken to resolve and return a response to an incoming search \
         request, in milliseconds",
      )),
      responses_sent: r.counter(
        name("search_incoming_responses_sent"),
        "Total number of search responses sent",
      ),
      response_send_rate: TimedCounter::new(r.gauge(
        name("search_incoming_response_send_rate_current"),
        "Number of search responses sent in the last minute",
      )),
      response_cache_size: r.gauge(
        name("search_response_cache_size"),
        "Number of search responses kept for searchers that could not be reached yet",
      ),
    };
    let browse = BrowseMetrics {
      requests_received: r.counter(
        name("browse_requests_received"),
        "Total number of browse requests received",
      ),
      response_latency: r.histogram(
        name("browse_response_latency"),
        "The time taken to resolve a response to an incoming browse request, in milliseconds",
        browse_latency(),
      ),
      response_latency_current: Ema::new(r.gauge(
        name("browse_response_latency_current"),
        "The average time taken to resolve a response to an incoming browse request, in \
         milliseconds",
      )),
      responses_sent: r.counter(
        name("browse_responses_sent"),
        "Total number of browse responses sent",
      ),
    };
    let enqueue = EnqueueMetrics {
      requests_received: r.counter(
        name("enqueue_requests_received"),
        "Total number of incoming enqueue requests received",
      ),
      requests_dropped: r.counter(
        name("enqueue_requests_dropped"),
        "Total number of incoming enqueue requests dropped due to processing pressure",
      ),
      queue_depth: r.gauge(
        name("enqueue_queue_depth_current"),
        "The number of incoming enqueue requests waiting to be processed",
      ),
      decision_latency: r.histogram(
        name("enqueue_decision_latency"),
        "The time taken to evaluate an incoming request to enqueue a file against configured \
         limits, in milliseconds",
        enqueue_latency(),
      ),
      decision_latency_current: Ema::new(r.gauge(
        name("enqueue_decision_latency_current"),
        "The average time taken to evaluate an incoming request to enqueue a file against \
         configured limits, in milliseconds",
      )),
      latency: r.histogram(
        name("enqueue_latency"),
        "The total time taken to resolve a response to an incoming request to enqueue a file, \
         in milliseconds",
        enqueue_latency(),
      ),
      latency_current: Ema::new(r.gauge(
        name("enqueue_latency_current"),
        "The average total time taken to resolve a response to an incoming request to enqueue \
         a file, in milliseconds",
      )),
      requests_accepted: r.counter(
        name("enqueue_requests_accepted"),
        "Total number of incoming enqueue requests accepted",
      ),
      requests_rejected: r.counter(
        name("enqueue_requests_rejected"),
        "Total number of incoming enqueue requests rejected",
      ),
    };
    let transfers = TransferFamily {
      uploads: TransferMetrics::register(&mut r, "uploads"),
      downloads: TransferMetrics::register(&mut r, "downloads"),
    };
    let distributed = DistributedMetrics {
      children: r.gauge(
        name("dnet_children"),
        "Current number of connected distributed children",
      ),
      child_limit: r.gauge(name("dnet_child_limit"), "Current distributed child limit"),
      has_parent: r.gauge(
        name("dnet_has_parent"),
        "A value indicating whether a distributed parent connection is established",
      ),
      branch_level: r.gauge(
        name("dnet_branch_level"),
        "Current distributed branch level",
      ),
      broadcast_latency: r.histogram(
        name("dnet_broadcast_latency"),
        "The time taken to broadcast incoming search requests to connected children, in \
         milliseconds",
        small_latency(),
      ),
      broadcast_latency_current: r.gauge(
        name("dnet_broadcast_latency_current"),
        "The average time taken to broadcast incoming search requests to connected children",
      ),
    };
    let relay = RelayMetrics {
      agents_connected: r.gauge(
        name("relay_agents_connected"),
        "Current number of relay agents logged in",
      ),
      uploads_in_flight: r.gauge(
        name("relay_uploads_in_flight"),
        "Current number of uploads whose file is coming from a relay agent",
      ),
      requests: r.labeled_counter(
        name("relay_requests_total"),
        "Total number of relay requests, by type and outcome",
      ),
    };
    Self {
      search,
      browse,
      enqueue,
      transfers: Mutex::new(transfers),
      distributed,
      relay,
      registry: r,
    }
  }
}

impl Catalog {
  /// The metrics of both directions of transfers, held until the guard is dropped, so the
  /// figures one update changes together are seen together.
  pub fn transfers(&self) -> MutexGuard<'_, TransferFamily> {
    lock(&self.transfers)
  }

  /// Ends the minute of every per-minute count, publishing each to its gauge.
  pub fn elapse_minute(&self) {
    self.search.request_receive_rate.elapse();
    self.search.request_drop_rate.elapse();
    self.search.response_send_rate.elapse();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::telemetry::registry::text;

  /// The names dashboards are built on, besides the transfer families.
  const FIXED: [&str; 36] = [
    "reseek_search_incoming_requests_received",
    "reseek_search_incoming_request_receive_rate_current",
    "reseek_search_incoming_requests_dropped",
    "reseek_search_incoming_request_drop_rate_current",
    "reseek_search_incoming_request_queue_depth_current",
    "reseek_search_incoming_filter_latency",
    "reseek_search_incoming_filter_latency_current",
    "reseek_search_incoming_query_latency",
    "reseek_search_incoming_query_latency_current",
    "reseek_search_incoming_response_latency",
    "reseek_search_incoming_response_latency_current",
    "reseek_search_incoming_responses_sent",
    "reseek_search_incoming_response_send_rate_current",
    "reseek_search_response_cache_size",
    "reseek_browse_requests_received",
    "reseek_browse_response_latency",
    "reseek_browse_response_latency_current",
    "reseek_browse_responses_sent",
    "reseek_enqueue_requests_received",
    "reseek_enqueue_requests_dropped",
    "reseek_enqueue_queue_depth_current",
    "reseek_enqueue_decision_latency",
    "reseek_enqueue_decision_latency_current",
    "reseek_enqueue_latency",
    "reseek_enqueue_latency_current",
    "reseek_enqueue_requests_accepted",
    "reseek_enqueue_requests_rejected",
    "reseek_dnet_children",
    "reseek_dnet_child_limit",
    "reseek_dnet_has_parent",
    "reseek_dnet_branch_level",
    "reseek_dnet_broadcast_latency",
    "reseek_dnet_broadcast_latency_current",
    "reseek_relay_agents_connected",
    "reseek_relay_uploads_in_flight",
    "reseek_relay_requests_total",
  ];

  /// The names of each transfer direction's families after `reseek_transfers_<direction>_`.
  const PER_DIRECTION: [&str; 13] = [
    "queued_users_current",
    "queued_files_current",
    "queued_bytes_current",
    "in_progress_users_current",
    "in_progress_files_current",
    "in_progress_bytes_current",
    "in_progress_speed_total_current",
    "in_progress_speed_average_current",
    "completed_succeeded",
    "completed_bytes",
    "completed_failed",
    "completed_speed_average",
    "completed_speed_average_current",
  ];

  #[test]
  fn the_catalog_registers_every_family_once_at_zero() {
    let catalog = Catalog::default();
    let families = catalog.registry.families();
    let mut names: Vec<_> = families.iter().map(|f| f.name.clone()).collect();
    names.sort_unstable();
    let mut expected: Vec<String> = FIXED.iter().map(ToString::to_string).collect();
    for dir in ["uploads", "downloads"] {
      expected.extend(
        PER_DIRECTION
          .iter()
          .map(|suffix| format!("reseek_transfers_{dir}_{suffix}")),
      );
    }
    expected.sort_unstable();
    assert_eq!(names, expected);
    let written = text(&families);
    assert!(written.contains("\nreseek_transfers_uploads_completed_failed 0\n"));
    assert!(written.contains(
      "# HELP reseek_transfers_downloads_queued_users_current Current number of unique users \
       with queued downloads\n"
    ));
    assert!(written.contains("reseek_enqueue_latency_bucket{le=\"32768\"} 0\n"));
    assert!(
      written
        .contains("reseek_transfers_uploads_completed_speed_average_bucket{le=\"1073741824\"} 0\n")
    );
  }
}
