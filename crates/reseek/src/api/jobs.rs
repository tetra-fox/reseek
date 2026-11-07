//! The jobs routes: every job of every kind listed and controlled in one place, and the
//! schedules. Each kind keeps its own call to start a job; these cancel, pause, resume, run
//! again and remove them, by kind.

use api_types::jobs::{
  Job, JobDetail, JobEnding, JobKind, JobList, JobListQuery, JobSchedule, JobStarted, JobState,
};
use axum::extract::State;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};
use crate::jobs::schedules;

/// The jobs routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list, clear))
    .routes(routes!(schedules_list))
    .routes(routes!(run_schedule))
    .routes(routes!(detail, remove))
    .routes(routes!(cancel))
    .routes(routes!(pause))
    .routes(routes!(resume))
    .routes(routes!(run_again))
}

/// The job `id`, 404 when there is none.
async fn job(app: &AppState, id: Uuid) -> Result<Job, ApiError> {
  app
    .jobs
    .get(id)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::not_found(format!("No job {id}")))
}

/// Every job: running and paused ones first, then queued ones, then the ended ones newest
/// first, as many as `retention.jobs` keeps.
#[utoipa::path(
  operation_id = "list_jobs",
  get, path = "/jobs", tag = "jobs", params(JobListQuery),
  responses(
    (status = 200, body = JobList), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<JobListQuery>,
) -> Result<Json<JobList>, ApiError> {
  let jobs = app.jobs.list(query).await.map_err(ApiError::internal)?;
  Ok(Json(JobList { jobs }))
}

/// Takes every ended job out of the list.
#[utoipa::path(
  operation_id = "clear_ended_jobs",
  delete, path = "/jobs", tag = "jobs",
  responses(
    (status = 204, description = "cleared"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn clear(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
) -> Result<StatusCode, ApiError> {
  app.jobs.clear_ended().await.map_err(ApiError::internal)?;
  Ok(StatusCode::NO_CONTENT)
}

/// A job with its parts; a running download job's parts are its downloads.
#[utoipa::path(
  operation_id = "job_detail",
  get, path = "/jobs/{id}", tag = "jobs",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 200, body = JobDetail), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn detail(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<Uuid>,
) -> Result<Json<JobDetail>, ApiError> {
  let mut detail = app
    .jobs
    .detail(id)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::not_found(format!("No job {id}")))?;
  if detail.job.kind == JobKind::Download && detail.job.state != JobState::Ended {
    detail.parts = app.downloads.job_parts(id).await?;
  }
  Ok(Json(detail))
}

/// Takes an ended job out of the list; 409 for a job that has not ended.
#[utoipa::path(
  operation_id = "remove_job",
  delete, path = "/jobs/{id}", tag = "jobs",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 204, description = "removed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn remove(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  let job = job(&app, id).await?;
  if job.state != JobState::Ended {
    return Err(ApiError::conflict(
      "The job has not ended",
      Some("Cancel it first.".to_string()),
    ));
  }
  app
    .jobs
    .remove(vec![id])
    .await
    .map_err(ApiError::internal)?;
  Ok(StatusCode::NO_CONTENT)
}

/// Cancels a job: a download job's downloads, an analysis job's files, the share scan, or a
/// maintenance run between its steps. 409 for a job that has ended.
#[utoipa::path(
  operation_id = "cancel_any_job",
  post, path = "/jobs/{id}/cancel", tag = "jobs",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 204, description = "canceled"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn cancel(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  let job = job(&app, id).await?;
  if job.state == JobState::Ended {
    return Err(ApiError::conflict("The job has ended", None));
  }
  match job.kind {
    JobKind::Download => app.downloads.cancel_job(id).await?,
    JobKind::Analysis => app.analysis.cancel_job_files(id).await?,
    JobKind::Scan => app
      .shares
      .cancel_scan()
      .map_err(|_| ApiError::conflict("No share scan is in progress", None))?,
    JobKind::Maintenance => {
      if !app.jobs.cancel_held(id) {
        return Err(ApiError::conflict(
          "The job can no longer be canceled",
          None,
        ));
      }
    }
  }
  Ok(StatusCode::NO_CONTENT)
}

/// Pauses a download or analysis job. A download job stops the files in progress, keeping what
/// arrived, and holds the rest; each user forgets its place in their queue. An analysis job
/// finishes the file it is on and holds the rest. 409 for any other job.
#[utoipa::path(
  operation_id = "pause_job",
  post, path = "/jobs/{id}/pause", tag = "jobs",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 204, description = "paused"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn pause(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  let job = job(&app, id).await?;
  if !job.pausable {
    return Err(ApiError::conflict("The job cannot be paused", None));
  }
  match job.kind {
    JobKind::Download => app.downloads.pause_job(id).await?,
    JobKind::Analysis => app.analysis.pause_job(id).await?,
    JobKind::Scan | JobKind::Maintenance => unreachable!("only download and analysis pause"),
  }
  Ok(StatusCode::NO_CONTENT)
}

