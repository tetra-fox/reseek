//! Download jobs: the downloads queued in one go, such as a file, a folder, an album whose
//! discs go to folders of their own, or files from several users picked from one search. A job
//! is open while its files are being enqueued. Once it is closed, the write that ends its last
//! download tells of it with a `transfers.job_finished` frame, or the close does when every
//! download ended before it. Each check runs inside a write of `transfers.db`, whose one
//! writer orders them, so exactly one of them finds the job over.
//!
//! Each download job is also a job in the jobs list, kept in step after every write that
//! changes where one of its downloads is: its counts and whether it is paused while it runs,
//! and its files by name, outcome and error once it ended. A job whose every download was
//! removed ends canceled.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use api_types::jobs::{JobCounts, JobKind, JobPart, JobPartState, JobStarter, JobState};
use api_types::live::LiveFrame;
use api_types::transfers::{DownloadJobFinished, TransferStates};
use rusqlite::{Connection, OptionalExtension};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Downloads, Group};
use crate::jobs::{Ended, NewJob, NewPart};
use crate::sync::lock;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::listing::remote_directory;
use crate::transfers::{ASKED_ORDER, Record, text_at};

/// The jobs whose files are still being enqueued: none of them is over, whatever its
/// downloads do. A job is opened before any download of it runs, as none exists yet or, on a
/// resume, every one has ended, so no write ending one can come before it; it is closed and
/// read only inside writes of `transfers.db`, whose one writer orders them.
pub(super) type Open = Arc<Mutex<HashSet<Uuid>>>;

/// Whether the download `id` has a row that is not finished, read before a write ends it.
pub(super) fn unfinished(conn: &Connection, id: Uuid) -> rusqlite::Result<bool> {
  Ok(
    conn
      .prepare_cached(&format!("SELECT {UNFINISHED} FROM Transfers WHERE Id = ?1"))?
      .query_row([id.to_string()], |row| row.get::<_, bool>(0))
      .optional()?
      .unwrap_or(false),
  )
}

/// The job of `record` as it ended, when the write that just ended `record`, which was
/// `unfinished` before it, ended the job's last download.
pub(super) fn ended_by(
  conn: &Connection,
  record: &Record,
  unfinished: bool,
  open: &Open,
) -> rusqlite::Result<Option<DownloadJobFinished>> {
  match record.job_id {
    Some(job) if unfinished && record.state.contains(TransferStates::Completed) => {
      finished(conn, job, open)
    }
    _ => Ok(None),
  }
}

/// The job `job` as it ended, when it is not open and none of its downloads still listed is
/// unfinished; none for a job with no download listed.
fn finished(
  conn: &Connection,
  job: Uuid,
  open: &Open,
) -> rusqlite::Result<Option<DownloadJobFinished>> {
  if lock(open).contains(&job) {
    return Ok(None);
  }
  let pending: bool = conn
    .prepare_cached(&format!(
      "SELECT EXISTS (SELECT 1 FROM Transfers WHERE JobId = ?1 AND Removed = 0 AND {UNFINISHED})"
    ))?
    .query_row([job.to_string()], |row| row.get(0))?;
  if pending {
    return Ok(None);
  }
  let mut statement = conn.prepare_cached(&format!(
    "SELECT Username, Filename, State FROM Transfers WHERE JobId = ?1 AND Removed = 0 \
     ORDER BY {ASKED_ORDER}"
  ))?;
  let rows = statement
    .query_map([job.to_string()], |row| {
      Ok((
        text_at(row, 0)?,
        text_at(row, 1)?,
        TransferStates::from_bits_retain(row.get(2)?),
      ))
    })?
    .collect::<rusqlite::Result<Vec<_>>>()?;
  Ok(summary(job, &rows))
}

