//! The telemetry routes: the metrics registry as JSON or Prometheus text, and the reports over
//! the transfer history the dashboard draws. Every route takes the ReadOnly role.
//!
//! A report counts transfers by when they ended. The API fills in `start` and `end` where they
//! are left out, as each route says, and refuses a window that ends before it starts.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use api_types::reports::{
  DirectionSummaries, DirectoriesQuery, DirectoryCount, ExceptionCount, ExceptionsQuery,
  HistogramQuery, LeaderboardEntry, LeaderboardQuery, ParetoQuery, SummaryQuery, SummaryReport,
  TransferExceptionDetail, UserReport, UserReportQuery,
};
use api_types::telemetry::PrometheusMetric;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Days, Utc};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized};
use crate::db;
use crate::telemetry::reports::{GENESIS, Width, genesis};

/// The most buckets a histogram has.
const BUCKETS_MAX: u32 = 1000;

/// The buckets a histogram has when neither `buckets` nor `interval` is given.
const BUCKETS_DEFAULT: NonZeroU32 = NonZeroU32::new(100).expect("100 is not zero");

/// The telemetry routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(metrics))
    .routes(routes!(kpis))
    .routes(routes!(summary))
    .routes(routes!(histogram))
    .routes(routes!(leaderboard))
    .routes(routes!(exceptions))
    .routes(routes!(pareto))
    .routes(routes!(directories))
    .routes(routes!(user))
}

fn week_ago() -> DateTime<Utc> {
  Utc::now() - Days::new(7)
}

/// Refuses a window that does not end after it starts.
fn window(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> Result<(), ApiError> {
  match (start, end) {
    (Some(start), Some(end)) if start >= end => Err(ApiError::bad_request(
      "End time must be later than start time",
    )),
    _ => Ok(()),
  }
}

/// Whether a request's `Accept` prefers JSON to text: JSON is acceptable and weighed above
/// `text/plain`.
fn prefers_json(headers: &HeaderMap) -> bool {
  let Some(accept) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) else {
    return false;
  };
  let weight = |kind: &str, family: &str| {
    let mut best: Option<(u8, f32)> = None;
    for range in accept.split(',') {
      let mut parts = range.split(';');
      let media = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
      let q = parts
        .filter_map(|p| p.trim().strip_prefix("q="))
        .find_map(|q| q.trim().parse::<f32>().ok())
        .unwrap_or(1.0);
      let specificity = if media == kind {
        2
      } else if media == format!("{family}/*") {
        1
      } else if media == "*/*" {
        0
      } else {
        continue;
      };
      if best.is_none_or(|(s, _)| specificity > s) {
        best = Some((specificity, q));
      }
    }
    best.map_or(0.0, |(_, q)| q)
  };
  let json = weight("application/json", "application");
  json > 0.0 && json > weight("text/plain", "text")
}

