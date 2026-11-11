//! The analysis queue, kept as analysis jobs: each request, a file, a batch or the files of a
//! download job while `analysis.on_download` is on, is a job whose parts are its files, waiting
//! in `jobs.db` so they carry on after a restart. A paused job finishes the file it is on and
//! holds the rest. The workers that take the files are in `worker.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use api_types::analysis::{
  AnalysisBatchQueued, AnalysisBatchRefusal, AnalysisJob, AnalysisJobState, AnalysisQueue,
  AnalysisTarget,
};
use api_types::files::FileRoot;
use api_types::jobs::{JobKind, JobPartState, JobStarter, JobStarterKind, JobState};
use api_types::live::LiveFrame;
use chrono::{DateTime, Utc};
use reseek_shares::lock;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::{Analysis, current, store};
use crate::api::error::ApiError;
use crate::config::Analysis as AnalysisOptions;
use crate::files::LocalFile;
use crate::jobs::{Ended, NewJob, NewPart, StoredPart, store as job_store};
use crate::services::FileTarget;

/// The most files one batch may name.
pub(super) const BATCH_LIMIT: usize = 100_000;

/// The most waiting files the queue listing names; the rest are counted.
const LISTED: usize = 1000;

/// A file of an analysis job as its part keeps it: what names it and where it was found.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Target {
  /// what names the file
  pub target: AnalysisTarget,
  /// the directory it is under
  pub root: FileRoot,
  /// its path under the root
  pub relative: String,
  /// its local path
  pub path: String,
  /// the download it belongs to
  pub transfer_id: Option<Uuid>,
}

impl Target {
  /// The part target of `file`, named by `target`.
  fn of(target: &FileTarget, file: &LocalFile) -> Self {
    Self {
      target: match target {
        FileTarget::File { root, path } => AnalysisTarget::File {
          root: *root,
          path: path.clone(),
        },
        FileTarget::Download { username, id } => AnalysisTarget::Download {
          username: username.clone(),
          id: *id,
        },
      },
      root: file.root,
      relative: file.relative.clone(),
      path: file.path.to_string_lossy().into_owned(),
      transfer_id: file.transfer_id,
    }
  }

  /// The file target this names.
  pub(super) fn file_target(&self) -> FileTarget {
    match &self.target {
      AnalysisTarget::File { root, path } => FileTarget::File {
        root: *root,
        path: path.clone(),
      },
      AnalysisTarget::Download { username, id } => FileTarget::Download {
        username: username.clone(),
        id: *id,
      },
    }
  }

  /// The part of a job this file is, waiting.
  fn part(&self) -> NewPart {
    NewPart {
      name: file_name(&self.relative).to_string(),
      state: JobPartState::Waiting,
      error: None,
      target: Some(
        serde_json::to_string(self).expect("a target of text, ids and a root is written as JSON"),
      ),
      key: Some(self.path.clone()),
    }
  }

  /// The listing of this file waiting as the part `id` of a job made at `queued_at`.
  pub(super) fn listed(&self, id: Uuid, queued_at: DateTime<Utc>) -> AnalysisJob {
    AnalysisJob {
      id,
      path: self.path.clone(),
      root: self.root,
      relative_path: self.relative.clone(),
      transfer_id: self.transfer_id,
      state: AnalysisJobState::Queued,
      queued_at,
      started_at: None,
      progress: None,
    }
  }
}

/// The last segment of a `/`-separated path.
fn file_name(relative: &str) -> &str {
  relative.rsplit('/').next().unwrap_or(relative)
}

/// A file under analysis.
#[derive(Clone, Debug)]
pub(super) struct Running {
  /// its part
  pub part: Uuid,
  /// its job
  pub job: Uuid,
  pub file: LocalFile,
  pub queued_at: DateTime<Utc>,
  pub started_at: DateTime<Utc>,
  /// canceled when its part or job is canceled or the daemon stops
  pub cancel: CancellationToken,
  /// how far the worker is, in thousandths of the file
  pub progress: Arc<AtomicU32>,
}

