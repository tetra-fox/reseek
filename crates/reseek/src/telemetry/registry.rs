//! The metric primitives and the registry that holds them: counters, gauges and histograms
//! with fixed buckets, written out in the Prometheus text format 0.0.4 or as the JSON object
//! the telemetry API returns.
//!
//! Names are exposed exactly as registered: a counter gets no `_total` added.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use api_types::telemetry::{MetricSample, MetricType, PrometheusMetric};

/// A value that only goes up. Cloning gives another handle to the same value.
#[derive(Clone, Debug, Default)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
  /// Adds one.
  pub fn inc(&self) {
    self.inc_by(1.0);
  }

  /// Adds `by`, which is not negative.
  pub fn inc_by(&self, by: f64) {
    add(&self.0, by);
  }

  /// The value.
  pub fn get(&self) -> f64 {
    f64::from_bits(self.0.load(Ordering::Relaxed))
  }
}

/// A value that is set. Cloning gives another handle to the same value.
#[derive(Clone, Debug, Default)]
pub struct Gauge(Arc<AtomicU64>);

impl Gauge {
  /// Sets the value.
  pub fn set(&self, value: f64) {
    self.0.store(value.to_bits(), Ordering::Relaxed);
  }

  /// Adds `by`, which may be negative.
  pub fn add(&self, by: f64) {
    add(&self.0, by);
  }

  /// The value.
  pub fn get(&self) -> f64 {
    f64::from_bits(self.0.load(Ordering::Relaxed))
  }
}

/// Adds `by` to the `f64` whose bits `cell` holds.
fn add(cell: &AtomicU64, by: f64) {
  cell.update(Ordering::Relaxed, Ordering::Relaxed, |bits| {
    (f64::from_bits(bits) + by).to_bits()
  });
}

/// Observations counted into buckets by their upper bound. Cloning gives another handle to the
/// same histogram.
#[derive(Clone, Debug)]
pub struct Histogram(Arc<Mutex<Observed>>);

#[derive(Debug)]
struct Observed {
  /// the upper bounds, ascending, without `+Inf`
  bounds: Vec<f64>,
  /// how many observations fell at or below each bound and above the one before it, with
  /// those above every bound last
  counts: Vec<u64>,
  sum: f64,
}

impl Histogram {
  /// A histogram with the upper bounds `bounds`, ascending; `+Inf` is added.
  pub fn new(bounds: Vec<f64>) -> Self {
    let counts = vec![0; bounds.len() + 1];
    Self(Arc::new(Mutex::new(Observed {
      bounds,
      counts,
      sum: 0.0,
    })))
  }

  /// Counts `value` into its bucket.
  pub fn observe(&self, value: f64) {
    let mut observed = self.lock();
    let slot = observed
      .bounds
      .iter()
      .position(|bound| value <= *bound)
      .unwrap_or(observed.bounds.len());
    observed.counts[slot] += 1;
    observed.sum += value;
  }

  /// The cumulative count at each bound, `+Inf` last as `f64::INFINITY`, with the sum and the
  /// count of every observation.
  fn snapshot(&self) -> (Vec<(f64, u64)>, f64, u64) {
    let observed = self.lock();
    let mut running = 0;
    let buckets = observed
      .bounds
      .iter()
      .copied()
      .chain([f64::INFINITY])
      .zip(&observed.counts)
      .map(|(bound, count)| {
        running += count;
        (bound, running)
      })
      .collect();
    (buckets, observed.sum, running)
  }

  fn lock(&self) -> std::sync::MutexGuard<'_, Observed> {
    // an observation is made whole before the lock is let go
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// `start`, then each bound `factor` times the one before, `count` bounds in all.
pub fn exponential(start: f64, factor: f64, count: u32) -> Vec<f64> {
  (0..count)
    .scan(start, |bound, _| {
      let this = *bound;
      *bound *= factor;
      Some(this)
    })
    .collect()
}

/// One metric family as it is exposed: from the registry, or read from the system when the
/// metrics are asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Family {
  /// the name
  pub name: String,
  /// what it measures
  pub help: String,
  /// its samples
  pub values: Values,
}