/// What the job `job` came to, from its downloads in the order they were asked for.
fn summary(job: Uuid, rows: &[(String, String, TransferStates)]) -> Option<DownloadJobFinished> {
  let (_, first, _) = rows.first()?;
  let mut usernames: Vec<String> = Vec::new();
  let (mut succeeded, mut failed, mut canceled) = (0, 0, 0);
  for (username, _, state) in rows {
    if !usernames.contains(username) {
      usernames.push(username.clone());
    }
    if *state == TransferStates::Completed | TransferStates::Succeeded {
      succeeded += 1;
    } else if state.contains(TransferStates::Cancelled) {
      canceled += 1;
    } else {
      failed += 1;
    }
  }
  let folder = if usernames.len() == 1 {
    common_folder(rows.iter().map(|(_, filename, _)| filename.as_str())).to_string()
  } else {
    String::new()
  };
  let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
  Some(DownloadJobFinished {
    id: job,
    usernames,
    filename: first.clone(),
    folder,
    file_count: count(rows.len()),
    succeeded_count: succeeded,
    failed_count: failed,
    canceled_count: canceled,
  })
}

/// The deepest remote folder holding every one of `filenames`; empty when they share none.
fn common_folder<'a>(mut filenames: impl Iterator<Item = &'a str>) -> &'a str {
  let Some(first) = filenames.next() else {
    return "";
  };
  let holds = |folder: &str, path: &str| {
    path
      .strip_prefix(folder)
      .is_some_and(|rest| rest.starts_with(['\\', '/']))
  };
  let mut common = remote_directory(first);
  for filename in filenames {
    while !common.is_empty() && !holds(common, filename) {
      common = remote_directory(common);
    }
  }
  common
}

/// The name of a download job of `groups`: its file or its folder from its user when it has one
/// user, else whom its files come from.
pub(super) fn job_name(groups: &[Group]) -> String {
  let usernames: Vec<&str> = groups.iter().map(|g| g.username.as_str()).collect();
  let files: Vec<&str> = groups
    .iter()
    .flat_map(|g| g.files.iter().map(|f| f.filename.as_str()))
    .collect();
  let mut users: Vec<&str> = Vec::new();
  for username in usernames {
    if !users.contains(&username) {
      users.push(username);
    }
  }
  let from = from_whom(&users);
  if let [only] = files.as_slice() {
    return format!("{} from {from}", last_segment(only));
  }
  let folder = common_folder(files.iter().copied());
  // a folder of one segment is the top of a share, which names nothing
  if users.len() == 1 && folder.contains(['\\', '/']) {
    return format!("{} from {from}", last_segment(folder));
  }
  format!("Downloads from {from}")
}

/// The last segment of a remote path.
fn last_segment(path: &str) -> &str {
  path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Whom files come from: `alice`, `alice and bob`, `alice and 2 others`.
fn from_whom(users: &[&str]) -> String {
  match users {
    [] => "nobody".to_string(),
    [one] => (*one).to_string(),
    [one, two] => format!("{one} and {two}"),
    [one, rest @ ..] => format!("{one} and {} others", rest.len()),
  }
}

/// The counts of the downloads of the job `job` still listed: every one, how each that ended
/// ended, and those paused.
fn counts(conn: &Connection, job: Uuid) -> rusqlite::Result<JobCounts> {
  conn
    .prepare_cached(&format!(
      "SELECT COUNT(*), \
       COALESCE(SUM(NOT {UNFINISHED} AND State = 48), 0), \
       COALESCE(SUM(NOT {UNFINISHED} AND State NOT IN (48, 80)), 0), \
       COALESCE(SUM(NOT {UNFINISHED} AND State = 80), 0), \
       COALESCE(SUM({UNFINISHED} AND Paused = 1), 0) \
       FROM Transfers WHERE JobId = ?1 AND Removed = 0"
    ))?
    .query_row([job.to_string()], |row| {
      let at =
        |i| -> rusqlite::Result<u32> { Ok(u32::try_from(row.get::<_, i64>(i)?).unwrap_or(0)) };
      Ok(JobCounts {
        total: at(0)?,
        succeeded: at(1)?,
        failed: at(2)?,
        canceled: at(3)?,
        paused: at(4)?,
      })
    })
}

/// A download as a part of its job.
#[derive(Clone, Debug)]
pub(super) struct Part {
  id: Uuid,
  username: String,
  filename: String,
  state: TransferStates,
  paused: bool,
  exception: Option<String>,
}

impl Part {
  fn state(&self) -> JobPartState {
    let finished = self.state.contains(TransferStates::Completed);
    if finished && self.state.contains(TransferStates::Succeeded) {
      JobPartState::Succeeded
    } else if finished && self.state.contains(TransferStates::Cancelled) {
      JobPartState::Canceled
    } else if finished {
      JobPartState::Failed
    } else if self.paused {
      JobPartState::Paused
    } else if self
      .state
      .intersects(TransferStates::InProgress | TransferStates::Initializing)
    {
      JobPartState::Running
    } else {
      JobPartState::Waiting
    }
  }

  /// The part as the job's history keeps it, named by its path under `folder`.
  fn kept(&self, folder: &str) -> NewPart {
    let state = self.state();
    NewPart {
      name: name_under(&self.filename, folder).to_string(),
      state,
      error: (state == JobPartState::Failed)
        .then(|| self.exception.clone())
        .flatten(),
      target: None,
      key: None,
    }
  }

  /// The part as a running job lists it, with its download.
  fn listed(&self, folder: &str) -> JobPart {
    let state = self.state();
    JobPart {
      name: name_under(&self.filename, folder).to_string(),
      state,
      error: (state == JobPartState::Failed)
        .then(|| self.exception.clone())
        .flatten(),
      transfer_id: Some(self.id),
      username: Some(self.username.clone()),
    }
  }
}

/// `filename` without the folder `folder` holding it.
fn name_under<'a>(filename: &'a str, folder: &str) -> &'a str {
  if folder.is_empty() {
    return filename;
  }
  filename
    .strip_prefix(folder)
    .map_or(filename, |rest| rest.trim_start_matches(['\\', '/']))
}