impl Running {
  /// The file as the queue listing shows it.
  fn listed(&self) -> AnalysisJob {
    AnalysisJob {
      id: self.part,
      path: self.file.path.to_string_lossy().into_owned(),
      root: self.file.root,
      relative_path: self.file.relative.clone(),
      transfer_id: self.file.transfer_id,
      state: AnalysisJobState::Running,
      queued_at: self.queued_at,
      started_at: Some(self.started_at),
      progress: Some(f64::from(self.progress.load(Ordering::Relaxed)) / 1000.0),
    }
  }
}

/// What asking for one file's analysis came to.
pub(super) enum Queued {
  /// queued in this job
  Job(Uuid),
  /// its report is current, so it was left alone
  Current,
  /// it is queued or running already
  Already,
}

/// The name of an analysis job of `targets`: its file, its folder or how many files.
fn job_name(targets: &[Target]) -> String {
  match targets {
    [one] => format!("Analyze {}", file_name(&one.relative)),
    _ => {
      let folder = common_folder(targets.iter().map(|t| t.relative.as_str()));
      match folder.rsplit('/').next().filter(|name| !name.is_empty()) {
        Some(name) => format!("Analyze {name}"),
        None => format!("Analyze {} files", targets.len()),
      }
    }
  }
}

/// The deepest folder holding every one of `paths`, `/`-separated; empty when none does.
fn common_folder<'a>(mut paths: impl Iterator<Item = &'a str>) -> &'a str {
  let parent = |path: &'a str| path.rsplit_once('/').map_or("", |(dir, _)| dir);
  let Some(first) = paths.next() else {
    return "";
  };
  let mut common = parent(first);
  for path in paths {
    while !common.is_empty()
      && !path
        .strip_prefix(common)
        .is_some_and(|rest| rest.starts_with('/'))
    {
      common = parent(common);
    }
  }
  common
}

impl Analysis {
  /// The target the stored part `part` keeps, none when it keeps none that reads, which is
  /// said once per part.
  pub(super) fn target_of(&self, part: &StoredPart) -> Option<Target> {
    let read = part
      .target
      .as_deref()
      .map(serde_json::from_str::<Target>)
      .transpose();
    match read {
      Ok(target) => target,
      Err(e) => {
        if lock(&self.unreadable).insert(part.id.to_string()) {
          warn!(part = %part.id, job = %part.job, error = %e, "a file of an analysis job does not read");
        }
        None
      }
    }
  }

  /// Whether `path` is waiting or running in any analysis job.
  async fn holds(&self, path: &std::path::Path) -> Result<bool, ApiError> {
    if lock(&self.running).iter().any(|r| r.file.path == path) {
      return Ok(true);
    }
    let text = path.to_string_lossy().into_owned();
    self
      .jobs
      .database()
      .read(move |conn| {
        conn.query_row(
          "SELECT EXISTS (SELECT 1 FROM job_parts p JOIN jobs j ON j.id = p.job_id \
           WHERE p.key = ?1 AND j.kind = 'analysis' \
           AND p.state IN ('waiting', 'paused', 'running'))",
          [text],
          |row| row.get(0),
        )
      })
      .await
      .map_err(ApiError::internal)
  }

  /// Whether the report of `file` is current.
  pub(super) async fn is_current(&self, file: &LocalFile) -> Result<bool, ApiError> {
    let path = file.path.to_string_lossy().into_owned();
    let stored = store::find(&self.db, &path)
      .await
      .map_err(ApiError::internal)?;
    // a stale report is no reading of the file, so it is analyzed again
    Ok(
      stored
        .as_ref()
        .is_some_and(|stored| !stored.report.stale && current(stored, file.size, file.modified)),
    )
  }

  /// Queues the file `target` names as a job of its own started by the user, unless its report
  /// is current and not `force`d or it is queued already.
  pub(super) async fn enqueue_one(
    &self,
    target: &FileTarget,
    force: bool,
  ) -> Result<Queued, ApiError> {
    let file = self.files.local_file(target).await?;
    if !force && self.is_current(&file).await? {
      return Ok(Queued::Current);
    }
    if self.holds(&file.path).await? {
      return Ok(Queued::Already);
    }
    let target = Target::of(target, &file);
    let job = self
      .start_job(vec![target], force, JobStarter::you(), None)
      .await?;
    Ok(Queued::Job(job))
  }

