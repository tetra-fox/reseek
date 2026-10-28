//! Telemetry: the metrics the daemon keeps and reads from the system, served as Prometheus
//! text or JSON, and the reports over the transfer history.

pub mod catalogue;
pub mod derived;
pub mod exposition;
pub mod registry;
pub mod reports;
mod system;
pub mod transfers;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_types::reports::{
  DirectionSummaries, DirectoriesQuery, DirectoryCount, ExceptionCount, ExceptionsQuery,
  HistogramQuery, LeaderboardEntry, LeaderboardQuery, ParetoQuery, SummaryQuery, SummaryReport,
  TransferExceptionDetail, UserReport, UserReportQuery,
};
use api_types::telemetry::PrometheusMetric;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info_span};

use crate::api::error::ApiError;
use crate::db::Database;
use crate::services::ReportService;
use catalogue::Catalogue;
use registry::Family;
use reports::{HistogramError, Reports, Width, Window};
use system::SystemProbe;

/// How long a per-minute count runs before it is published.
const MINUTE: Duration = Duration::from_secs(60);

/// The metrics and the reports.
pub struct Telemetry {
  catalogue: Catalogue,
  system: Arc<SystemProbe>,
  reports: Reports,
}

impl Telemetry {
  /// Every metric registered at zero, the reports over `transfers`, and the clock that ends
  /// each minute of the per-minute counts until `shutdown`.
  pub fn new(transfers: Database, shutdown: CancellationToken) -> Arc<Self> {
    let telemetry = Arc::new(Self {
      catalogue: Catalogue::default(),
      system: Arc::new(SystemProbe::default()),
      reports: Reports::new(transfers),
    });
    let clock = telemetry.clone();
    tokio::spawn(
      async move {
        let mut every = tokio::time::interval_at(tokio::time::Instant::now() + MINUTE, MINUTE);
        loop {
          tokio::select! {
            () = shutdown.cancelled() => return,
            _ = every.tick() => clock.catalogue.elapse_minute(),
          }
        }
      }
      .instrument(info_span!("metrics_clock")),
    );
    telemetry
  }

  /// The metrics the engines update.
  pub fn metrics(&self) -> &Catalogue {
    &self.catalogue
  }

  /// The memory this process holds in RAM, in bytes, when the system says.
  pub async fn resident_memory(&self) -> Option<u64> {
    let system = self.system.clone();
    match tokio::task::spawn_blocking(move || system.resident_memory()).await {
      Ok(memory) => memory,
      Err(e) => {
        error!(error = %e, "the process's memory not read");
        None
      }
    }
  }

  /// Every family: the daemon's own, then those read from the system.
  async fn families(&self) -> Result<Vec<Family>, ApiError> {
    let mut families = self.catalogue.registry.families();
    let system = self.system.clone();
    let read = tokio::task::spawn_blocking(move || system.families())
      .await
      .map_err(|e| {
        error!(error = %e, "system metrics not read");
        ApiError::internal(e).logged()
      })?;
    families.extend(read);
    Ok(families)
  }
}

/// The time since `started`, in milliseconds, the unit latencies are kept in.
pub fn millis(started: std::time::Instant) -> f64 {
  started.elapsed().as_secs_f64() * 1000.0
}

/// Whether a family is one a dashboard shows: the daemon's own, the process's, and the host's
/// except its processor times.
fn is_kpi(name: &str) -> bool {
  name.starts_with("reseek_")
    || name.starts_with("process_")
    || (name.starts_with("node_") && !name.starts_with("node_cpu"))
}

