//! `jobs.db`: each job's row, the parts kept for it, and the last run of each schedule. Kinds,
//! states and endings are stored as the API spells them.

use api_types::jobs::{
  Job, JobCounts, JobEnding, JobKind, JobPart, JobPartState, JobStarter, JobStarterKind, JobState,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::db::{self, Schema, read_timestamp, timestamp};

/// `jobs.db`: jobs, their parts and the schedules' last runs.
pub static SCHEMA: Schema = db::schema!("jobs", ["0001_initial"]);

/// The columns [`job`] reads, in its order.
const COLUMNS: &str = "id, kind, name, starter, starter_name, state, ending, error, summary, \
                       created_at, started_at, ended_at, total, succeeded, failed, canceled, \
                       paused, follows";

/// A value of an API enum as it is spelled, which is how it is stored.
pub fn text<T: Serialize>(value: T) -> String {
  match serde_json::to_value(value) {
    Ok(serde_json::Value::String(text)) => text,
    other => unreachable!("an API enum is written as a string, not {other:?}"),
  }
}

/// A stored API enum read back.
fn read<T: DeserializeOwned>(row: &Row, at: usize) -> rusqlite::Result<T> {
  let text: String = row.get(at)?;
  serde_json::from_value(serde_json::Value::String(text)).map_err(|e| {
    rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into())
  })
}

/// An id stored as text, read back.
fn id_at(row: &Row, at: usize) -> rusqlite::Result<Uuid> {
  let text: String = row.get(at)?;
  text.parse().map_err(|e: uuid::Error| {
    rusqlite::Error::FromSqlConversionFailure(at, rusqlite::types::Type::Text, e.into())
  })
}

/// An id that may be missing, stored as text, read back.
fn optional_id_at(row: &Row, at: usize) -> rusqlite::Result<Option<Uuid>> {
  match row.get::<_, Option<String>>(at)? {
    Some(_) => id_at(row, at).map(Some),
    None => Ok(None),
  }
}

fn time_at(row: &Row, at: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
  row
    .get::<_, Option<String>>(at)?
    .map(|text| read_timestamp(&text))
    .transpose()
}

/// Whether a job of `kind` in `state` can be paused or is paused.
pub fn pausable(kind: JobKind, state: JobState) -> bool {
  matches!(kind, JobKind::Download | JobKind::Analysis) && state != JobState::Ended
}

/// A job's row.
fn job(row: &Row) -> rusqlite::Result<Job> {
  let kind: JobKind = read(row, 1)?;
  let state: JobState = read(row, 5)?;
  Ok(Job {
    id: id_at(row, 0)?,
    kind,
    name: row.get(2)?,
    starter: JobStarter {
      kind: read::<JobStarterKind>(row, 3)?,
      name: row.get(4)?,
    },
    state,
    ending: row
      .get::<_, Option<String>>(6)?
      .map(|_| read::<JobEnding>(row, 6))
      .transpose()?,
    error: row.get(7)?,
    summary: row.get(8)?,
    created_at: read_timestamp(&row.get::<_, String>(9)?)?,
    started_at: time_at(row, 10)?,
    ended_at: time_at(row, 11)?,
    counts: JobCounts {
      total: row.get(12)?,
      succeeded: row.get(13)?,
      failed: row.get(14)?,
      canceled: row.get(15)?,
      paused: row.get(16)?,
    },
    progress: None,
    follows: optional_id_at(row, 17)?,
    pausable: pausable(kind, state),
  })
}

/// Writes a new job with `data`, what a run again repeats.
pub fn insert(
  conn: &Connection,
  job: &Job,
  data: Option<&serde_json::Value>,
) -> rusqlite::Result<()> {
  conn.execute(
    &format!(
      "INSERT INTO jobs ({COLUMNS}, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, \
              ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)"
    ),
    params![
      job.id.to_string(),
      text(job.kind),
      job.name,
      text(job.starter.kind),
      job.starter.name,
      text(job.state),
      job.ending.map(text),
      job.error,
      job.summary,
      timestamp(job.created_at),
      job.started_at.map(timestamp),
      job.ended_at.map(timestamp),
      job.counts.total,
      job.counts.succeeded,
      job.counts.failed,
      job.counts.canceled,
      job.counts.paused,
      job.follows.map(|id| id.to_string()),
      data.map(serde_json::Value::to_string),
    ],
  )?;
  Ok(())
}