  /// Queues the files of a batch as one job started by the user, each named with its place in
  /// the batch; files whose report is current or that are queued already are left alone, and
  /// those that cannot be found refused.
  pub(super) async fn batch(&self, files: Vec<(u32, FileTarget)>) -> AnalysisBatchQueued {
    let mut result = AnalysisBatchQueued {
      queued: 0,
      current: 0,
      refused: Vec::new(),
      job_id: None,
    };
    let mut targets = Vec::new();
    for (index, target) in files {
      if targets.len() >= BATCH_LIMIT {
        result.refused.push(AnalysisBatchRefusal {
          index,
          error: format!("A batch analyzes at most {BATCH_LIMIT} files"),
        });
        continue;
      }
      let checked = async {
        let file = self.files.local_file(&target).await?;
        let skip = self.is_current(&file).await? || self.holds(&file.path).await?;
        Ok::<_, ApiError>((file, skip))
      };
      match checked.await {
        Ok((_, true)) => result.current += 1,
        Ok((file, false)) => targets.push(Target::of(&target, &file)),
        Err(e) => result.refused.push(AnalysisBatchRefusal {
          index,
          error: e.message().to_string(),
        }),
      }
    }
    if targets.is_empty() {
      return result;
    }
    let count = u32::try_from(targets.len()).unwrap_or(u32::MAX);
    match self
      .start_job(targets, false, JobStarter::you(), None)
      .await
    {
      Ok(job) => {
        result.queued = count;
        result.job_id = Some(job);
      }
      Err(e) => {
        error!(error = ?e, files = count, "batch analysis not queued");
        result.refused.push(AnalysisBatchRefusal {
          index: 0,
          error: e.message().to_string(),
        });
      }
    }
    result
  }

  /// Queues the file the download `id` from `username` of the download job `job` finished into
  /// at `path`, when `analysis.on_download` is on and the file's extension is in
  /// `analysis.extensions`: a part of the one analysis job that follows the download job.
  /// `analysis` is the options read when the download's completion began, before its move, so a
  /// change made once it shows as finished does not apply to it.
  pub async fn downloaded(
    &self,
    analysis: &AnalysisOptions,
    username: &str,
    id: Uuid,
    job: Option<Uuid>,
    path: &std::path::Path,
  ) {
    if !analysis.on_download || !listed(&analysis.extensions, path) {
      return;
    }
    let target = FileTarget::Download {
      username: username.to_string(),
      id,
    };
    let file = match self.files.local_file(&target).await {
      Ok(file) => file,
      Err(e) => {
        return warn!(path = %path.display(), error = ?e, "finished download not queued for analysis");
      }
    };
    match self.is_current(&file).await {
      Ok(true) => return debug!(path = %path.display(), "finished download analyzed already"),
      Ok(false) => {}
      Err(e) => {
        return warn!(path = %path.display(), error = ?e, "finished download not queued for analysis");
      }
    }
    let part = Target::of(&target, &file);
    let added = match job {
      Some(download_job) => self.follow(download_job, part).await,
      None => self
        .start_job(
          vec![part],
          false,
          JobStarter::of(JobStarterKind::Download),
          None,
        )
        .await
        .map(drop),
    };
    if let Err(e) = added {
      warn!(path = %path.display(), error = ?e, "finished download not queued for analysis");
    }
  }