/// Every metric, as JSON keyed by family name when `Accept` prefers JSON, else as Prometheus
/// text.
#[utoipa::path(
  get, path = "/telemetry/metrics", tag = "telemetry",
  responses(
    (status = 200, content(
      (BTreeMap<String, PrometheusMetric> = "application/json"), (String = "text/plain")
    )),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn metrics(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  if prefers_json(&headers) {
    return app
      .services
      .reports
      .metrics()
      .await
      .map(|families| Json(families).into_response());
  }
  let text = app.services.reports.metrics_text().await?;
  Ok(
    (
      [(header::CONTENT_TYPE, crate::telemetry::exposition::TEXT)],
      text,
    )
      .into_response(),
  )
}

/// The metric families a dashboard shows, keyed by name.
#[utoipa::path(
  get, path = "/telemetry/metrics/kpis", tag = "telemetry",
  responses(
    (status = 200, body = BTreeMap<String, PrometheusMetric>),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn kpis(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<BTreeMap<String, PrometheusMetric>>, ApiError> {
  app.services.reports.kpis().await.map(Json)
}

/// Completed transfers by direction and outcome. The window is the last 7 days unless given.
#[utoipa::path(
  operation_id = "transfer_summary_report",
  get, path = "/telemetry/reports/transfers/summary", tag = "telemetry", params(SummaryQuery),
  responses(
    (status = 200, body = SummaryReport), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn summary(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(mut query): Query<SummaryQuery>,
) -> Result<Json<SummaryReport>, ApiError> {
  query.start.get_or_insert_with(week_ago);
  query.end.get_or_insert_with(Utc::now);
  window(query.start, query.end)?;
  app.services.reports.summary(query).await.map(Json)
}

/// Completed transfers by time bucket, direction and outcome, every bucket listed and keyed by
/// its start, which is a multiple of the bucket's length since the Unix epoch. The window is
/// the last 7 days unless given, in 100 buckets unless `buckets` or `interval` (minutes) says
/// otherwise. A bucket's `averageSpeed` is its bytes over its transfers' time.
#[utoipa::path(
  get, path = "/telemetry/reports/transfers/histogram", tag = "telemetry", params(HistogramQuery),
  responses(
    (status = 200, body = BTreeMap<String, DirectionSummaries>),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn histogram(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(mut query): Query<HistogramQuery>,
) -> Result<Json<BTreeMap<String, DirectionSummaries>>, ApiError> {
  query.start.get_or_insert_with(week_ago);
  let end = *query.end.get_or_insert_with(Utc::now);
  window(query.start, query.end)?;
  if end < genesis() {
    return Err(ApiError::bad_request(format!(
      "End time impossibly early (earlier than {GENESIS})"
    )));
  }
  let width = match (query.buckets, query.interval) {
    (Some(_), Some(_)) => {
      return Err(ApiError::bad_request(
        "Specify either interval or buckets, not both",
      ));
    }
    (Some(buckets), None) => NonZeroU32::new(buckets)
      .filter(|b| b.get() <= BUCKETS_MAX)
      .map(Width::Buckets)
      .ok_or_else(|| ApiError::bad_request("Buckets must be between 1 and 1000, if specified"))?,
    (None, Some(minutes)) => NonZeroU32::new(minutes)
      .map(Width::Minutes)
      .ok_or_else(|| ApiError::bad_request("Interval must be at least 1"))?,
    (None, None) => Width::Buckets(BUCKETS_DEFAULT),
  };
  app.services.reports.histogram(query, width).await.map(Json)
}

/// Users ranked by their successful transfers in one direction. The window is everything
/// unless given.
#[utoipa::path(
  get, path = "/telemetry/reports/transfers/leaderboard", tag = "telemetry", params(LeaderboardQuery),
  responses(
    (status = 200, body = Vec<LeaderboardEntry>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn leaderboard(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(mut query): Query<LeaderboardQuery>,
) -> Result<Json<Vec<LeaderboardEntry>>, ApiError> {
  query.start.get_or_insert_with(genesis);
  query.end.get_or_insert_with(Utc::now);
  window(query.start, query.end)?;
  validate::positive("limit", query.limit)?;
  app.services.reports.leaderboard(query).await.map(Json)
}

/// Transfers in one direction that failed, ordered by when they ended. The window is
/// unbounded unless given.
#[utoipa::path(
  get, path = "/telemetry/reports/transfers/exceptions", tag = "telemetry", params(ExceptionsQuery),
  responses(
    (status = 200, body = Vec<TransferExceptionDetail>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn exceptions(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(mut query): Query<ExceptionsQuery>,
) -> Result<Json<Vec<TransferExceptionDetail>>, ApiError> {
  query.start.get_or_insert_with(db::earliest);
  query.end.get_or_insert_with(db::latest);
  window(query.start, query.end)?;
  validate::positive("limit", query.limit)?;
  app.services.reports.exceptions(query).await.map(Json)
}

/// Failed transfers in one direction counted by why they failed, most frequent first. The
/// window is unbounded unless given.
#[utoipa::path(
  get, path = "/telemetry/reports/transfers/exceptions/pareto", tag = "telemetry", params(ParetoQuery),
  responses(
    (status = 200, body = Vec<ExceptionCount>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn pareto(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(mut query): Query<ParetoQuery>,
) -> Result<Json<Vec<ExceptionCount>>, ApiError> {
  query.start.get_or_insert_with(db::earliest);
  query.end.get_or_insert_with(db::latest);
  window(query.start, query.end)?;
  validate::positive("limit", query.limit)?;
  app.services.reports.pareto(query).await.map(Json)
}

/// The directories other users downloaded most from us, by how many users did. The window is
/// everything unless given.
#[utoipa::path(
  get, path = "/telemetry/reports/transfers/directories", tag = "telemetry", params(DirectoriesQuery),
  responses(
    (status = 200, body = Vec<DirectoryCount>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn directories(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(mut query): Query<DirectoriesQuery>,
) -> Result<Json<Vec<DirectoryCount>>, ApiError> {
  query.start.get_or_insert_with(genesis);
  query.end.get_or_insert_with(db::latest);
  window(query.start, query.end)?;
  validate::positive("limit", query.limit)?;
  app.services.reports.directories(query).await.map(Json)
}

/// One user's transfers by direction: outcomes, totals and why they failed. The window is
/// everything unless given.
#[utoipa::path(
  get, path = "/telemetry/reports/transfers/users/{username}", tag = "telemetry",
  params(("username" = String, Path, description = "the user"), UserReportQuery),
  responses(
    (status = 200, body = UserReport), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn user(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(username): Path<String>,
  Query(mut query): Query<UserReportQuery>,
) -> Result<Json<UserReport>, ApiError> {
  if username.trim().is_empty() {
    return Err(ApiError::bad_request("Username is required"));
  }
  query.start.get_or_insert_with(genesis);
  query.end.get_or_insert_with(Utc::now);
  window(query.start, query.end)?;
  app.services.reports.user(&username, query).await.map(Json)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{app, request};

  #[tokio::test]
  async fn report_queries_are_checked_before_the_reports_run() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    )
    .await;
    let reports = "/api/v0/telemetry/reports/transfers";
    let at = "2026-01-02T00:00:00Z";
    let cases = [
      (format!("{reports}/summary?start={at}&end={at}"), 400),
      (
        format!("{reports}/summary?start=2026-01-01T00:00:00&end=2026-01-02T00:00:00"),
        200,
      ),
      (format!("{reports}/summary?start=2026-01-01"), 400),
      (
        format!("{reports}/summary?start=2026-01-01T00:00:00Z&end={at}"),
        200,
      ),
      (format!("{reports}/histogram?interval=0"), 400),
      (format!("{reports}/histogram?buckets=1001"), 400),
      (format!("{reports}/histogram?buckets=1000"), 200),
      (format!("{reports}/histogram?buckets=5&interval=60"), 400),
      (
        format!("{reports}/histogram?end=2020-12-30T06:21:59Z&start=2020-01-01T00:00:00Z"),
        400,
      ),
      (
        format!("{reports}/leaderboard?direction=Upload&limit=0"),
        400,
      ),
      (
        format!("{reports}/exceptions?direction=Upload&limit=0"),
        400,
      ),
      (
        format!("{reports}/exceptions/pareto?direction=Upload&limit=0"),
        400,
      ),
      (format!("{reports}/directories?limit=0"), 400),
      (format!("{reports}/users/%20"), 400),
      (format!("{reports}/users/alice"), 200),
    ];
    for (uri, status) in cases {
      assert_eq!(
        app.send(request("GET", &uri, None)).await.status,
        status,
        "{uri}"
      );
    }
    let unbounded = app
      .send(request("GET", &format!("{reports}/histogram"), None))
      .await;
    assert_eq!(unbounded.status, 200);
    let buckets = unbounded.json().as_object().unwrap().len();
    assert!(
      (100..=101).contains(&buckets),
      "100 buckets when neither is given: {buckets}"
    );
  }

  #[test]
  fn json_is_chosen_only_when_weighed_above_text() {
    let accept = |value: &'static str| {
      let mut headers = HeaderMap::new();
      headers.insert(header::ACCEPT, value.parse().unwrap());
      headers
    };
    assert!(prefers_json(&accept("application/json")));
    assert!(prefers_json(&accept("text/plain;q=0.5, application/json")));
    assert!(prefers_json(&accept("application/*, text/plain;q=0.9")));
    assert!(
      !prefers_json(&accept("application/json, text/plain")),
      "a tie stays text"
    );
    assert!(!prefers_json(&accept("*/*")));
    assert!(!prefers_json(&accept("application/json;q=0")));
    assert!(!prefers_json(&HeaderMap::new()));
  }
}
