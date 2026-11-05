//! Download jobs: the downloads queued in one go, such as a file, a folder, an album whose
//! discs go to folders of their own, or files from several users picked from one search. A job
//! is open while its files are being enqueued. Once it is closed, the write that ends its last
//! download tells of it with a `transfers.job_finished` frame, or the close does when every
//! download ended before it. Each check runs inside a write of `transfers.db`, whose one
//! writer orders them, so exactly one of them finds the job over.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use api_types::live::LiveFrame;
use api_types::transfers::{DownloadJobFinished, TransferStates};
use rusqlite::{Connection, OptionalExtension};
use tracing::{info, warn};
use uuid::Uuid;

use super::Downloads;
use crate::sync::lock;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::listing::remote_directory;
use crate::transfers::{Record, text_at};

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
  let mut statement = conn.prepare_cached(
    "SELECT Username, Filename, State FROM Transfers WHERE JobId = ?1 AND Removed = 0 \
     ORDER BY RequestedAt, Id",
  )?;
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

impl Downloads {
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
      let mut records: Vec<Record> = (1..=3)
        .map(|n| queued(job, "alice", &format!("@@a\\Album\\0{n}.flac")))
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