  /// Adds `part` to the analysis job that follows the download job `download_job`, made when
  /// there is none yet.
  async fn follow(&self, download_job: Uuid, part: Target) -> Result<(), ApiError> {
    // one follower is made at a time, so two files of a job do not make two
    let _making = self.making.lock().await;
    let followers = self
      .jobs
      .unended(JobKind::Analysis)
      .await
      .map_err(ApiError::internal)?;
    let Some(follower) = followers
      .into_iter()
      .find(|job| job.follows == Some(download_job))
    else {
      let name = match self.jobs.get(download_job).await {
        Ok(Some(job)) => format!("Analyze {}", job.name),
        _ => job_name(std::slice::from_ref(&part)),
      };
      return self
        .start_named(
          name,
          vec![part],
          false,
          JobStarter::of(JobStarterKind::Download),
          Some(download_job),
        )
        .await
        .map(drop);
    };
    let mut new = part.part();
    // a file joining a paused job is held with the rest
    if follower.state == JobState::Paused {
      new.state = JobPartState::Paused;
    }
    let job = follower.id;
    let ids = self
      .jobs
      .database()
      .write(move |conn| job_store::add_parts(conn, job, &[new]))
      .await
      .map_err(ApiError::internal)?;
    let queued = part.listed(ids[0], follower.created_at);
    self.settle(job).await;
    self.live.publish(LiveFrame::FilesAnalysisQueued(queued));
    self.wake.notify_one();
    Ok(())
  }

  /// Tells that the download job `job` ended, so the analysis job following it ends once its
  /// files are done.
  pub async fn download_job_ended(&self, job: Uuid) {
    match self.jobs.unended(JobKind::Analysis).await {
      Ok(followers) => {
        for follower in followers.into_iter().filter(|f| f.follows == Some(job)) {
          self.settle(follower.id).await;
        }
      }
      Err(e) => warn!(%job, error = %e, "the analysis following a download job not checked"),
    }
  }

  /// Makes a job of `targets` named after them and wakes the queue; returns its id.
  async fn start_job(
    &self,
    targets: Vec<Target>,
    force: bool,
    starter: JobStarter,
    follows: Option<Uuid>,
  ) -> Result<Uuid, ApiError> {
    let name = job_name(&targets);
    self
      .start_named(name, targets, force, starter, follows)
      .await
  }

  async fn start_named(
    &self,
    name: String,
    targets: Vec<Target>,
    force: bool,
    starter: JobStarter,
    follows: Option<Uuid>,
  ) -> Result<Uuid, ApiError> {
    let first = targets.first().cloned();
    let parts: Vec<NewPart> = targets.iter().map(Target::part).collect();
    let new = NewJob {
      id: Uuid::new_v4(),
      kind: JobKind::Analysis,
      name,
      starter,
      state: JobState::Queued,
      follows,
      data: Some(serde_json::json!({ "force": force })),
    };
    let (job, ids) = self
      .jobs
      .create_with_parts(new, parts)
      .await
      .map_err(ApiError::internal)?;
    debug!(job = %job.id, files = job.counts.total, "analysis queued");
    // a client that sees nothing waiting or running stops reading the queue until this says
    if let (Some(first), Some(id)) = (first, ids.first()) {
      self.live.publish(LiveFrame::FilesAnalysisQueued(
        first.listed(*id, job.created_at),
      ));
    }
    self.wake.notify_one();
    Ok(job.id)
  }

  /// The queue: the files being analyzed, then the first of those waiting, and how many more
  /// wait.
  pub(super) async fn queue(&self) -> Result<AnalysisQueue, ApiError> {
    let mut jobs: Vec<AnalysisJob> = lock(&self.running).iter().map(Running::listed).collect();
    let (taken, (waiting, total)) = self
      .jobs
      .database()
      .read(|conn| {
        let total: i64 = conn.query_row(
          "SELECT COUNT(*) FROM job_parts p JOIN jobs j ON j.id = p.job_id \
           WHERE j.kind = 'analysis' AND p.state = 'waiting'",
          [],
          |row| row.get(0),
        )?;
        let waiting = job_store::parts_in(
          conn,
          JobPartState::Waiting,
          &[JobState::Queued, JobState::Running],
          LISTED,
        )?;
        let mut listed = Vec::with_capacity(waiting.len());
        for part in waiting {
          let at = job_store::get(conn, part.job)?.map_or_else(Utc::now, |job| job.created_at);
          listed.push((part, at));
        }
        // a file is running here from when it is taken until its worker starts
        let taken = job_store::parts_in(
          conn,
          JobPartState::Running,
          &[JobState::Queued, JobState::Running, JobState::Paused],
          LISTED,
        )?;
        Ok((taken, (listed, total)))
      })
      .await
      .map_err(ApiError::internal)?;
    let started: Vec<Uuid> = jobs.iter().map(|j| j.id).collect();
    jobs.extend(
      taken
        .into_iter()
        .filter(|part| !started.contains(&part.id))
        .filter_map(|part| {
          let target = self.target_of(&part)?;
          Some(AnalysisJob {
            state: AnalysisJobState::Running,
            ..target.listed(part.id, Utc::now())
          })
        }),
    );
    let shown = waiting.len();
    jobs.extend(
      waiting
        .into_iter()
        .filter_map(|(part, at)| Some(self.target_of(&part)?.listed(part.id, at))),
    );
    let more = usize::try_from(total).unwrap_or(0).saturating_sub(shown);
    Ok(AnalysisQueue {
      jobs,
      backlog: u32::try_from(more).unwrap_or(u32::MAX),
    })
  }

