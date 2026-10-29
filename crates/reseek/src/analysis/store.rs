//! `analysis.db`: the newest report of each file, the user's judgement of each file, and the
//! spectrogram and waveform files kept on disk with when each was last viewed.

use api_types::analysis::{
  AnalysisListQuery, AnalysisMeasures, AnalysisOverride, AnalysisVerdict, AudioAnalysis,
  ClaimedFormat, Judgement,
};
use api_types::files::FileRoot;
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use super::report::spectrogram_url;
use crate::db::{self, Database, DbError, Schema};

/// The analysis database's migrations.
pub static SCHEMA: Schema = db::schema!(
  "analysis",
  ["0001_initial", "0002_report_format", "0003_spectra"]
);

/// The shape of the report JSON this version writes. Raise it with any change a stored report
/// could not be read back through: a report stored in another shape reads as stale.
pub(super) const REPORT_FORMAT: i64 = 2;

/// What a stale report says in place of a reading.
pub(super) const STALE_SUMMARY: &str =
  "This was analysed by an older version of reseek. Analyse again to see it.";

/// A report as it is stored.
#[derive(Clone, Debug)]
pub(super) struct Stored {
  /// the report, with its windows and the judgement set now
  pub report: AudioAnalysis,
  /// the size the file had when analysed
  pub size: u64,
  /// when the file had last changed when analysed, as stored
  pub modified: String,
  /// the name the report's spectrogram and waveform are kept under
  pub kept: String,
  /// why a report stored in this version's own shape could not be read, so it reads as stale
  pub unreadable: Option<String>,
}

/// A listed report: the report or its stale stand-in, and why it could not be read as
/// [`Stored::unreadable`] says.
pub(super) struct Listed {
  pub report: AudioAnalysis,
  pub unreadable: Option<String>,
}

/// A report to store, which names its file.
pub(super) struct NewReport<'a> {
  pub report: &'a AudioAnalysis,
  pub modified: &'a str,
  pub kept: &'a str,
}

/// A value of one of the API's enums as its JSON spells it, which the database stores.
fn spelled(value: &impl serde::Serialize) -> String {
  serde_plain::to_string(value).expect("the analysis enums are plain names")
}

/// A failure to read column `index` as its type.
fn conversion(index: usize) -> impl Fn(serde_plain::Error) -> rusqlite::Error {
  move |e| rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, e.into())
}

/// The report of a row read with [`JUDGED`], and why its JSON could not be read when it was in
/// this version's shape. The JSON is in column 0 and its shape in column 14; the judgement is in
/// columns 4 to 7, null when there is none; the file's root, path under it, download, local path,
/// when it was analysed and the reading are in columns 8 to 13, which name the file whatever the
/// JSON holds. A report in another shape, or one that does not read, is a stale stand-in made from
/// those columns, with the judgement kept.
fn judged_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<(AudioAnalysis, Option<String>)> {
  let judgement = match row.get::<_, Option<String>>(4)? {
    Some(verdict) => Some(AnalysisOverride {
      verdict: serde_plain::from_str(&verdict).map_err(conversion(4))?,
      note: row.get(5)?,
      by: row.get(6)?,
      at: db::read_timestamp(&row.get::<_, String>(7)?)?,
    }),
    None => None,
  };
  let format: Option<i64> = row.get(14)?;
  let read = match format {
    Some(format) if format != REPORT_FORMAT => None,
    _ => Some(decoded(row)?),
  };
  let (mut report, unreadable) = match read {
    Some(Ok(report)) => (report, None),
    // one stored before shapes were numbered that does not read is in an older shape
    Some(Err(e)) if format.is_some() => (stale(row)?, Some(e)),
    _ => (stale(row)?, None),
  };
  report.judgement = judgement;
  Ok((report, unreadable))
}

/// The report in a row's JSON, named by its columns, or why it does not read.
fn decoded(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<AudioAnalysis, String>> {
  let json: String = row.get(0)?;
  let naming = [
    ("root", row.get::<_, String>(8)?.into()),
    ("relativePath", row.get::<_, String>(9)?.into()),
    ("transferId", row.get::<_, Option<String>>(10)?.into()),
    ("stale", false.into()),
  ];
  let read = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json)
    .and_then(|mut fields| {
      for (key, value) in naming {
        fields.insert(key.into(), value);
      }
      serde_json::from_value(fields.into())
    })
    .map_err(|e| e.to_string());
  Ok(read)
}