/// The window the API filled into a query.
fn window(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> Result<Window, ApiError> {
  match (start, end) {
    (Some(start), Some(end)) => Ok(Window { start, end }),
    _ => Err(ApiError::internal(
      "a report was asked for with its window left open",
    )),
  }
}

/// A failed report as the API answers it, logged with the report and its window.
fn failed(
  report: &str,
  start: Option<DateTime<Utc>>,
  end: Option<DateTime<Utc>>,
  e: impl std::fmt::Display,
) -> ApiError {
  error!(
    report,
    start = ?start,
    end = ?end,
    error = %e,
    "Error fetching report"
  );
  ApiError::internal(e).logged()
}

#[async_trait]
impl ReportService for Telemetry {
  async fn metrics(&self) -> Result<BTreeMap<String, PrometheusMetric>, ApiError> {
    Ok(registry::json(self.families().await?))
  }

  async fn metrics_text(&self) -> Result<String, ApiError> {
    Ok(registry::text(&self.families().await?))
  }

  async fn kpis(&self) -> Result<BTreeMap<String, PrometheusMetric>, ApiError> {
    let families = self.families().await?;
    Ok(registry::json(
      families.into_iter().filter(|f| is_kpi(&f.name)).collect(),
    ))
  }

  async fn summary(&self, query: SummaryQuery) -> Result<SummaryReport, ApiError> {
    let (start, end) = (query.start, query.end);
    self
      .reports
      .summary(window(start, end)?, query)
      .await
      .map_err(|e| failed("summary", start, end, e))
  }

  async fn histogram(
    &self,
    query: HistogramQuery,
    width: Width,
  ) -> Result<BTreeMap<String, DirectionSummaries>, ApiError> {
    let (start, end) = (query.start, query.end);
    match self
      .reports
      .histogram(window(start, end)?, width, query)
      .await
    {
      Ok(histogram) => Ok(histogram),
      Err(e @ HistogramError::TooManyBuckets(_)) => Err(ApiError::bad_request(e.to_string())),
      Err(HistogramError::Db(e)) => Err(failed("histogram", start, end, e)),
    }
  }

  async fn leaderboard(&self, query: LeaderboardQuery) -> Result<Vec<LeaderboardEntry>, ApiError> {
    let (start, end) = (query.start, query.end);
    self
      .reports
      .leaderboard(window(start, end)?, query)
      .await
      .map_err(|e| failed("leaderboard", start, end, e))
  }

  async fn exceptions(
    &self,
    query: ExceptionsQuery,
  ) -> Result<Vec<TransferExceptionDetail>, ApiError> {
    let (start, end) = (query.start, query.end);
    self
      .reports
      .exceptions(window(start, end)?, query)
      .await
      .map_err(|e| failed("exceptions", start, end, e))
  }

  async fn pareto(&self, query: ParetoQuery) -> Result<Vec<ExceptionCount>, ApiError> {
    let (start, end) = (query.start, query.end);
    self
      .reports
      .pareto(window(start, end)?, query)
      .await
      .map_err(|e| failed("exceptions pareto", start, end, e))
  }

  async fn directories(&self, query: DirectoriesQuery) -> Result<Vec<DirectoryCount>, ApiError> {
    let (start, end) = (query.start, query.end);
    self
      .reports
      .directories(window(start, end)?, query)
      .await
      .map_err(|e| failed("directories", start, end, e))
  }

  async fn user(&self, username: &str, query: UserReportQuery) -> Result<UserReport, ApiError> {
    let (start, end) = (query.start, query.end);
    self
      .reports
      .user(username, window(start, end)?)
      .await
      .map_err(|e| failed("user", start, end, e))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_figures_are_ours_the_processs_and_the_hosts_but_its_processor_times() {
    assert!(is_kpi("reseek_browse_requests_received"));
    assert!(is_kpi("process_resident_memory_bytes"));
    assert!(is_kpi("node_filesystem_avail_bytes"));
    assert!(!is_kpi("node_cpu_seconds_total"));
    assert!(!is_kpi("tokio_workers"));
  }

  #[tokio::test]
  async fn the_metrics_hold_the_catalogue_and_the_process() {
    let telemetry = Telemetry::new(
      crate::transfers::fixtures::transfers().await,
      CancellationToken::new(),
    );
    telemetry.metrics().browse.requests_received.inc();
    let json = ReportService::metrics(&*telemetry).await.unwrap();
    assert_eq!(
      json["reseek_browse_requests_received"]
        .samples
        .as_ref()
        .unwrap()[0]
        .value,
      1.0
    );
    assert!(json.contains_key("tokio_workers"));
    let kpis = telemetry.kpis().await.unwrap();
    assert!(kpis.contains_key("reseek_dnet_children"));
    assert!(!kpis.contains_key("tokio_workers"));
    if cfg!(target_os = "linux") {
      assert!(
        json["process_resident_memory_bytes"]
          .samples
          .as_ref()
          .unwrap()[0]
          .value
          > 0.0
      );
    }
  }
}