  /// Cancels the waiting or running file `part`, or every file of the analysis job `part`
  /// names; 404 when it is neither.
  pub(super) async fn cancel_part(&self, part: Uuid) -> Result<(), ApiError> {
    if let Ok(Some(job)) = self.jobs.get(part).await
      && job.kind == JobKind::Analysis
      && job.state != JobState::Ended
    {
      return self.cancel_job_files(part).await;
    }
    if let Some(running) = lock(&self.running).iter().find(|r| r.part == part) {
      // the worker sees it at its next window and ends the part itself
      running.cancel.cancel();
      return Ok(());
    }
    let ended = self
      .jobs
      .database()
      .write(move |conn| {
        let Some(stored) = job_store::part_by_id(conn, part)? else {
          return Ok(None);
        };
        if !matches!(
          stored.part.state,
          JobPartState::Waiting | JobPartState::Paused
        ) {
          return Ok(None);
        }
        job_store::set_part(conn, part, JobPartState::Canceled, None)?;
        Ok(Some(stored.job))
      })
      .await
      .map_err(ApiError::internal)?;
    let job = ended.ok_or_else(|| ApiError::not_found(format!("No analysis job {part}")))?;
    debug!(%part, "queued analysis taken off the queue");
    self.settle(job).await;
    Ok(())
  }

  /// Cancels every waiting file of every analysis job; the running ones go on.
  pub(super) async fn cancel_waiting(&self) -> Result<(), ApiError> {
    let jobs = self
      .jobs
      .unended(JobKind::Analysis)
      .await
      .map_err(ApiError::internal)?;
    for job in jobs {
      let id = job.id;
      self
        .jobs
        .database()
        .write(move |conn| {
          job_store::set_parts(conn, id, JobPartState::Waiting, JobPartState::Canceled)?;
          job_store::set_parts(conn, id, JobPartState::Paused, JobPartState::Canceled)
        })
        .await
        .map_err(ApiError::internal)?;
      self.settle(id).await;
    }
    Ok(())
  }

  /// Cancels the analysis job `job`: its waiting files and the one running.
  pub async fn cancel_job_files(&self, job: Uuid) -> Result<(), ApiError> {
    self
      .jobs
      .database()
      .write(move |conn| {
        job_store::set_parts(conn, job, JobPartState::Waiting, JobPartState::Canceled)?;
        job_store::set_parts(conn, job, JobPartState::Paused, JobPartState::Canceled)
      })
      .await
      .map_err(ApiError::internal)?;
    for running in lock(&self.running).iter().filter(|r| r.job == job) {
      running.cancel.cancel();
    }
    info!(%job, "analysis job canceled");
    self.settle(job).await;
    Ok(())
  }

  /// Pauses the analysis job `job`: the file it is on is finished and the rest held.
  pub async fn pause_job(&self, job: Uuid) -> Result<(), ApiError> {
    self
      .jobs
      .database()
      .write(move |conn| {
        job_store::set_parts(conn, job, JobPartState::Waiting, JobPartState::Paused)
      })
      .await
      .map_err(ApiError::internal)?;
    self.settle(job).await;
    info!(%job, "analysis job paused");
    Ok(())
  }