/// The stand-in for a report this version cannot show, from the row's columns.
fn stale(row: &rusqlite::Row<'_>) -> rusqlite::Result<AudioAnalysis> {
  let root: FileRoot = serde_plain::from_str(&row.get::<_, String>(8)?).map_err(conversion(8))?;
  let relative_path: String = row.get(9)?;
  Ok(AudioAnalysis {
    path: row.get(11)?,
    spectrogram_url: spectrogram_url(root, &relative_path),
    root,
    relative_path,
    // written from a uuid
    transfer_id: row
      .get::<_, Option<String>>(10)?
      .and_then(|id| Uuid::parse_str(&id).ok()),
    // written from a u64, so never negative
    size: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
    analysed_at: db::read_timestamp(&row.get::<_, String>(12)?)?,
    duration_seconds: 0.0,
    claimed: ClaimedFormat {
      codec: "unknown".to_string(),
      sample_rate: 0,
      bit_depth: None,
      channels: 0,
      bitrate_kbps: None,
    },
    // a reading an older version gave that this one no longer has is no reading to show
    verdict: serde_plain::from_str(&row.get::<_, String>(13)?)
      .unwrap_or(AnalysisVerdict::Inconclusive),
    confidence: 0.0,
    flags: Vec::new(),
    encoder_guess: None,
    summary: STALE_SUMMARY.to_string(),
    measures: AnalysisMeasures {
      cutoff_hz_histogram: Vec::new(),
      cutoff_median_hz: None,
      shelf_sharpness_db_p95: None,
      sfb21_holes: None,
      frame_artefact_score: None,
      upsample_ratio: None,
      bit_depth_effective: None,
      noise_floor_dbfs: None,
      stereo_correlation: None,
      side_energy_db: None,
      dc_offset: None,
      clipped_samples_pct: None,
    },
    windows: None,
    spectrogram: None,
    judgement: None,
    limitations: Vec::new(),
    stale: true,
  })
}

const JUDGED: &str = "SELECT r.report, r.size, r.modified, r.spectra, j.verdict, j.note, j.by, j.at, \
                      r.root, r.relative, r.transfer_id, r.path, r.analysed_at, r.verdict, \
                      r.format \
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
    new.report.root.to_string(),
    new.report.relative_path.clone(),
    i64::try_from(new.report.size).unwrap_or(i64::MAX),
    new.modified.to_string(),
    new.report.transfer_id.map(|id| id.to_string()),
    db::timestamp(new.report.analysed_at),
    spelled(&new.report.verdict),
    new.kept.to_string(),
    json,
  );
  db.write(move |conn| {
    conn.execute(
      "INSERT OR REPLACE INTO reports \
       (path, root, relative, size, modified, transfer_id, analysed_at, verdict, spectra, \
       report, format) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
      params![
        row.0,
        row.1,
        row.2,
        row.3,
        row.4,
        row.5,
        row.6,
        row.7,
        row.8,
        row.9,
        REPORT_FORMAT
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
        let (report, unreadable) = judged_report(row)?;
        Ok(Stored {
          report,
          unreadable,
          // written from a u64, so never negative
          size: u64::try_from(row.get::<_, i64>(1)?).unwrap_or_default(),
          modified: row.get(2)?,
          kept: row.get(3)?,
        })
      })
      .optional()
  })
  .await
}