/// The samples of a family.
#[derive(Clone, Debug, PartialEq)]
pub enum Values {
  /// a counter's samples, each with its labels
  Counter(Vec<(Labels, f64)>),
  /// a gauge's samples, each with its labels
  Gauge(Vec<(Labels, f64)>),
  /// a histogram's cumulative buckets with `+Inf` last, its sum and its count
  Histogram {
    /// the upper bound and the cumulative count of each bucket
    buckets: Vec<(f64, u64)>,
    /// the sum of the observations
    sum: f64,
    /// how many observations
    count: u64,
  },
}

/// The labels of a sample, by name.
pub type Labels = BTreeMap<String, String>;

/// A registered metric.
#[derive(Clone, Debug)]
enum Metric {
  Counter(Counter),
  Gauge(Gauge),
  Histogram(Histogram),
}

/// The metrics the daemon keeps, in the order they were registered.
#[derive(Debug, Default)]
pub struct Registry {
  entries: Vec<(String, String, Metric)>,
}

impl Registry {
  /// Registers a counter.
  pub fn counter(&mut self, name: impl Into<String>, help: impl Into<String>) -> Counter {
    let counter = Counter::default();
    self
      .entries
      .push((name.into(), help.into(), Metric::Counter(counter.clone())));
    counter
  }

  /// Registers a gauge.
  pub fn gauge(&mut self, name: impl Into<String>, help: impl Into<String>) -> Gauge {
    let gauge = Gauge::default();
    self
      .entries
      .push((name.into(), help.into(), Metric::Gauge(gauge.clone())));
    gauge
  }

  /// Registers a histogram with the upper bounds `bounds`.
  pub fn histogram(
    &mut self,
    name: impl Into<String>,
    help: impl Into<String>,
    bounds: Vec<f64>,
  ) -> Histogram {
    let histogram = Histogram::new(bounds);
    self.entries.push((
      name.into(),
      help.into(),
      Metric::Histogram(histogram.clone()),
    ));
    histogram
  }

  /// Every registered family with its values now.
  pub fn families(&self) -> Vec<Family> {
    self
      .entries
      .iter()
      .map(|(name, help, metric)| Family {
        name: name.clone(),
        help: help.clone(),
        values: match metric {
          Metric::Counter(c) => Values::Counter(vec![(Labels::new(), c.get())]),
          Metric::Gauge(g) => Values::Gauge(vec![(Labels::new(), g.get())]),
          Metric::Histogram(h) => {
            let (buckets, sum, count) = h.snapshot();
            Values::Histogram {
              buckets,
              sum,
              count,
            }
          }
        },
      })
      .collect()
  }
}

/// `families` in the Prometheus text format 0.0.4.
pub fn text(families: &[Family]) -> String {
  let mut out = String::new();
  for family in families {
    let kind = match family.values {
      Values::Counter(_) => "counter",
      Values::Gauge(_) => "gauge",
      Values::Histogram { .. } => "histogram",
    };
    // writing to a String does not fail
    let _ = writeln!(out, "# HELP {} {}", family.name, escape_help(&family.help));
    let _ = writeln!(out, "# TYPE {} {kind}", family.name);
    match &family.values {
      Values::Counter(samples) | Values::Gauge(samples) => {
        for (labels, value) in samples {
          let _ = writeln!(
            out,
            "{}{} {}",
            family.name,
            label_text(labels),
            number(*value)
          );
        }
      }
      Values::Histogram {
        buckets,
        sum,
        count,
      } => {
        for (bound, cumulative) in buckets {
          let _ = writeln!(
            out,
            "{}_bucket{{le=\"{}\"}} {cumulative}",
            family.name,
            number(*bound)
          );
        }
        let _ = writeln!(out, "{}_sum {}", family.name, number(*sum));
        let _ = writeln!(out, "{}_count {count}", family.name);
      }
    }
  }
  out
}