/// The downloads of the job `job` still listed, in the order they were asked for.
fn parts(conn: &Connection, job: Uuid) -> rusqlite::Result<Vec<Part>> {
  conn
    .prepare_cached(&format!(
      "SELECT Id, Username, Filename, State, Paused, Exception FROM Transfers \
       WHERE JobId = ?1 AND Removed = 0 ORDER BY {ASKED_ORDER}"
    ))?
    .query_map([job.to_string()], |row| {
      let id: String = row.get(0)?;
      Ok(Part {
        id: id.parse().map_err(|e: uuid::Error| {
          rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
        })?,
        username: text_at(row, 1)?,
        filename: text_at(row, 2)?,
        state: TransferStates::from_bits_retain(row.get(3)?),
        paused: row.get(4)?,
        exception: row.get(5)?,
      })
    })?
    .collect()
}

/// The folder every part of a job is under, when they come from one user.
fn parts_folder(parts: &[Part]) -> &str {
  let one_user = parts
    .windows(2)
    .all(|pair| pair[0].username == pair[1].username);
  if one_user && parts.len() > 1 {
    common_folder(parts.iter().map(|p| p.filename.as_str()))
  } else {
    ""
  }
}

impl Downloads {
  /// Enqueues `groups` as a new download job started by `starter`, listed in the jobs list;
  /// returns its id and what came of each group. With `filtered`, the files of a job of more
  /// than one file that match `transfers.download.filters` are skipped. A job of which no
  /// download was made ends failed with the groups' reasons.
  pub async fn enqueue_new(
    &self,
    starter: JobStarter,
    groups: Vec<Group>,
    filtered: bool,
  ) -> (
    Uuid,
    Vec<Result<super::Enqueued, crate::api::error::ApiError>>,
  ) {
    let job = Uuid::new_v4();
    let new = NewJob {
      id: job,
      kind: JobKind::Download,
      name: job_name(&groups),
      starter,
      state: JobState::Running,
      follows: None,
      data: None,
    };
    if let Err(e) = self.jobs.create(new).await {
      warn!(%job, error = %e, "download job not listed");
    }
    // a file asked for on its own is never filtered
    let files: usize = groups.iter().map(|g| g.files.len()).sum();
    let results = self.enqueue(job, groups, filtered && files > 1).await;
    let made = results.iter().any(|result| {
      result
        .as_ref()
        .is_ok_and(|enqueued| !enqueued.enqueued.is_empty())
    });
    if !made {
      let reasons: Vec<String> = results
        .iter()
        .map(|result| match result {
          Ok(enqueued) => enqueued
            .failed
            .iter()
            .map(|f| f.message.clone())
            .collect::<Vec<_>>()
            .join("; "),
          // the code of a refusal for want of the server is no sentence to show
          Err(e) if e.is_unavailable() => "Not connected to the Soulseek server".to_string(),
          Err(e) => e.message().to_string(),
        })
        .filter(|reason| !reason.is_empty())
        .collect();
      let ended = Ended {
        ending: Some(api_types::jobs::JobEnding::Failed),
        error: Some(if reasons.is_empty() {
          "Nothing was queued".to_string()
        } else {
          reasons.join("; ")
        }),
        ..Ended::default()
      };
      if let Err(e) = self.jobs.end(job, ended).await {
        warn!(%job, error = %e, "download job that queued nothing not ended");
      }
    }
    (job, results)
  }

