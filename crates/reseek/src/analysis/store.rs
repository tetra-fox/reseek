//! `analysis.db`: the newest report of each file, the user's judgement of each file, and the
//! spectrogram images kept on disk with when each was last viewed.

use api_types::analysis::{AnalysisListQuery, AnalysisOverride, AudioAnalysis, Judgement};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::db::{self, Database, DbError, Schema};

/// The analysis database's migrations.
pub static SCHEMA: Schema = db::schema!("analysis", ["0001_initial"]);

/// A report as it is stored.
#[derive(Clone, Debug)]
pub(super) struct Stored {
  /// the report, with its windows and the judgement set now
  pub report: AudioAnalysis,
  /// the size the file had when analysed
  pub size: u64,
  /// when the file had last changed when analysed, as stored
  pub modified: String,
  /// the name the spectrogram images are kept under
  pub image: String,
}

/// A report to store.
pub(super) struct NewReport<'a> {
  pub report: &'a AudioAnalysis,
  pub root: &'a str,
  pub relative: &'a str,
  pub modified: &'a str,
  pub transfer_id: Option<Uuid>,
  pub image: &'a str,
}

/// A value of one of the API's enums as its JSON spells it, which the database stores.
fn spelled(value: &impl serde::Serialize) -> String {
  serde_plain::to_string(value).expect("the analysis enums are plain names")
}

/// The report of a row read with [`JUDGED`]: its JSON in column 0, and the judgement beside
/// it in columns 4 to 7, which are null when there is none.
fn judged_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<AudioAnalysis> {
  let json: String = row.get(0)?;
  let mut report: AudioAnalysis = serde_json::from_str(&json).map_err(|e| {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
  })?;
  let verdict: Option<String> = row.get(4)?;
  report.judgement = match verdict {
    Some(verdict) => Some(AnalysisOverride {
      verdict: serde_plain::from_str(&verdict).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, e.into())
      })?,
      note: row.get(5)?,
      by: row.get(6)?,
      at: db::read_timestamp(&row.get::<_, String>(7)?)?,
    }),
    None => None,
  };
  Ok(report)
}

const JUDGED: &str = "SELECT r.report, r.size, r.modified, r.image, j.verdict, j.note, j.by, j.at \
                      FROM reports r LEFT JOIN judgements j ON j.path = r.path";

/// Stores `new` as the newest report of its file.
pub(super) async fn save(db: &Database, new: NewReport<'_>) -> Result<(), DbError> {
  // a report is strings, numbers and lists, which always serialise
  let json = serde_json::to_string(&AudioAnalysis {
    judgement: None,
    ..new.report.clone()
  })
  .expect("a report serialises");
  let row = (
    new.report.path.clone(),
    new.root.to_string(),
    new.relative.to_string(),
    i64::try_from(new.report.size).unwrap_or(i64::MAX),
    new.modified.to_string(),
    new.transfer_id.map(|id| id.to_string()),
    db::timestamp(new.report.analysed_at),
    spelled(&new.report.verdict),
    new.image.to_string(),
    json,
  );
  db.write(move |conn| {
    conn.execute(
      "INSERT OR REPLACE INTO reports \
       (path, root, relative, size, modified, transfer_id, analysed_at, verdict, image, report) \
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
      params![
        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9
      ],
    )?;
    Ok(())
  })
  .await
}

/// The newest report of the file at `path`.
pub(super) async fn find(db: &Database, path: &str) -> Result<Option<Stored>, DbError> {
  let path = path.to_string();
  db.read(move |conn| {
    conn
      .query_row(&format!("{JUDGED} WHERE r.path = ?1"), [path], |row| {
        Ok(Stored {
          report: judged_report(row)?,
          // written from a u64, so never negative
          size: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
          modified: row.get(2)?,
          image: row.get(3)?,
        })
      })
      .optional()
  })
  .await
}

/// The reports `query` asks for, newest first, without their windows.
pub(super) async fn list(
  db: &Database,
  query: &AnalysisListQuery,
) -> Result<Vec<AudioAnalysis>, DbError> {
  let verdict = query.verdict.as_ref().map(spelled);
  let since = query.since.map(db::timestamp);
  let (offset, limit) = (query.offset, query.limit);
  db.read(move |conn| {
    let mut statement = conn.prepare(&format!(
      "{JUDGED} WHERE (?1 IS NULL OR r.verdict = ?1) AND (?2 IS NULL OR r.analysed_at > ?2) \
       ORDER BY r.analysed_at DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let rows = statement.query_map(params![verdict, since, limit, offset], |row| {
      let mut report = judged_report(row)?;
      report.windows = None;
      Ok(report)
    })?;
    rows.collect()
  })
  .await
}

/// Records the user's judgement of the file at `path`, replacing one recorded before.
pub(super) async fn judge(
  db: &Database,
  path: &str,
  verdict: Judgement,
  note: Option<String>,
  by: &str,
  at: DateTime<Utc>,
) -> Result<(), DbError> {
  let row = (
    path.to_string(),
    spelled(&verdict),
    note,
    by.to_string(),
    db::timestamp(at),
  );
  db.write(move |conn| {
    conn.execute(
      "INSERT OR REPLACE INTO judgements (path, verdict, note, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
      params![row.0, row.1, row.2, row.3, row.4],
    )?;
    Ok(())
  })
  .await
}

/// Takes the user's judgement of the file at `path` away; whether there was one.
pub(super) async fn unjudge(db: &Database, path: &str) -> Result<bool, DbError> {
  let path = path.to_string();
  db.write(move |conn| Ok(conn.execute("DELETE FROM judgements WHERE path = ?1", [path])? > 0))
    .await
}

/// Records that the images named `name` take `bytes` and were made or viewed `at`.
pub(super) async fn used_image(
  db: &Database,
  name: &str,
  bytes: Option<u64>,
  at: DateTime<Utc>,
) -> Result<(), DbError> {
  let (name, at) = (name.to_string(), db::timestamp(at));
  let bytes = bytes.map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX));
  db.write(move |conn| {
    match bytes {
      Some(bytes) => conn.execute(
        "INSERT OR REPLACE INTO images (name, bytes, used_at) VALUES (?1, ?2, ?3)",
        params![name, bytes, at],
      )?,
      None => conn.execute(
        "UPDATE images SET used_at = ?2 WHERE name = ?1",
        params![name, at],
      )?,
    };
    Ok(())
  })
  .await
}

/// Forgets the least recently used images until the rest take no more than `limit` bytes,
/// keeping the most recently used whatever its size; returns the names forgotten, whose
/// files are to go.
pub(super) async fn evict(db: &Database, limit: u64) -> Result<Vec<String>, DbError> {
  db.write(move |conn| {
    let transaction = conn.transaction()?;
    let images: Vec<(String, u64)> = {
      let mut statement = transaction.prepare("SELECT name, bytes FROM images ORDER BY used_at")?;
      statement
        .query_map([], |row| {
          Ok((
            row.get(0)?,
            // written from a u64, so never negative
            u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
          ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let mut total: u64 = images.iter().map(|(_, bytes)| bytes).sum();
    let mut gone = Vec::new();
    for (name, bytes) in images.iter().take(images.len().saturating_sub(1)) {
      if total <= limit {
        break;
      }
      transaction.execute("DELETE FROM images WHERE name = ?1", [name])?;
      total -= bytes;
      gone.push(name.clone());
    }
    transaction.commit()?;
    Ok(gone)
  })
  .await
}
