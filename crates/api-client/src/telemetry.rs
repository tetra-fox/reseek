//! The telemetry calls: the metrics and the transfer reports.

use std::collections::BTreeMap;

use api_types::reports::{
  DirectionSummaries, DirectoriesQuery, DirectoryCount, ExceptionCount, ExceptionsQuery,
  HistogramQuery, LeaderboardEntry, LeaderboardQuery, ParetoQuery, SummaryQuery, SummaryReport,
  TransferExceptionDetail, UserReport, UserReportQuery,
};
use api_types::telemetry::PrometheusMetric;

use super::{Client, Result, Route, json, text};

impl Client {
  /// `GET /telemetry/metrics` as JSON: every metric family by name.
  pub async fn metrics(&self) -> Result<BTreeMap<String, PrometheusMetric>> {
    json(
      self
        .get(Route::new("telemetry/metrics"))
        .header("accept", "application/json"),
    )
    .await
  }

  /// `GET /telemetry/metrics` as Prometheus text.
  pub async fn metrics_text(&self) -> Result<String> {
    text(
      self
        .get(Route::new("telemetry/metrics"))
        .header("accept", "text/plain"),
    )
    .await
  }

  /// `GET /telemetry/metrics/kpis`: the metric families a dashboard shows.
  pub async fn kpis(&self) -> Result<BTreeMap<String, PrometheusMetric>> {
    json(self.get(Route::new("telemetry/metrics/kpis"))).await
  }

  /// `GET /telemetry/reports/transfers/summary`.
  pub async fn summary_report(&self, query: &SummaryQuery) -> Result<SummaryReport> {
    json(self.get(reports("summary").query(query)?)).await
  }

  /// `GET /telemetry/reports/transfers/histogram`, keyed by each bucket's start.
  pub async fn histogram_report(
    &self,
    query: &HistogramQuery,
  ) -> Result<BTreeMap<String, DirectionSummaries>> {
    json(self.get(reports("histogram").query(query)?)).await
  }

  /// `GET /telemetry/reports/transfers/leaderboard`.
  pub async fn leaderboard_report(
    &self,
    query: &LeaderboardQuery,
  ) -> Result<Vec<LeaderboardEntry>> {
    json(self.get(reports("leaderboard").query(query)?)).await
  }

  /// `GET /telemetry/reports/transfers/exceptions`.
  pub async fn exceptions_report(
    &self,
    query: &ExceptionsQuery,
  ) -> Result<Vec<TransferExceptionDetail>> {
    json(self.get(reports("exceptions").query(query)?)).await
  }

  /// `GET /telemetry/reports/transfers/exceptions/pareto`.
  pub async fn pareto_report(&self, query: &ParetoQuery) -> Result<Vec<ExceptionCount>> {
    json(self.get(reports("exceptions/pareto").query(query)?)).await
  }

  /// `GET /telemetry/reports/transfers/directories`.
  pub async fn directories_report(&self, query: &DirectoriesQuery) -> Result<Vec<DirectoryCount>> {
    json(self.get(reports("directories").query(query)?)).await
  }

  /// `GET /telemetry/reports/transfers/users/{username}`.
  pub async fn user_report(&self, username: &str, query: &UserReportQuery) -> Result<UserReport> {
    json(self.get(reports("users").segment(username).query(query)?)).await
  }
}

/// The route of a transfer report.
fn reports(report: &str) -> Route {
  Route::new("telemetry/reports/transfers").then(report)
}