/// Writes what can change of a job: its name, state, ending, words, counts and times.
pub fn update(conn: &Connection, job: &Job) -> rusqlite::Result<bool> {
  let changed = conn.execute(
    "UPDATE jobs SET name = ?2, state = ?3, ending = ?4, error = ?5, summary = ?6, \
     started_at = ?7, ended_at = ?8, total = ?9, succeeded = ?10, failed = ?11, canceled = ?12, \
     paused = ?13 WHERE id = ?1",
    params![
      job.id.to_string(),
      job.name,
      text(job.state),
      job.ending.map(text),
      job.error,
      job.summary,
      job.started_at.map(timestamp),
      job.ended_at.map(timestamp),
      job.counts.total,
      job.counts.succeeded,
      job.counts.failed,
      job.counts.canceled,
      job.counts.paused,
    ],
  )?;
  Ok(changed > 0)
}

/// The job `id`, none when there is none.
pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Job>> {
  conn
    .prepare_cached(&format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"))?
    .query_row([id.to_string()], job)
    .optional()
}

/// What a run again of the job `id` repeats, as it was stored.
pub fn data(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<serde_json::Value>> {
  let text: Option<String> = conn
    .query_row(
      "SELECT data FROM jobs WHERE id = ?1",
      [id.to_string()],
      |row| row.get(0),
    )
    .optional()?
    .flatten();
  text
    .map(|text| {
      serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
      })
    })
    .transpose()
}

/// Every job: running and paused first, then queued in the order they were made, then the
/// ended ones newest first.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Job>> {
  conn
    .prepare(&format!(
      "SELECT {COLUMNS} FROM jobs ORDER BY \
       CASE state WHEN 'running' THEN 0 WHEN 'paused' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END, \
       CASE WHEN state = 'ended' THEN ended_at END DESC, created_at, id"
    ))?
    .query_map([], job)?
    .collect()
}

/// The jobs not ended, of `kind` when one is given.
pub fn unended(conn: &Connection, kind: Option<JobKind>) -> rusqlite::Result<Vec<Job>> {
  conn
    .prepare(&format!(
      "SELECT {COLUMNS} FROM jobs WHERE state <> 'ended' AND (?1 IS NULL OR kind = ?1) \
       ORDER BY created_at, id"
    ))?
    .query_map([kind.map(text)], job)?
    .collect()
}

/// A part to write: its name, state and error, and what it works on.
#[derive(Clone, Debug, PartialEq)]
pub struct NewPart {
  /// what it works on
  pub name: String,
  /// where it is
  pub state: JobPartState,
  /// why it failed
  pub error: Option<String>,
  /// what it works on, as its kind reads it
  pub target: Option<String>,
  /// what finds it among the parts of every job, such as an analyzed file's local path
  pub key: Option<String>,
}

/// A part as it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredPart {
  /// its id
  pub id: Uuid,
  /// the job it is part of
  pub job: Uuid,
  /// its name, state and error
  pub part: JobPart,
  /// what it works on
  pub target: Option<String>,
}

fn part(row: &Row) -> rusqlite::Result<StoredPart> {
  Ok(StoredPart {
    id: id_at(row, 0)?,
    job: id_at(row, 1)?,
    part: JobPart {
      name: row.get(2)?,
      state: read(row, 3)?,
      error: row.get(4)?,
      transfer_id: None,
      username: None,
    },
    target: row.get(5)?,
  })
}

/// Adds `parts` after the parts the job `job` has, returning their ids.
pub fn add_parts(conn: &Connection, job: Uuid, parts: &[NewPart]) -> rusqlite::Result<Vec<Uuid>> {
  let next: i64 = conn.query_row(
    "SELECT COALESCE(MAX(position) + 1, 0) FROM job_parts WHERE job_id = ?1",
    [job.to_string()],
    |row| row.get(0),
  )?;
  let mut insert = conn.prepare_cached(
    "INSERT INTO job_parts (id, job_id, position, name, state, error, target, key) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
  )?;
  let mut ids = Vec::with_capacity(parts.len());
  for (position, new) in (next..).zip(parts) {
    let id = Uuid::new_v4();
    insert.execute(params![
      id.to_string(),
      job.to_string(),
      position,
      new.name,
      text(new.state),
      new.error,
      new.target,
      new.key,
    ])?;
    ids.push(id);
  }
  Ok(ids)
}