/// `families` as the telemetry API's JSON object, keyed by name.
pub fn json(families: Vec<Family>) -> BTreeMap<String, PrometheusMetric> {
  families
    .into_iter()
    .map(|family| {
      let sample = |(labels, value): (Labels, f64)| MetricSample {
        value,
        labels: (!labels.is_empty()).then_some(labels),
      };
      let plain = |kind, samples: Vec<(Labels, f64)>| PrometheusMetric {
        name: family.name.clone(),
        help: family.help.clone(),
        kind,
        samples: Some(samples.into_iter().map(sample).collect()),
        sum: None,
        count: None,
        buckets: None,
        quantiles: None,
      };
      let metric = match family.values {
        Values::Counter(samples) => plain(MetricType::Counter, samples),
        Values::Gauge(samples) => plain(MetricType::Gauge, samples),
        Values::Histogram {
          buckets,
          sum,
          count,
        } => PrometheusMetric {
          name: family.name.clone(),
          help: family.help,
          kind: MetricType::Histogram,
          samples: None,
          sum: Some(sum),
          count: Some(count as f64),
          buckets: Some(
            buckets
              .into_iter()
              .map(|(bound, cumulative)| {
                let le = number(bound);
                let labels = Labels::from([("le".to_string(), le.clone())]);
                (
                  le,
                  MetricSample {
                    value: cumulative as f64,
                    labels: Some(labels),
                  },
                )
              })
              .collect(),
          ),
          quantiles: None,
        },
      };
      (family.name, metric)
    })
    .collect()
}

/// A number as the text format writes it: `+Inf`, `-Inf`, `NaN`, else the shortest form that
/// reads back.
fn number(value: f64) -> String {
  if value == f64::INFINITY {
    "+Inf".to_string()
  } else if value == f64::NEG_INFINITY {
    "-Inf".to_string()
  } else if value.is_nan() {
    "NaN".to_string()
  } else {
    value.to_string()
  }
}

/// `{name="value",...}`, or nothing without labels.
fn label_text(labels: &Labels) -> String {
  if labels.is_empty() {
    return String::new();
  }
  let pairs: Vec<String> = labels
    .iter()
    .map(|(name, value)| {
      let value = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
      format!("{name}=\"{value}\"")
    })
    .collect();
  format!("{{{}}}", pairs.join(","))
}

/// Help text with its backslashes and line breaks escaped.
fn escape_help(help: &str) -> String {
  help.replace('\\', "\\\\").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn histograms_count_cumulatively_and_include_their_bounds() {
    let mut registry = Registry::default();
    let latency = registry.histogram("lat", "how long", exponential(0.1, 2.0, 3));
    for value in [0.1, 0.15, 0.4, 7.0] {
      latency.observe(value);
    }
    let families = registry.families();
    assert_eq!(
      text(&families),
      "# HELP lat how long\n# TYPE lat histogram\nlat_bucket{le=\"0.1\"} 1\n\
       lat_bucket{le=\"0.2\"} 2\nlat_bucket{le=\"0.4\"} 3\nlat_bucket{le=\"+Inf\"} 4\n\
       lat_sum 7.65\nlat_count 4\n"
    );
    let json = json(families);
    let buckets = json["lat"].buckets.as_ref().unwrap();
    assert_eq!(buckets["+Inf"].value, 4.0);
    assert_eq!(buckets["0.2"].labels.as_ref().unwrap()["le"], "0.2");
    assert_eq!(json["lat"].count, Some(4.0));
    assert_eq!(json["lat"].samples, None);
  }

  #[test]
  fn counters_keep_their_name_and_labels_are_escaped() {
    let mut registry = Registry::default();
    registry.counter("reseek_sent", "sent").inc_by(2.0);
    let mut families = registry.families();
    families.push(Family {
      name: "node_fs".into(),
      help: "a\nb".into(),
      values: Values::Gauge(vec![(
        Labels::from([("mountpoint".to_string(), "/a \"b\"".to_string())]),
        1.5,
      )]),
    });
    let written = text(&families);
    assert!(
      written.contains("# TYPE reseek_sent counter\nreseek_sent 2\n"),
      "{written}"
    );
    assert!(written.contains("# HELP node_fs a\\nb\n"));
    assert!(written.contains("node_fs{mountpoint=\"/a \\\"b\\\"\"} 1.5\n"));
    let json = json(families);
    assert_eq!(json["reseek_sent"].kind, MetricType::Counter);
    assert_eq!(
      json["reseek_sent"].samples.as_ref().unwrap()[0].labels,
      None
    );
    assert_eq!(json["node_fs"].kind, MetricType::Gauge);
  }
}
