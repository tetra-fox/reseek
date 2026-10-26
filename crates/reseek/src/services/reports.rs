//! Telemetry: the metrics registry and the reports over the transfer history.

use std::collections::BTreeMap;

use api_types::reports::{
  DirectionSummaries, DirectoriesQuery, DirectoryCount, ExceptionCount, ExceptionsQuery,
  HistogramQuery, LeaderboardEntry, LeaderboardQuery, ParetoQuery, SummaryQuery, SummaryReport,
  TransferExceptionDetail, UserReport, UserReportQuery,
};
use api_types::telemetry::PrometheusMetric;
use async_trait::async_trait;

use super::not_yet;
use crate::api::error::ApiError;
use crate::telemetry::reports::Width;

const DOMAIN: &str = "reports";

/// The metrics and the transfer reports. Every query's `start` and `end` are filled in and
/// checked by the API.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait ReportService: Send + Sync {
  /// Every metric family, by name.
  async fn metrics(&self) -> Result<BTreeMap<String, PrometheusMetric>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The metrics in the Prometheus text format.
  async fn metrics_text(&self) -> Result<String, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The metric families a dashboard shows, by name.
  async fn kpis(&self) -> Result<BTreeMap<String, PrometheusMetric>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Completed transfers by direction and outcome.
  async fn summary(&self, query: SummaryQuery) -> Result<SummaryReport, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Completed transfers by time bucket, direction and outcome, keyed by each bucket's start.
  async fn histogram(
    &self,
    query: HistogramQuery,
    width: Width,
  ) -> Result<BTreeMap<String, DirectionSummaries>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Users ranked by their successful transfers.
  async fn leaderboard(&self, query: LeaderboardQuery) -> Result<Vec<LeaderboardEntry>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Transfers that failed, with why.
  async fn exceptions(
    &self,
    query: ExceptionsQuery,
  ) -> Result<Vec<TransferExceptionDetail>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Failed transfers counted by why, most frequent first.
  async fn pareto(&self, query: ParetoQuery) -> Result<Vec<ExceptionCount>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The directories other users downloaded most from us.
  async fn directories(&self, query: DirectoriesQuery) -> Result<Vec<DirectoryCount>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One user's transfers by direction: outcomes, totals and failures.
  async fn user(&self, username: &str, query: UserReportQuery) -> Result<UserReport, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