/// The reports `query` asks for, newest first, without their windows.
pub(super) async fn list(db: &Database, query: &AnalysisListQuery) -> Result<Vec<Listed>, DbError> {
  let verdict = query.verdict.as_ref().map(spelled);
  let since = query.since.map(db::timestamp);
  let (offset, limit) = (query.offset, query.limit);
  db.read(move |conn| {
    let mut statement = conn.prepare(&format!(
      "{JUDGED} WHERE (?1 IS NULL OR r.verdict = ?1) AND (?2 IS NULL OR r.analysed_at > ?2) \
       ORDER BY r.analysed_at DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let rows = statement.query_map(params![verdict, since, limit, offset], |row| {
      let (mut report, unreadable) = judged_report(row)?;
      report.windows = None;
      Ok(Listed { report, unreadable })
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

/// Records that the spectrogram and waveform file named `name` takes `bytes` and was made or
/// viewed `at`.
pub(super) async fn used(
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
        "INSERT OR REPLACE INTO spectra (name, bytes, used_at) VALUES (?1, ?2, ?3)",
        params![name, bytes, at],
      )?,
      None => conn.execute(
        "UPDATE spectra SET used_at = ?2 WHERE name = ?1",
        params![name, at],
      )?,
    };
    Ok(())
  })
  .await
}

/// Forgets the least recently used spectrogram and waveform files until the rest take no more
/// than `limit` bytes, keeping the most recently used whatever its size; returns the names
/// forgotten, whose files are to go.
pub(super) async fn evict(db: &Database, limit: u64) -> Result<Vec<String>, DbError> {
  db.write(move |conn| {
    let transaction = conn.transaction()?;
    let kept: Vec<(String, u64)> = {
      let mut statement =
        transaction.prepare("SELECT name, bytes FROM spectra ORDER BY used_at")?;
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
    let mut total: u64 = kept.iter().map(|(_, bytes)| bytes).sum();
    let mut gone = Vec::new();
    for (name, bytes) in kept.iter().take(kept.len().saturating_sub(1)) {
      if total <= limit {
        break;
      }
      transaction.execute("DELETE FROM spectra WHERE name = ?1", [name])?;
      total -= bytes;
      gone.push(name.clone());
    }
    transaction.commit()?;
    Ok(gone)
  })
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::{Location, Schema};

  /// The database as it was before reports carried their shape.
  static BEFORE: Schema = Schema {
    name: "analysis",
    migrations: SCHEMA.migrations.split_at(1).0,
    private: false,
  };

  const PATH: &str = "/srv/downloads/a.flac";

  fn everything() -> AnalysisListQuery {
    AnalysisListQuery {
      verdict: None,
      since: None,
      offset: 0,
      limit: 100,
    }
  }

  #[tokio::test]
  async fn a_report_stored_before_shapes_were_numbered_reads_as_stale_with_its_judgement() {
    let dir = tempfile::tempdir().unwrap();
    let location = Location {
      data: dir.path().join("data"),
      backups: dir.path().join("backups"),
      volatile: false,
      log_sql: false,
    };
    std::fs::create_dir_all(&location.data).unwrap();
    std::fs::create_dir_all(&location.backups).unwrap();
    let db = Database::open(&BEFORE, &location).await.unwrap();
    let at = db::timestamp(Utc::now());
    db.write(move |conn| {
      // a report in a shape this version's type no longer reads
      conn.execute(
        "INSERT INTO reports \
         (path, root, relative, size, modified, transfer_id, analysed_at, verdict, image, report) \
         VALUES (?1, 'downloads', 'a.flac', 10, 'then', NULL, ?2, 'likely_transcode', 'img', \
         '{\"path\": \"/srv/downloads/a.flac\", \"summary\": \"an older shape\"}')",
        params![PATH, at],
      )?;
      conn.execute(
        "INSERT INTO judgements (path, verdict, note, by, at) \
         VALUES (?1, 'fake', 'an old rip', 'local:reseek', ?2)",
        params![PATH, at],
      )
    })
    .await
    .unwrap();
    drop(db);

    let db = Database::open(&SCHEMA, &location).await.unwrap();
    let stored = find(&db, PATH).await.unwrap().unwrap();
    let report = &stored.report;
    assert!(report.stale);
    assert_eq!(
      stored.unreadable, None,
      "an older shape is no fault to report"
    );
    assert_eq!(report.summary, STALE_SUMMARY);
    assert_eq!(report.verdict, AnalysisVerdict::LikelyTranscode);
    assert_eq!(
      (report.root, report.relative_path.as_str()),
      (FileRoot::Downloads, "a.flac")
    );
    let judgement = report.judgement.as_ref().unwrap();
    assert_eq!(judgement.verdict, Judgement::Fake);
    assert_eq!(judgement.note.as_deref(), Some("an old rip"));
    let listed = list(&db, &everything()).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].report.stale && listed[0].report.judgement.is_some());

    // a report in this version's own shape that does not read is stale too, and says why
    let format = |format: i64| {
      move |conn: &mut rusqlite::Connection| {
        conn.execute("UPDATE reports SET format = ?1", [format])
      }
    };
    db.write(format(REPORT_FORMAT)).await.unwrap();
    let stored = find(&db, PATH).await.unwrap().unwrap();
    assert!(stored.report.stale);
    assert!(stored.unreadable.unwrap().contains("missing field"));
    // one in another numbered shape is stale without being read
    db.write(format(REPORT_FORMAT + 1)).await.unwrap();
    let stored = find(&db, PATH).await.unwrap().unwrap();
    assert!(stored.report.stale);
    assert_eq!(stored.unreadable, None);
  }
}