  /// Makes the job `job` in the jobs list as started by a restart when it is not there, for
  /// downloads queued before the list kept them.
  pub(super) async fn ensure_listed(&self, job: Uuid, groups: &[Group]) {
    match self.jobs.get(job).await {
      Ok(Some(_)) => {}
      Ok(None) => {
        let new = NewJob {
          id: job,
          kind: JobKind::Download,
          name: job_name(groups),
          starter: JobStarter::of(api_types::jobs::JobStarterKind::Resumed),
          state: JobState::Running,
          follows: None,
          data: None,
        };
        if let Err(e) = self.jobs.create(new).await {
          warn!(%job, error = %e, "resumed download job not listed");
        }
      }
      Err(e) => warn!(%job, error = %e, "resumed download job not read"),
    }
  }

  /// Brings the job `job` in the jobs list in step with its downloads: ended with its files
  /// once every one of them ended and it is not being enqueued, canceled when every one was
  /// removed, else its counts and whether it is paused. A job that never had a download is left
  /// as it is, for the caller to end with its reason; returns whether it was one.
  pub(super) async fn sync_job(&self, job: Uuid) -> bool {
    let open = lock(&self.open_jobs).contains(&job);
    let read = self
      .db
      .read(move |conn| Ok((counts(conn, job)?, parts(conn, job)?)))
      .await;
    let (counts, rows) = match read {
      Ok(read) => read,
      Err(e) => {
        warn!(%job, error = %e, "download job not read for the jobs list");
        return false;
      }
    };
    let ended = counts.ended();
    if counts.total == 0 && !open {
      // a job whose every download was removed ends canceled; one that never had any is ended
      // by its caller, which knows why
      match self.jobs.get(job).await {
        Ok(Some(listed)) if listed.counts.total == 0 => return true,
        Ok(_) => {}
        Err(e) => {
          warn!(%job, error = %e, "download job not read from the jobs list");
          return false;
        }
      }
    }
    let result = if !open && (counts.total == 0 || ended == counts.total) {
      let folder = parts_folder(&rows).to_string();
      let end = Ended {
        ending: (counts.total == 0).then_some(api_types::jobs::JobEnding::Canceled),
        counts: Some(counts),
        parts: Some(rows.iter().map(|part| part.kept(&folder)).collect()),
        ..Ended::default()
      };
      match self.jobs.end(job, end).await {
        Ok(Some(_)) => {
          self.analysis.download_job_ended(job).await;
          Ok(())
        }
        Ok(None) => Ok(()),
        Err(e) => Err(e),
      }
    } else {
      let open_files = counts.total.saturating_sub(ended);
      let state = if open_files > 0 && counts.paused == open_files {
        JobState::Paused
      } else {
        JobState::Running
      };
      self.jobs.set_counts(job, counts, state).await.map(drop)
    };
    if let Err(e) = result {
      warn!(%job, error = %e, "download job not brought in step in the jobs list");
    }
    false
  }

