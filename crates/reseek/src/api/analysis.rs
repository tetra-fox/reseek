//! The analysis routes: queueing a downloaded file for analysis, reading its report and its
//! spectrogram, recording the user's own verdict, and listing reports and the queue. A file is
//! named as the files routes name it, or as a succeeded download.

use api_types::analysis::{
  AnalysisJob, AnalysisJobCreated, AnalysisListQuery, AnalysisReportQuery, AudioAnalysis,
  RunAnalysisQuery, SetAnalysisOverride, SpectrogramQuery,
};
use api_types::files::FileRoot;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::files::root;
use super::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};
use crate::services::FileTarget;

/// The analysis routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(run_file, report_file))
    .routes(routes!(spectrogram_file))
    .routes(routes!(set_override, clear_override))
    .routes(routes!(run_download, report_download))
    .routes(routes!(spectrogram_download))
    .routes(routes!(list))
    .routes(routes!(jobs))
}

/// The file under a root that a path names.
fn file(name: &str, path: &str) -> Result<FileTarget, ApiError> {
  Ok(FileTarget::File {
    root: root(name)?,
    path: validate::files_path(path, false)?,
  })
}

/// Queues a file for analysis, again with `force` when it has a report.
#[utoipa::path(
  post, path = "/files/{root}/files/{path}/analysis", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    RunAnalysisQuery
  ),
  responses(
    (status = 202, body = AnalysisJobCreated), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn run_file(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<RunAnalysisQuery>,
) -> Result<Response, ApiError> {
  let target = file(&name, &path)?;
  queued(&app, &caller.principal.subject, target, query.force).await
}

/// Queues a succeeded download's file for analysis, again with `force` when it has a report.
#[utoipa::path(
  post, path = "/transfers/downloads/{username}/{id}/analysis", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    RunAnalysisQuery
  ),
  responses(
    (status = 202, body = AnalysisJobCreated), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn run_download(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<RunAnalysisQuery>,
) -> Result<Response, ApiError> {
  let target = FileTarget::Download { username, id };
  queued(&app, &caller.principal.subject, target, query.force).await
}

async fn queued(
  app: &AppState,
  by: &str,
  target: FileTarget,
  force: bool,
) -> Result<Response, ApiError> {
  let job = app.services.analysis.run(target.clone(), force).await?;
  info!(by, ?target, job = %job.job_id, "analysis queued");
  Ok((StatusCode::ACCEPTED, Json(job)).into_response())
}

/// A file's analysis report; without `windows` the per-window measures are left out.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/analysis", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    AnalysisReportQuery
  ),
  responses(
    (status = 200, body = AudioAnalysis), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn report_file(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<AnalysisReportQuery>,
) -> Result<Json<AudioAnalysis>, ApiError> {
  let target = file(&name, &path)?;
  app
    .services
    .analysis
    .report(target, query.windows)
    .await
    .map(Json)
}

/// A succeeded download's analysis report.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/analysis", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    AnalysisReportQuery
  ),
  responses(
    (status = 200, body = AudioAnalysis), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn report_download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<AnalysisReportQuery>,
) -> Result<Json<AudioAnalysis>, ApiError> {
  let target = FileTarget::Download { username, id };
  app
    .services
    .analysis
    .report(target, query.windows)
    .await
    .map(Json)
}

/// A file's spectrogram as a PNG image. A link token does not load it; the session does.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/analysis/spectrogram.png", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    SpectrogramQuery
  ),
  responses(
    (status = 200, content_type = "image/png", body = api_types::files::FileContent),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn spectrogram_file(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<SpectrogramQuery>,
) -> Result<Response, ApiError> {
  let target = file(&name, &path)?;
  app.services.analysis.spectrogram(target, query).await
}

/// A succeeded download's spectrogram as a PNG image.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/analysis/spectrogram.png", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    SpectrogramQuery
  ),
  responses(
    (status = 200, content_type = "image/png", body = api_types::files::FileContent),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn spectrogram_download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<SpectrogramQuery>,
) -> Result<Response, ApiError> {
  let target = FileTarget::Download { username, id };
  app.services.analysis.spectrogram(target, query).await
}

/// Records the user's own verdict on a file.
#[utoipa::path(
  put, path = "/files/{root}/files/{path}/analysis/override", tag = "analysis",
  request_body = SetAnalysisOverride,
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64")
  ),
  responses(
    (status = 204, description = "recorded"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_override(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((name, path)): Path<(String, String)>,
  Json(verdict): Json<SetAnalysisOverride>,
) -> Result<StatusCode, ApiError> {
  let target = file(&name, &path)?;
  app
    .services
    .analysis
    .set_override(&caller.principal, target, verdict)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Takes the user's verdict on a file away.
#[utoipa::path(
  delete, path = "/files/{root}/files/{path}/analysis/override", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64")
  ),
  responses(
    (status = 204, description = "removed"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn clear_override(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path((name, path)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
  let target = file(&name, &path)?;
  app.services.analysis.clear_override(target).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// The analysis reports, newest first, without their windows.
#[utoipa::path(
  get, path = "/analysis", tag = "analysis", params(AnalysisListQuery),
  responses(
    (status = 200, body = Vec<AudioAnalysis>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<AnalysisListQuery>,
) -> Result<Json<Vec<AudioAnalysis>>, ApiError> {
  validate::positive("limit", query.limit)?;
  app.services.analysis.list(query).await.map(Json)
}

/// The analyses queued and running.
#[utoipa::path(
  get, path = "/analysis/jobs", tag = "analysis",
  responses(
    (status = 200, body = Vec<AnalysisJob>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn jobs(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<AnalysisJob>>, ApiError> {
  app.services.analysis.jobs().await.map(Json)
}
