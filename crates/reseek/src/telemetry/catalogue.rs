//! Every metric the daemon keeps, registered once at startup with its name and help: incoming
//! searches, browses and enqueue requests, both directions of transfers, and the distributed
//! network. The engines hold the handles and update them where things happen; until then every
//! value is zero. None of these carries labels.

use super::derived::{Ema, TimedCounter};
use super::registry::{Counter, Gauge, Histogram, Registry, exponential};

/// The prefix of every metric of the daemon's own.
const PREFIX: &str = "reseek";

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

/// Transfer speeds, in bytes per second: 1 KiB/s to 1 GiB/s.
fn speeds() -> Vec<f64> {
  vec![
    1024.0,
    10240.0,
    51200.0,
    102_400.0,
    204_800.0,
    307_200.0,
    409_600.0,
    512_000.0,
    1_048_576.0,
    2_097_152.0,
    4_194_304.0,
    6_291_456.0,
    10_485_760.0,
    26_214_400.0,
    52_428_800.0,
    104_857_600.0,
    262_144_000.0,
    524_288_000.0,
    1_073_741_824.0,
  ]
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

/// The transfers of one direction.
#[derive(Debug)]
pub struct TransferMetrics {
  /// users with queued transfers
  pub queued_users: Gauge,
  /// queued transfers
  pub queued_files: Gauge,
  /// the size of the queued transfers in bytes
  pub queued_bytes: Gauge,
  /// users with transfers in progress
  pub in_progress_users: Gauge,
  /// transfers in progress
  pub in_progress_files: Gauge,
  /// the size of the transfers in progress in bytes
  pub in_progress_bytes: Gauge,
  /// the moving average of the summed speed of the transfers in progress
  pub in_progress_speed_total: Ema,
  /// the moving average of the mean speed of the transfers in progress
  pub in_progress_speed_average: Ema,
  /// transfers that succeeded
  pub completed_succeeded: Counter,
  /// bytes moved by the transfers that succeeded
  pub completed_bytes: Counter,
  /// transfers that failed in any way
  pub completed_failed: Counter,
  /// the average speeds of the transfers that succeeded
  pub completed_speed_average: Histogram,
  /// the moving average of `completed_speed_average`
  pub completed_speed_average_current: Ema,
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

/// Every metric of the daemon, and the registry they are exposed from.
#[derive(Debug)]
pub struct Catalogue {
  /// incoming searches
  pub search: SearchMetrics,
  /// incoming browses
  pub browse: BrowseMetrics,
  /// incoming enqueue requests
  pub enqueue: EnqueueMetrics,
  /// uploads
  pub uploads: TransferMetrics,
  /// downloads
  pub downloads: TransferMetrics,
  /// the distributed network
  pub distributed: DistributedMetrics,
  /// where they are all registered
  pub registry: Registry,
}

impl Default for Catalogue {
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
    let uploads = transfers(&mut r, "uploads");
    let downloads = transfers(&mut r, "downloads");
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
    Self {
      search,
      browse,
      enqueue,
      uploads,
      downloads,
      distributed,
      registry: r,
    }
  }
}

impl Catalogue {
  /// Ends the minute of every per-minute count, publishing each to its gauge.
  pub fn elapse_minute(&self) {
    self.search.request_receive_rate.elapse();
    self.search.request_drop_rate.elapse();
    self.search.response_send_rate.elapse();
  }
}

/// The transfer metrics of `dir`, `uploads` or `downloads`.
fn transfers(r: &mut Registry, dir: &str) -> TransferMetrics {
  let name = |rest: &str| format!("{PREFIX}_transfers_{dir}_{rest}");
  TransferMetrics {
    queued_users: r.gauge(
      name("queued_users_current"),
      format!("Current number of unique users with queued {dir}"),
    ),
    queued_files: r.gauge(
      name("queued_files_current"),
      format!("Current number of queued {dir}"),
    ),
    queued_bytes: r.gauge(
      name("queued_bytes_current"),
      format!("Current total size, in bytes, of queued {dir}"),
    ),
    in_progress_users: r.gauge(
      name("in_progress_users_current"),
      format!("Current number of unique users with in-progress {dir}"),
    ),
    in_progress_files: r.gauge(
      name("in_progress_files_current"),
      format!("Current number of in-progress {dir}"),
    ),
    in_progress_bytes: r.gauge(
      name("in_progress_bytes_current"),
      format!("Current total size, in bytes, of in-progress {dir}"),
    ),
    in_progress_speed_total: Ema::new(r.gauge(
      name("in_progress_speed_total_current"),
      format!("Current total speed, in bytes per second, of in-progress {dir}"),
    )),
    in_progress_speed_average: Ema::new(r.gauge(
      name("in_progress_speed_average_current"),
      format!("Current average speed, in bytes per second, of in-progress {dir}"),
    )),
    completed_succeeded: r.counter(
      name("completed_succeeded"),
      format!("Total number of {dir} that completed successfully"),
    ),
    completed_bytes: r.counter(
      name("completed_bytes"),
      format!("Total number of bytes transferred by successfully completed {dir}"),
    ),
    completed_failed: r.counter(
      name("completed_failed"),
      format!("Total number of {dir} that failed"),
    ),
    completed_speed_average: r.histogram(
      name("completed_speed_average"),
      format!("The average speed of completed {dir}, in bytes per second"),
      speeds(),
    ),
    completed_speed_average_current: Ema::new(r.gauge(
      name("completed_speed_average_current"),
      format!("Current average speed, in bytes per second, of completed {dir}"),
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::telemetry::registry::text;

  #[test]
  fn the_catalogue_registers_every_family_once_at_zero() {
    let catalogue = Catalogue::default();
    let families = catalogue.registry.families();
    assert_eq!(families.len(), 13 + 4 + 9 + 2 * 13 + 6);
    let mut names: Vec<_> = families.iter().map(|f| f.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), families.len(), "no name twice");
    assert!(names.iter().all(|n| n.starts_with("reseek_")));
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