  /// Ends the job `job`, which never had a download and is not being enqueued, as `ending`
  /// with `error`.
  pub(super) async fn end_empty(
    &self,
    job: Uuid,
    ending: api_types::jobs::JobEnding,
    error: Option<String>,
  ) {
    if lock(&self.open_jobs).contains(&job) {
      return;
    }
    let ended = Ended {
      ending: Some(ending),
      error,
      counts: Some(JobCounts::default()),
      ..Ended::default()
    };
    if let Err(e) = self.jobs.end(job, ended).await {
      warn!(%job, error = %e, "download job with no download not ended");
    }
  }

  /// The downloads of the job `job` as its parts.
  pub async fn job_parts(&self, job: Uuid) -> Result<Vec<JobPart>, crate::api::error::ApiError> {
    let rows = self
      .db
      .read(move |conn| parts(conn, job))
      .await
      .map_err(crate::api::error::ApiError::internal)?;
    let folder = parts_folder(&rows);
    Ok(rows.iter().map(|part| part.listed(folder)).collect())
  }

  /// The downloads of the job `job` that are not finished, with their users.
  async fn unfinished_of(&self, job: Uuid) -> Result<Vec<Part>, crate::api::error::ApiError> {
    let rows = self
      .db
      .read(move |conn| parts(conn, job))
      .await
      .map_err(crate::api::error::ApiError::internal)?;
    Ok(
      rows
        .into_iter()
        .filter(|part| !part.state.contains(TransferStates::Completed))
        .collect(),
    )
  }

  /// Cancels every download of the job `job` that has not finished; a job with none to
  /// cancel is brought in step, and one that never had a download ends canceled.
  pub async fn cancel_job(&self, job: Uuid) -> Result<(), crate::api::error::ApiError> {
    let unfinished = self.unfinished_of(job).await?;
    for part in &unfinished {
      self.cancel(&part.username, part.id, false).await?;
    }
    if unfinished.is_empty() && self.sync_job(job).await {
      self
        .end_empty(job, api_types::jobs::JobEnding::Canceled, None)
        .await;
    }
    info!(%job, downloads = unfinished.len(), "download job canceled");
    Ok(())
  }

  /// Pauses every download of the job `job` that has not finished.
  pub async fn pause_job(&self, job: Uuid) -> Result<(), crate::api::error::ApiError> {
    let ids = self
      .unfinished_of(job)
      .await?
      .into_iter()
      .filter(|part| !part.paused)
      .map(|part| part.id)
      .collect();
    let outcomes = self.pause(ids).await;
    let changed = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    let failed: Vec<_> = outcomes.into_iter().filter_map(Result::err).collect();
    if changed == 0 {
      debug!(%job, failed = failed.len(), "no download of the job paused");
    } else {
      info!(%job, downloads = changed, failed = failed.len(), "download job paused");
    }
    if let Some(first) = failed.first() {
      debug!(%job, error = first.error, "first download of the job not paused");
    }
    Ok(())
  }

  /// Resumes every paused download of the job `job`.
  pub async fn resume_job(&self, job: Uuid) -> Result<(), crate::api::error::ApiError> {
    let ids = self
      .unfinished_of(job)
      .await?
      .into_iter()
      .filter(|part| part.paused)
      .map(|part| part.id)
      .collect();
    let outcomes = self.resume_paused(ids).await;
    let changed = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    let failed: Vec<_> = outcomes.into_iter().filter_map(Result::err).collect();
    if changed == 0 {
      debug!(%job, failed = failed.len(), "no download of the job resumed");
    } else {
      info!(%job, downloads = changed, failed = failed.len(), "download job resumed");
    }
    if let Some(first) = failed.first() {
      debug!(%job, error = first.error, "first download of the job not resumed");
    }
    Ok(())
  }

  /// Opens the job `job` while its files are being enqueued.
  pub(super) fn open_job(&self, job: Uuid) {
    lock(&self.open_jobs).insert(job);
  }

