//! The analysis routes: queueing a downloaded file for analysis, reading its report, its
//! spectrogram and its waveform, recording the user's own verdict, and listing reports and the
//! queue. A file is named as the files routes name it, or as a succeeded download.

use api_types::analysis::{
  AnalysisBatch, AnalysisBatchQueued, AnalysisBatchRefusal, AnalysisJob, AnalysisJobCreated,
  AnalysisListQuery, AnalysisReportQuery, AnalysisTarget, AnalysisUpload, AnalysisUploadQuery,
  AudioAnalysis, RunAnalysisQuery, SetAnalysisOverride, TileQuery, WindowSpectrum,
};
use api_types::files::FileRoot;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
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
use crate::services::{FileTarget, Piece};

/// The analysis routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(run_file, report_file))
    .routes(routes!(spectrogram_file))
    .routes(routes!(waveform_file))
    .routes(routes!(spectra_file))
    .routes(routes!(set_override, clear_override))
    .routes(routes!(set_download_override, clear_download_override))
    .routes(routes!(run_download, report_download))
    .routes(routes!(spectrogram_download))
    .routes(routes!(waveform_download))
    .routes(routes!(spectra_download))
    .routes(routes!(window_file))
    .routes(routes!(window_download))
    .routes(routes!(list))
    .routes(routes!(upload))
    .routes(routes!(run_batch))
    .routes(routes!(jobs))
    .routes(routes!(cancel_job))
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
  app.services.analysis.report(target, query).await.map(Json)
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
  app.services.analysis.report(target, query).await.map(Json)
}

/// A file's spectrogram: the overview of its whole length, or with `tile` that tile at full
/// detail. A link token does not load it; the session does.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/analysis/spectrogram", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    TileQuery
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent,
      description = "a four-byte little-endian length, a SpectrogramTile header as JSON, then its columns"),
    (status = 304, description = "the caller has these bytes already, by their ETag"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn spectrogram_file(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<TileQuery>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let target = file(&name, &path)?;
  app
    .services
    .analysis
    .piece(target, Piece::Spectrogram(query.tile), &headers)
    .await
}

/// A succeeded download's spectrogram: the overview, or with `tile` that tile at full detail.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/analysis/spectrogram", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    TileQuery
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent,
      description = "a four-byte little-endian length, a SpectrogramTile header as JSON, then its columns"),
    (status = 304, description = "the caller has these bytes already, by their ETag"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn spectrogram_download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<TileQuery>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let target = FileTarget::Download { username, id };
  app
    .services
    .analysis
    .piece(target, Piece::Spectrogram(query.tile), &headers)
    .await
}

/// A file's waveform: the overview of its whole length, or with `tile` that tile at full
/// detail.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/analysis/waveform", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    TileQuery
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent,
      description = "a four-byte little-endian length, a WaveformTile header as JSON, then its buckets"),
    (status = 304, description = "the caller has these bytes already, by their ETag"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn waveform_file(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<TileQuery>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let target = file(&name, &path)?;
  app
    .services
    .analysis
    .piece(target, Piece::Waveform(query.tile), &headers)
    .await
}

/// A succeeded download's waveform: the overview, or with `tile` that tile at full detail.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/analysis/waveform", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    TileQuery
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent,
      description = "a four-byte little-endian length, a WaveformTile header as JSON, then its buckets"),
    (status = 304, description = "the caller has these bytes already, by their ETag"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn waveform_download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  Query(query): Query<TileQuery>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let target = FileTarget::Download { username, id };
  app
    .services
    .analysis
    .piece(target, Piece::Waveform(query.tile), &headers)
    .await
}

/// The spectrum of every analyzed window of a file, in the order of its report's windows.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/analysis/spectra", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64")
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent,
      description = "a four-byte little-endian length, a WindowSpectra header as JSON, then the spectra"),
    (status = 304, description = "the caller has these bytes already, by their ETag"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn spectra_file(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let target = file(&name, &path)?;
  app
    .services
    .analysis
    .piece(target, Piece::Spectra, &headers)
    .await
}

/// The spectrum of every analyzed window of a succeeded download.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/analysis/spectra", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id")
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent,
      description = "a four-byte little-endian length, a WindowSpectra header as JSON, then the spectra"),
    (status = 304, description = "the caller has these bytes already, by their ETag"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn spectra_download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id)): Path<(String, Uuid)>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let target = FileTarget::Download { username, id };
  app
    .services
    .analysis
    .piece(target, Piece::Spectra, &headers)
    .await
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