  /// Resumes the paused analysis job `job`.
  pub async fn resume_job(&self, job: Uuid) -> Result<(), ApiError> {
    self
      .jobs
      .database()
      .write(move |conn| {
        job_store::set_parts(conn, job, JobPartState::Paused, JobPartState::Waiting)
      })
      .await
      .map_err(ApiError::internal)?;
    self.settle(job).await;
    info!(%job, "analysis job resumed");
    self.wake.notify_one();
    Ok(())
  }

  /// Analyzes the files of the ended analysis job `job` that failed or were canceled again, as
  /// a new job started by the user; returns its id. 409 when there are none.
  pub async fn run_again(&self, job: Uuid) -> Result<Uuid, ApiError> {
    let parts = self
      .jobs
      .database()
      .read(move |conn| job_store::parts(conn, job))
      .await
      .map_err(ApiError::internal)?;
    let targets: Vec<Target> = parts
      .into_iter()
      .filter(|p| matches!(p.part.state, JobPartState::Failed | JobPartState::Canceled))
      .filter_map(|p| self.target_of(&p))
      .collect();
    if targets.is_empty() {
      return Err(ApiError::conflict("No file of the job failed", None));
    }
    self.start_job(targets, true, JobStarter::you(), None).await
  }

  /// Ends the job `job` when no file of it waits or runs and the download job it follows, if
  /// any, has ended; else brings its counts up to date.
  pub(super) async fn settle(&self, job: Uuid) {
    let started = lock(&self.running).iter().any(|r| r.job == job);
    let found = self
      .jobs
      .database()
      .read(move |conn| {
        let Some(found) = job_store::get(conn, job)? else {
          return Ok(None);
        };
        let counts = job_store::counts(conn, job)?;
        let (waiting, taken) = job_store::open_parts(conn, job)?;
        let follows_open = match found.follows {
          Some(followed) => {
            job_store::get(conn, followed)?.is_some_and(|f| f.state != JobState::Ended)
          }
          None => false,
        };
        Ok(Some((counts, waiting, taken, follows_open)))
      })
      .await;
    let (counts, waiting, taken, follows_open) = match found {
      Ok(Some(found)) => found,
      Ok(None) => return,
      Err(e) => return warn!(%job, error = %e, "analysis job not checked"),
    };
    // a file taken but not yet started is running in the database alone
    let running = started || taken > 0;
    let open = counts.total.saturating_sub(counts.ended());
    let result = if open == 0 && !running && !follows_open {
      self
        .jobs
        .end(
          job,
          Ended {
            counts: Some(counts),
            ..Ended::default()
          },
        )
        .await
        .map(drop)
    } else {
      let state = if counts.paused > 0 && waiting == 0 {
        JobState::Paused
      } else if running {
        JobState::Running
      } else {
        JobState::Queued
      };
      self.jobs.set_counts(job, counts, state).await.map(drop)
    };
    if let Err(e) = result {
      warn!(%job, error = %e, "analysis job not updated");
    }
  }
}

/// Whether the extension of `path` is one of `extensions`, ignoring case.
fn listed(extensions: &[String], path: &std::path::Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      extensions
        .iter()
        .any(|listed| listed.eq_ignore_ascii_case(extension))
    })
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::{common_folder, listed};

  #[test]
  fn listed_extensions_match_without_case_and_only_the_last_one() {
    let extensions = ["flac".to_string(), "wav".to_string()];
    assert!(listed(&extensions, Path::new("/d/album/01 song.FLAC")));
    assert!(listed(&extensions, Path::new("/d/a.mp3.wav")));
    assert!(!listed(&extensions, Path::new("/d/a.flac.mp3")));
    assert!(!listed(&extensions, Path::new("/d/flac")));
    assert!(!listed(&[], Path::new("/d/a.flac")));
  }

  #[test]
  fn a_batch_is_named_by_its_deepest_common_folder() {
    let folder = |paths: &[&'static str]| common_folder(paths.iter().copied());
    assert_eq!(folder(&["A/Album/1.flac", "A/Album/CD2/1.flac"]), "A/Album");
    assert_eq!(folder(&["A/Album/1.flac", "A/Albums/1.flac"]), "A");
    assert_eq!(folder(&["1.flac", "A/2.flac"]), "");
  }
}
