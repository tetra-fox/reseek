//! Telemetry: the live transfer figures of the `metrics.update` frame and the JSON form of the
//! metrics registry.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Live transfer figures for both directions, as `metrics.update` frames carry them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferMetrics {
  /// downloads
  pub downloads: DirectionMetrics,
  /// uploads
  pub uploads: DirectionMetrics,
}

/// Live transfer figures for one direction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectionMetrics {
  /// transfers that are moving bytes
  pub in_progress: InProgressMetrics,
  /// transfers waiting in a queue, here or at the peer
  pub queued: QueuedMetrics,
  /// transfers that finished since the daemon started
  pub completed: CompletedMetrics,
}

/// Transfers that are moving bytes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InProgressMetrics {
  /// how many
  pub files: u64,
  /// how many distinct users they are with
  pub users: u64,
  /// the moving average of their mean speed, in bytes per second
  pub average_speed: f64,
  /// the moving average of their summed speed, in bytes per second
  pub total_speed: f64,
}

/// Transfers waiting in a queue.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMetrics {
  /// how many
  pub files: u64,
  /// how many distinct users they are with
  pub users: u64,
  /// their summed size in bytes
  pub bytes: u64,
}

/// Transfers that finished since the daemon started.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompletedMetrics {
  /// how many succeeded
  pub succeeded: u64,
  /// how many failed, cancelled ones included
  pub failed: u64,
  /// the bytes moved by the ones that succeeded
  pub bytes: u64,
}

/// What kind of metric a family is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
  /// a value that only goes up
  Counter,
  /// a value that is set
  Gauge,
  /// observations counted into cumulative buckets
  Histogram,
  /// observations summarised as quantiles
  Summary,
  /// a value without a declared kind
  Untyped,
}

/// One metric family of the registry, an entry of the object `GET /telemetry/metrics` returns
/// when JSON is asked for, keyed by the family's name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrometheusMetric {
  /// the family's name
  #[schema(example = "reseek_search_incoming_response_latency")]
  pub name: String,
  /// what the family measures
  pub help: String,
  /// what kind of metric it is
  #[serde(rename = "type")]
  pub kind: MetricType,
  /// one entry per sample of a counter, gauge or untyped family; null for histograms and
  /// summaries
  pub samples: Option<Vec<MetricSample>>,
  /// the sum of the observations of a histogram or summary, null for the other kinds
  pub sum: Option<f64>,
  /// how many observations a histogram or summary has, null for the other kinds
  pub count: Option<f64>,
  /// a histogram's cumulative buckets keyed by their upper bound such as `"0.1"` or `"+Inf"`,
  /// null for the other kinds
  pub buckets: Option<BTreeMap<String, MetricSample>>,
  /// a summary's quantiles keyed by the quantile such as `"0.5"`, null for the other kinds
  pub quantiles: Option<BTreeMap<String, MetricSample>>,
}

/// One sample of a metric family.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricSample {
  /// the value
  pub value: f64,
  /// the sample's labels, null when it has none
  pub labels: Option<BTreeMap<String, String>>,
}