/// Records the user's own verdict on a succeeded download's file.
#[utoipa::path(
  put, path = "/transfers/downloads/{username}/{id}/analysis/override", tag = "analysis",
  request_body = SetAnalysisOverride,
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id")
  ),
  responses(
    (status = 204, description = "recorded"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_download_override(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((username, id)): Path<(String, Uuid)>,
  Json(verdict): Json<SetAnalysisOverride>,
) -> Result<StatusCode, ApiError> {
  let target = FileTarget::Download { username, id };
  app
    .services
    .analysis
    .set_override(&caller.principal, target, verdict)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Takes the user's verdict on a succeeded download's file away.
#[utoipa::path(
  delete, path = "/transfers/downloads/{username}/{id}/analysis/override", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id")
  ),
  responses(
    (status = 204, description = "removed"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn clear_download_override(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path((username, id)): Path<(String, Uuid)>,
) -> Result<StatusCode, ApiError> {
  let target = FileTarget::Download { username, id };
  app.services.analysis.clear_override(target).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// The analysis reports, newest first, without their windows.
#[utoipa::path(
  operation_id = "list_analyses",
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

/// Takes a file from the caller's machine into the analysis cache, to analyze and play under the
/// `temporary` root. It leaves no report, mark or judgment and goes when the cache needs the room
/// or the daemon restarts.
#[utoipa::path(
  operation_id = "upload_for_analysis",
  post, path = "/analysis/uploads", tag = "analysis", params(AnalysisUploadQuery),
  request_body(content_type = "application/octet-stream", content = Vec<u8>),
  responses(
    (status = 201, body = AnalysisUpload), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 413, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn upload(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Query(query): Query<AnalysisUploadQuery>,
  body: Body,
) -> Result<Response, ApiError> {
  let by = &caller.principal.subject;
  let upload = app.services.analysis.upload(by, &query.name, body).await?;
  Ok((StatusCode::CREATED, Json(upload)).into_response())
}

/// Queues many files for analysis in their order, each left alone when its report is current.
/// What the queue has no room for is kept and joins it as it drains.
#[utoipa::path(
  post, path = "/analysis/batch", tag = "analysis",
  request_body = AnalysisBatch,
  responses(
    (status = 202, body = AnalysisBatchQueued), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn run_batch(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(batch): Json<AnalysisBatch>,
) -> Result<Response, ApiError> {
  let mut refused = Vec::new();
  let mut files = Vec::with_capacity(batch.files.len());
  for (index, target) in (0u32..).zip(batch.files) {
    let named = match target {
      AnalysisTarget::File { root, path } => {
        validate::relative_path(&path, false).map(|path| FileTarget::File { root, path })
      }
      AnalysisTarget::Download { username, id } => Ok(FileTarget::Download { username, id }),
    };
    match named {
      Ok(target) => files.push((index, target)),
      Err(e) => refused.push(AnalysisBatchRefusal {
        index,
        error: e.message().to_string(),
      }),
    }
  }
  let mut queued = app.services.analysis.run_batch(files).await?;
  queued.refused.extend(refused);
  queued.refused.sort_by_key(|refusal| refusal.index);
  info!(
    by = caller.principal.subject,
    queued = queued.queued,
    following = queued.following,
    current = queued.current,
    refused = queued.refused.len(),
    "analysis batch queued"
  );
  Ok((StatusCode::ACCEPTED, Json(queued)).into_response())
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

/// Takes a queued analysis off the queue, or stops a running one.
#[utoipa::path(
  delete, path = "/analysis/jobs/{id}", tag = "analysis",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 204, description = "canceled"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn cancel_job(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  app.services.analysis.cancel_job(id).await?;
  info!(by = caller.principal.subject, job = %id, "analysis canceled");
  Ok(StatusCode::NO_CONTENT)
}

/// The spectrum of one window of a file's analysis, over linear frequency bins.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/analysis/windows/{index}", tag = "analysis",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    ("index" = u32, Path, description = "the window's position in the report's windows")
  ),
  responses(
    (status = 200, body = WindowSpectrum), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn window_file(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path, index)): Path<(String, String, u32)>,
) -> Result<Json<WindowSpectrum>, ApiError> {
  let target = file(&name, &path)?;
  app.services.analysis.window(target, index).await.map(Json)
}

/// The spectrum of one window of a succeeded download's analysis.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/analysis/windows/{index}", tag = "analysis",
  params(
    ("username" = String, Path, description = "the peer"),
    ("id" = Uuid, Path, description = "the download's id"),
    ("index" = u32, Path, description = "the window's position in the report's windows")
  ),
  responses(
    (status = 200, body = WindowSpectrum), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn window_download(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((username, id, index)): Path<(String, Uuid, u32)>,
) -> Result<Json<WindowSpectrum>, ApiError> {
  let target = FileTarget::Download { username, id };
  app.services.analysis.window(target, index).await.map(Json)
}