  /// Closes the job `job` once its files are enqueued, and tells of it when every download it
  /// has ended meanwhile.
  pub(super) async fn close_job(&self, job: Uuid) {
    let open = self.open_jobs.clone();
    let finished = self
      .db
      .write(move |conn| {
        lock(&open).remove(&job);
        finished(conn, job, &open)
      })
      .await;
    match finished {
      Ok(finished) => self.tell(finished),
      Err(e) => warn!(%job, error = %e, "download job not checked once enqueued"),
    }
    // a job that never had a download is ended by `enqueue_new`, which knows why
    self.sync_job(job).await;
  }

  /// Tells of a job that ended, when one did.
  pub(super) fn tell(&self, finished: Option<DownloadJobFinished>) {
    let Some(job) = finished else {
      return;
    };
    info!(
      job = %job.id,
      files = job.file_count,
      succeeded = job.succeeded_count,
      failed = job.failed_count,
      canceled = job.canceled_count,
      "download job finished"
    );
    self.live.publish(LiveFrame::TransfersJobFinished(job));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::transfers;
  use crate::transfers::store;

  const DONE: TransferStates = TransferStates::Completed.union(TransferStates::Succeeded);
  const FAILED: TransferStates = TransferStates::Completed.union(TransferStates::Rejected);
  const CANCELED: TransferStates = TransferStates::Completed.union(TransferStates::Cancelled);

  fn queued(job: Uuid, username: &str, filename: &str) -> Record {
    let mut record = Record::download(username, filename, 1);
    record.job_id = Some(job);
    record
  }

  /// Ends `record` in `state` as the engine's write does, and returns the job it ended.
  fn end(
    conn: &Connection,
    record: &mut Record,
    state: TransferStates,
    open: &Open,
  ) -> Option<DownloadJobFinished> {
    let was = unfinished(conn, record.id).unwrap();
    record.state = state;
    record.ended_at = Some(chrono::Utc::now());
    store::save(conn, record).unwrap();
    ended_by(conn, record, was, open).unwrap()
  }

  #[tokio::test]
  async fn a_job_is_told_once_by_whichever_download_ends_last() {
    let db = transfers().await;
    db.write(|conn| {
      let open = Open::default();
      let job = Uuid::new_v4();
      // files asked for together share the instant, and their ids sort against the order
      let asked = chrono::Utc::now();
      let mut records: Vec<Record> = (1..=3u128)
        .map(|n| Record {
          id: Uuid::from_u128(10 - n),
          requested_at: asked,
          ..queued(job, "alice", &format!("@@a\\Album\\0{n}.flac"))
        })
        .collect();
      for record in &records {
        store::insert(conn, record)?;
      }
      let other = queued(Uuid::new_v4(), "alice", "@@a\\Other\\01.flac");
      store::insert(conn, &other)?;
      let (a, rest) = records.split_first_mut().unwrap();
      let (b, rest) = rest.split_first_mut().unwrap();
      let c = &mut rest[0];
      assert_eq!(end(conn, c, DONE, &open), None);
      assert_eq!(end(conn, a, FAILED, &open), None);
      let told = end(conn, b, CANCELED, &open).expect("the last download ends the job");
      assert_eq!(told.id, job);
      assert_eq!(told.usernames, ["alice"]);
      assert_eq!(told.filename, "@@a\\Album\\01.flac");
      assert_eq!(told.folder, "@@a\\Album");
      assert_eq!(
        (
          told.file_count,
          told.succeeded_count,
          told.failed_count,
          told.canceled_count
        ),
        (3, 1, 1, 1)
      );
      assert_eq!(
        end(conn, b, DONE, &open),
        None,
        "a write of a download that had ended tells nothing again"
      );
      Ok(())
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn a_job_of_one_file_is_told_and_removed_downloads_are_left_out() {
    let db = transfers().await;
    db.write(|conn| {
      let open = Open::default();
      let job = Uuid::new_v4();
      let mut lone = queued(job, "alice", "@@a\\Song.flac");
      store::insert(conn, &lone)?;
      let told = end(conn, &mut lone, DONE, &open).expect("a lone download is a job");
      assert_eq!((told.file_count, told.succeeded_count), (1, 1));
      assert_eq!(told.folder, "@@a");

      let job = Uuid::new_v4();
      let mut kept = queued(job, "alice", "@@a\\B\\1.flac");
      let mut gone = queued(job, "bob", "@@b\\B\\2.flac");
      store::insert(conn, &kept)?;
      store::insert(conn, &gone)?;
      gone.removed = true;
      assert_eq!(
        end(conn, &mut gone, CANCELED, &open),
        None,
        "the other download still runs"
      );
      let told = end(conn, &mut kept, DONE, &open).unwrap();
      assert_eq!(told.file_count, 1, "a removed download is not counted");
      assert_eq!(told.usernames, ["alice"]);

      let job = Uuid::new_v4();
      let mut only = queued(job, "alice", "@@a\\C\\1.flac");
      store::insert(conn, &only)?;
      only.removed = true;
      assert_eq!(
        end(conn, &mut only, CANCELED, &open),
        None,
        "a job with nothing listed is not told of"
      );
      Ok(())
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn an_open_job_is_told_by_its_close_when_its_downloads_ended_first() {
    let db = transfers().await;
    db.write(|conn| {
      let open = Open::default();
      let job = Uuid::new_v4();
      lock(&open).insert(job);
      let mut first = queued(job, "alice", "@@a\\Disc 1\\1.flac");
      store::insert(conn, &first)?;
      assert_eq!(
        end(conn, &mut first, DONE, &open),
        None,
        "more files of the job are still to come"
      );
      let mut second = queued(job, "bob", "@@b\\Disc 2\\1.flac");
      store::insert(conn, &second)?;
      lock(&open).remove(&job);
      let told = end(conn, &mut second, DONE, &open).unwrap();
      assert_eq!(told.usernames, ["alice", "bob"]);
      assert_eq!(told.folder, "", "files of several users share no folder");

      let job = Uuid::new_v4();
      lock(&open).insert(job);
      let mut early = queued(job, "alice", "@@a\\x.flac");
      store::insert(conn, &early)?;
      assert_eq!(end(conn, &mut early, FAILED, &open), None);
      lock(&open).remove(&job);
      let told = finished(conn, job, &open)?.expect("the close finds it over");
      assert_eq!(told.failed_count, 1);
      Ok(())
    })
    .await
    .unwrap();
  }

  #[test]
  fn a_download_job_is_named_by_its_file_its_folder_or_its_users() {
    let group = |username: &str, files: &[&str]| Group {
      username: username.to_string(),
      files: files
        .iter()
        .map(|f| super::super::FileRequest {
          filename: (*f).to_string(),
          size: 1,
        })
        .collect(),
      destination: None,
      search_id: None,
    };
    assert_eq!(
      job_name(&[group("alice", &["@@a\\Music\\Song.flac"])]),
      "Song.flac from alice"
    );
    assert_eq!(
      job_name(&[group(
        "alice",
        &["@@a\\M\\Album\\1.flac", "@@a\\M\\Album\\2.flac"]
      )]),
      "Album from alice"
    );
    assert_eq!(
      job_name(&[group("alice", &["@@a\\1.flac", "@@a\\2.flac"])]),
      "Downloads from alice",
      "the top of a share names nothing"
    );
    assert_eq!(
      job_name(&[
        group("alice", &["@@a\\A\\1.flac"]),
        group("bob", &["@@b\\A\\1.flac"]),
        group("carol", &["@@c\\A\\1.flac"])
      ]),
      "Downloads from alice and 2 others"
    );
  }

  #[test]
  fn the_common_folder_is_the_deepest_holding_every_file() {
    let folder = |names: &[&'static str]| common_folder(names.iter().copied());
    assert_eq!(
      folder(&[
        "@@a\\M\\Album\\Disc 1\\1.flac",
        "@@a\\M\\Album\\Disc 2\\1.flac"
      ]),
      "@@a\\M\\Album"
    );
    assert_eq!(
      folder(&["@@a\\M\\Album\\1.flac", "@@a\\M\\Albums\\1.flac"]),
      "@@a\\M",
      "a folder's name is not a prefix of another's"
    );
    assert_eq!(folder(&["a/b/c.mp3", "a/b/d/e.mp3"]), "a/b");
    assert_eq!(folder(&["one.flac", "@@a\\two.flac"]), "");
  }
}