/// The parts of the job `job`, in their order.
pub fn parts(conn: &Connection, job: Uuid) -> rusqlite::Result<Vec<StoredPart>> {
  conn
    .prepare_cached(
      "SELECT id, job_id, name, state, error, target FROM job_parts WHERE job_id = ?1 \
       ORDER BY position",
    )?
    .query_map([job.to_string()], part)?
    .collect()
}

/// The part `id`, none when there is none.
pub fn part_by_id(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<StoredPart>> {
  conn
    .prepare_cached("SELECT id, job_id, name, state, error, target FROM job_parts WHERE id = ?1")?
    .query_row([id.to_string()], part)
    .optional()
}

/// The parts in `state` of the jobs in `job_states`, the oldest job first and in each job's
/// order, at most `limit` of them.
pub fn parts_in(
  conn: &Connection,
  state: JobPartState,
  job_states: &[JobState],
  limit: usize,
) -> rusqlite::Result<Vec<StoredPart>> {
  let states: Vec<String> = job_states
    .iter()
    .map(|s| format!("'{}'", text(*s)))
    .collect();
  conn
    .prepare_cached(&format!(
      "SELECT p.id, p.job_id, p.name, p.state, p.error, p.target FROM job_parts p \
       JOIN jobs j ON j.id = p.job_id WHERE p.state = ?1 AND j.state IN ({}) \
       ORDER BY j.created_at, j.id, p.position LIMIT ?2",
      states.join(", ")
    ))?
    .query_map(
      params![text(state), i64::try_from(limit).unwrap_or(i64::MAX)],
      part,
    )?
    .collect()
}

/// Sets the state and error of the part `id`; false when there is no such part.
pub fn set_part(
  conn: &Connection,
  id: Uuid,
  state: JobPartState,
  error: Option<&str>,
) -> rusqlite::Result<bool> {
  Ok(
    conn.execute(
      "UPDATE job_parts SET state = ?2, error = ?3 WHERE id = ?1",
      params![id.to_string(), text(state), error],
    )? > 0,
  )
}

/// Sets every part of the job `job` in `from` to `to`, returning how many.
pub fn set_parts(
  conn: &Connection,
  job: Uuid,
  from: JobPartState,
  to: JobPartState,
) -> rusqlite::Result<usize> {
  conn.execute(
    "UPDATE job_parts SET state = ?3 WHERE job_id = ?1 AND state = ?2",
    params![job.to_string(), text(from), text(to)],
  )
}

/// Replaces the parts of the job `job` with `parts`.
pub fn replace_parts(conn: &Connection, job: Uuid, parts: &[NewPart]) -> rusqlite::Result<()> {
  conn.execute("DELETE FROM job_parts WHERE job_id = ?1", [job.to_string()])?;
  add_parts(conn, job, parts)?;
  Ok(())
}

/// The counts of the parts of the job `job` as stored.
pub fn counts(conn: &Connection, job: Uuid) -> rusqlite::Result<JobCounts> {
  conn
    .prepare_cached(
      "SELECT COUNT(*), \
       COALESCE(SUM(state = 'succeeded'), 0), COALESCE(SUM(state = 'failed'), 0), \
       COALESCE(SUM(state = 'canceled'), 0), COALESCE(SUM(state = 'paused'), 0) \
       FROM job_parts WHERE job_id = ?1",
    )?
    .query_row([job.to_string()], |row| {
      Ok(JobCounts {
        total: row.get(0)?,
        succeeded: row.get(1)?,
        failed: row.get(2)?,
        canceled: row.get(3)?,
        paused: row.get(4)?,
      })
    })
}

/// How many parts of the job `job` wait and how many run.
pub fn open_parts(conn: &Connection, job: Uuid) -> rusqlite::Result<(u32, u32)> {
  conn
    .prepare_cached(
      "SELECT COALESCE(SUM(state = 'waiting'), 0), COALESCE(SUM(state = 'running'), 0) \
       FROM job_parts WHERE job_id = ?1",
    )?
    .query_row([job.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))
}

/// Deletes the jobs `ids` with their parts, returning those that were there.
pub fn remove(conn: &Connection, ids: &[Uuid]) -> rusqlite::Result<Vec<Uuid>> {
  let mut removed = Vec::new();
  for id in ids {
    conn.execute("DELETE FROM job_parts WHERE job_id = ?1", [id.to_string()])?;
    if conn.execute("DELETE FROM jobs WHERE id = ?1", [id.to_string()])? > 0 {
      removed.push(*id);
    }
  }
  Ok(removed)
}

/// The ended jobs that ended before `before`, and those past the newest `keep` ended ones;
/// `None` keeps every one for that limit.
pub fn expired(
  conn: &Connection,
  before: Option<DateTime<Utc>>,
  keep: Option<u32>,
) -> rusqlite::Result<Vec<Uuid>> {
  // a negative limit is no limit
  let keep = keep.map_or(-1, i64::from);
  let mut statement = conn.prepare(
    "SELECT id FROM jobs WHERE state = 'ended' AND (\
       (?1 IS NOT NULL AND ended_at < ?1) OR \
       (?2 >= 0 AND id NOT IN (SELECT id FROM jobs WHERE state = 'ended' \
        ORDER BY ended_at DESC, id LIMIT ?2)))",
  )?;
  statement
    .query_map(params![before.map(timestamp), keep], |row| id_at(row, 0))?
    .collect()
}

/// The ended jobs.
pub fn ended(conn: &Connection) -> rusqlite::Result<Vec<Uuid>> {
  conn
    .prepare("SELECT id FROM jobs WHERE state = 'ended'")?
    .query_map([], |row| id_at(row, 0))?
    .collect()
}

/// How many jobs run or wait, how many are paused, and when the last failure ended.
pub fn summary(conn: &Connection) -> rusqlite::Result<api_types::jobs::JobsSummary> {
  conn.query_row(
    "SELECT COALESCE(SUM(state IN ('running', 'queued')), 0), \
     COALESCE(SUM(state = 'paused'), 0), \
     MAX(CASE WHEN ending IN ('failed', 'finished_with_failures') THEN ended_at END) FROM jobs",
    [],
    |row| {
      Ok(api_types::jobs::JobsSummary {
        running: row.get(0)?,
        paused: row.get(1)?,
        last_failed_at: time_at(row, 2)?,
      })
    },
  )
}

/// The last run of a schedule as stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastRun {
  /// the last run the schedule itself started
  pub scheduled_at: Option<DateTime<Utc>>,
  /// the last run of any starter
  pub run_at: Option<DateTime<Utc>>,
  /// how it ended
  pub ending: Option<JobEnding>,
  /// why it failed
  pub error: Option<String>,
  /// what it came to
  pub summary: Option<String>,
  /// its job, when it is in the list
  pub job_id: Option<Uuid>,
}

