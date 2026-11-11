//! Jobs: the longer work reseek does for the user or on its own, such as a download job, a share
//! scan, an analysis of a folder or the cleanup, listed with how far each is and how it ended,
//! and the schedules that start some of them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// What a job does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
  /// files downloaded together: a file, a folder, an album or a pick from a search
  Download,
  /// a scan of the shared directories
  Scan,
  /// the audio analysis of one or more files
  Analysis,
  /// the cleanup of what the retention options let go, or a database vacuum
  Maintenance,
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
  /// waiting for its turn
  Queued,
  /// working
  Running,
  /// held by the user until resumed
  Paused,
  /// over; `ending` says how
  Ended,
}

/// How a job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobEnding {
  /// every part succeeded, or the parts that did not were canceled
  Succeeded,
  /// some parts succeeded and some failed
  FinishedWithFailures,
  /// nothing succeeded, or the job itself failed
  Failed,
  /// the user canceled it
  Canceled,
}

/// Who or what started a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStarterKind {
  /// a user of this reseek, through the UI or the API
  You,
  /// a schedule, named by `name`
  Schedule,
  /// a download job, for the analysis of its files when `analysis.on_download` is on
  Download,
  /// reseek starting up again, such as the scan that checks the shares after a restart
  Resumed,
  /// a Soulseek user, named by `name`, who sent us files without being asked
  User,
}

/// Who or what started a job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobStarter {
  /// the kind of starter
  pub kind: JobStarterKind,
  /// the schedule or the Soulseek user that started it; null for the other kinds
  #[schema(example = "cleanup")]
  pub name: Option<String>,
}

impl JobStarter {
  /// A job the user started.
  pub fn you() -> Self {
    Self {
      kind: JobStarterKind::You,
      name: None,
    }
  }

  /// A job a starter of `kind` with no name started.
  pub fn of(kind: JobStarterKind) -> Self {
    Self { kind, name: None }
  }

  /// A job the schedule `name` started.
  pub fn schedule(name: &str) -> Self {
    Self {
      kind: JobStarterKind::Schedule,
      name: Some(name.to_string()),
    }
  }

  /// A job the Soulseek user `username` started by sending us files unasked.
  pub fn user(username: &str) -> Self {
    Self {
      kind: JobStarterKind::User,
      name: Some(username.to_string()),
    }
  }
}

/// How many of a job's parts ended which way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobCounts {
  /// every part
  pub total: u32,
  /// parts that succeeded
  pub succeeded: u32,
  /// parts that failed
  pub failed: u32,
  /// parts that were canceled
  pub canceled: u32,
  /// parts held by a pause
  pub paused: u32,
}

impl JobCounts {
  /// Parts that ended, whichever way.
  pub fn ended(&self) -> u32 {
    self.succeeded + self.failed + self.canceled
  }

  /// How a job with these parts ended, when every part has: canceled when every part was,
  /// failed when some failed and none succeeded, finished with failures when some failed and
  /// some succeeded, else succeeded.
  pub fn ending(&self) -> JobEnding {
    if self.total > 0 && self.canceled == self.total {
      JobEnding::Canceled
    } else if self.failed == 0 {
      JobEnding::Succeeded
    } else if self.succeeded == 0 {
      JobEnding::Failed
    } else {
      JobEnding::FinishedWithFailures
    }
  }
}

/// A job, as the list shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Job {
  /// the job's id; a download job's is the `jobId` of its downloads
  pub id: Uuid,
  /// what it does
  pub kind: JobKind,
  /// what it works on, made from its kind and subject
  #[schema(example = "Album from alice")]
  pub name: String,
  /// who or what started it
  pub starter: JobStarter,
  /// where it is
  pub state: JobState,
  /// how it ended, null until it does
  pub ending: Option<JobEnding>,
  /// why it failed, when the job itself did
  pub error: Option<String>,
  /// what it came to in words, such as how much a cleanup removed; null when there is nothing
  /// to say
  #[schema(example = "Removed 12 finished transfers")]
  pub summary: Option<String>,
  /// how many parts it has and how they ended
  pub counts: JobCounts,
  /// how far a job without parts is, from 0 to 1; null when it does not say
  pub progress: Option<f64>,
  /// the download job an analysis job analyzes the files of, null otherwise
  pub follows: Option<Uuid>,
  /// whether it can be paused: a running download or analysis job
  pub pausable: bool,
  /// when it was made
  pub created_at: DateTime<Utc>,
  /// when it first ran, null while queued
  pub started_at: Option<DateTime<Utc>>,
  /// when it ended, null until it does
  pub ended_at: Option<DateTime<Utc>>,
}