/// Resumes a paused download or analysis job; 409 for any other job.
#[utoipa::path(
  operation_id = "resume_job",
  post, path = "/jobs/{id}/resume", tag = "jobs",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 204, description = "resumed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn resume(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  let job = job(&app, id).await?;
  if !job.pausable {
    return Err(ApiError::conflict("The job cannot be resumed", None));
  }
  match job.kind {
    JobKind::Download => app.downloads.resume_job(id).await?,
    JobKind::Analysis => app.analysis.resume_job(id).await?,
    JobKind::Scan | JobKind::Maintenance => unreachable!("only download and analysis pause"),
  }
  Ok(StatusCode::NO_CONTENT)
}

/// Starts a job that did not succeed again, as a new job; the old one stays in the list. A
/// download job queues its failed and canceled files again, an analysis job analyzes its failed
/// files again, a scan or maintenance run runs again. The new job's id is given when it is
/// known at once; a scan's and a maintenance run's show in the list as they start.
#[utoipa::path(
  operation_id = "run_job_again",
  post, path = "/jobs/{id}/run-again", tag = "jobs",
  params(("id" = Uuid, Path, description = "the job's id")),
  responses(
    (status = 202, body = JobStarted), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn run_again(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<JobStarted>), ApiError> {
  let job = job(&app, id).await?;
  if job.state != JobState::Ended || job.ending == Some(JobEnding::Succeeded) {
    return Err(ApiError::conflict(
      "Only a job that ended without succeeding can be run again",
      None,
    ));
  }
  let data = app.jobs.data(id).await.map_err(ApiError::internal)?;
  let started = match job.kind {
    JobKind::Download => Some(app.downloads.run_again(id).await?),
    JobKind::Analysis => Some(app.analysis.run_again(id).await?),
    JobKind::Scan => {
      let force = data
        .as_ref()
        .and_then(|data| data["force"].as_bool())
        .unwrap_or(false);
      app
        .shares
        .scan(force, crate::shares::ScanOrigin::Asked)
        .map_err(|_| ApiError::conflict("A share scan is already in progress", None))?;
      None
    }
    JobKind::Maintenance => {
      crate::jobs::maintenance::run_again(&app, data.as_ref())?;
      None
    }
  };
  Ok((StatusCode::ACCEPTED, Json(JobStarted { id: started })))
}

/// Every schedule, as the schedules card shows it: what it does, how often, its last and its
/// next run.
#[utoipa::path(
  operation_id = "list_schedules",
  get, path = "/jobs/schedules", tag = "jobs",
  responses(
    (status = 200, body = Vec<JobSchedule>), (status = 401, body = api_types::ErrorBody)
  )
)]
async fn schedules_list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<JobSchedule>>, ApiError> {
  schedules::cards(&app).await.map(Json)
}

/// Runs a schedule now, started by the user; its next run stays where it was. 409 while it
/// runs.
#[utoipa::path(
  operation_id = "run_schedule",
  post, path = "/jobs/schedules/{name}/run", tag = "jobs",
  params(("name" = String, Path, description = "the schedule's name")),
  responses(
    (status = 202, description = "started"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody)
  )
)]
async fn run_schedule(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
  schedules::run_now(&app, &name).await?;
  Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
  use crate::testing::{TRUSTED, app, request};

  #[tokio::test]
  async fn run_now_lists_its_job_on_the_stream_and_an_ended_job_is_removed_but_not_canceled() {
    // the cleanup the schedule would run at the start is off, so the run listed is Run now's
    let app = app(&format!("[schedules]\ncleanup = \"off\"\n{TRUSTED}"), &[]).await;
    let mut stream = app
      .stream_from(
        "127.0.0.1:1",
        request("GET", "/api/v0/live?topics=jobs", None),
      )
      .await;
    stream.until("jobs.schedules").await;
    let (status, _) = app
      .call("POST", "/api/v0/jobs/schedules/cleanup/run", None)
      .await;
    assert_eq!(status, 202);
    let (status, _) = app
      .call("POST", "/api/v0/jobs/schedules/wishlist/run", None)
      .await;
    assert_eq!(status, 409, "the server sets how often the wishlist runs");
    let mut ended = None;
    while ended.is_none() {
      let frame = stream.until("jobs.updated").await.pop().unwrap().data;
      if frame["state"] == "ended" {
        ended = Some(frame);
      }
    }
    let job = ended.unwrap();
    assert_eq!(job["name"], "Cleanup");
    assert_eq!(job["starter"]["kind"], "you");
    let id = job["id"].as_str().unwrap();

    let (status, _) = app
      .call("POST", &format!("/api/v0/jobs/{id}/cancel"), None)
      .await;
    assert_eq!(status, 409, "an ended job is not canceled");
    let (status, _) = app
      .call("POST", &format!("/api/v0/jobs/{id}/pause"), None)
      .await;
    assert_eq!(status, 409, "maintenance does not pause");
    let (status, listed) = app.call("GET", "/api/v0/jobs", None).await;
    assert_eq!(status, 200);
    assert_eq!(listed["jobs"][0]["id"], id);
    let (status, _) = app
      .call("DELETE", &format!("/api/v0/jobs/{id}"), None)
      .await;
    assert_eq!(status, 204);
    let removed = stream.until("jobs.removed").await.pop().unwrap().data;
    assert_eq!(removed["ids"][0], id);
  }
}