/// A row of `schedules`, from its second column on.
fn last_run_row(row: &Row) -> rusqlite::Result<LastRun> {
  Ok(LastRun {
    scheduled_at: time_at(row, 1)?,
    run_at: time_at(row, 2)?,
    ending: row
      .get::<_, Option<String>>(3)?
      .map(|_| read::<JobEnding>(row, 3))
      .transpose()?,
    error: row.get(4)?,
    summary: row.get(5)?,
    job_id: optional_id_at(row, 6)?,
  })
}

/// The last run of every schedule that ran, by name.
pub fn last_runs(conn: &Connection) -> rusqlite::Result<Vec<(String, LastRun)>> {
  conn
    .prepare("SELECT name, scheduled_at, run_at, ending, error, summary, job_id FROM schedules")?
    .query_map([], |row| Ok((row.get(0)?, last_run_row(row)?)))?
    .collect()
}

/// The last run of the schedule `name`, none when it never ran.
pub fn last_run(conn: &Connection, name: &str) -> rusqlite::Result<Option<LastRun>> {
  conn
    .query_row(
      "SELECT name, scheduled_at, run_at, ending, error, summary, job_id FROM schedules \
       WHERE name = ?1",
      [name],
      last_run_row,
    )
    .optional()
}

/// Writes the last run of the schedule `name`.
pub fn set_last_run(conn: &Connection, name: &str, last: &LastRun) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO schedules (name, scheduled_at, run_at, ending, error, summary, job_id) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(name) DO UPDATE SET \
     scheduled_at = excluded.scheduled_at, run_at = excluded.run_at, ending = excluded.ending, \
     error = excluded.error, summary = excluded.summary, job_id = excluded.job_id",
    params![
      name,
      last.scheduled_at.map(timestamp),
      last.run_at.map(timestamp),
      last.ending.map(text),
      last.error,
      last.summary,
      last.job_id.map(|id| id.to_string()),
    ],
  )?;
  Ok(())
}