/// Where a part of a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobPartState {
  /// waiting for its turn
  Waiting,
  /// working
  Running,
  /// held by a pause
  Paused,
  /// done
  Succeeded,
  /// failed; `error` says why
  Failed,
  /// canceled
  Canceled,
}

/// One part of a job: a file of a download or analysis job, an area of a cleanup, a database
/// of a vacuum.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobPart {
  /// what it works on
  #[schema(example = "01 - Track.flac")]
  pub name: String,
  /// where it is
  pub state: JobPartState,
  /// why it failed, null when it did not
  pub error: Option<String>,
  /// the download a part of a download job is, while the job runs; null otherwise
  pub transfer_id: Option<Uuid>,
  /// the user a part of a download job downloads from, while the job runs; null otherwise
  pub username: Option<String>,
}

/// A job with its parts, as `GET /jobs/{id}` answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobDetail {
  /// the job
  pub job: Job,
  /// its parts in their order; a running download job's are read from its downloads
  pub parts: Vec<JobPart>,
}

/// The query of `GET /jobs`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct JobListQuery {
  /// only jobs of this kind
  pub kind: Option<JobKind>,
  /// only jobs in this state
  pub state: Option<JobState>,
  /// only jobs started by this kind of starter
  pub starter: Option<JobStarterKind>,
  /// only jobs whose name holds this text, ignoring case
  pub q: Option<String>,
}

/// The jobs, as `GET /jobs` answers them: running and paused ones first, then queued ones in
/// their order, then the ended ones, newest first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobList {
  /// the jobs
  pub jobs: Vec<Job>,
}

/// Jobs taken out of the list, as a `jobs.removed` frame carries them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobsRemoved {
  /// the ids of the jobs removed
  pub ids: Vec<Uuid>,
}

/// What `POST /jobs/{id}/run-again` started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobStarted {
  /// the new job's id; null for a scan or maintenance run, which is listed as it starts
  pub id: Option<Uuid>,
}

/// A recurring piece of work, as the schedules card shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobSchedule {
  /// the schedule's name, which its jobs' starter carries
  #[schema(example = "cleanup")]
  pub name: String,
  /// what it is called
  #[schema(example = "Cleanup")]
  pub title: String,
  /// what one run does
  pub description: String,
  /// how often it runs, as written in its option, such as `5m` or `7d at 04:00`; null when it
  /// is off
  #[schema(example = "7d at 04:00")]
  pub every: Option<String>,
  /// the option that sets it, null when it cannot be changed
  #[schema(example = "schedules.cleanup")]
  pub option: Option<String>,
  /// why it is fixed or off, when it is
  pub note: Option<String>,
  /// whether it can be run now
  pub runnable: bool,
  /// whether a run is going on now
  pub running: bool,
  /// when it last ran, null when it never did
  pub last_run_at: Option<DateTime<Utc>>,
  /// when its last run ended
  pub last_ended_at: Option<DateTime<Utc>>,
  /// how its last run ended
  pub last_ending: Option<JobEnding>,
  /// what its last run came to in words
  pub last_summary: Option<String>,
  /// why its last run failed
  pub last_error: Option<String>,
  /// the job of its last run, when that run is in the list
  pub last_job_id: Option<Uuid>,
  /// when it runs next, null when it is off
  pub next_run_at: Option<DateTime<Utc>>,
}

/// The jobs at a glance, as the bottom bar shows them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobsSummary {
  /// jobs running or queued
  pub running: u32,
  /// jobs paused
  pub paused: u32,
  /// when a job last ended failed or finished with failures, null when none in the list did
  pub last_failed_at: Option<DateTime<Utc>>,
}
