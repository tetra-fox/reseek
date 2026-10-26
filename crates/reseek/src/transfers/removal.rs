//! Taking completed transfers out of the listings. The rows stay for the reports; each one
//! removed is pushed to the event stream.

use api_types::live::LiveFrame;
use api_types::transfers::{TransferDirection, TransferRemoved};
use chrono::Utc;
use rusqlite::{Connection, params};
use uuid::Uuid;

use super::categories::COMPLETED;
use super::{Record, direction_text};
use crate::db::{Database, DbError, timestamp};
use crate::live::Live;

/// A transfer taken out of the listings: its id and its peer.
pub type Removed = (Uuid, String);

/// Reads the `RETURNING Id, Username` of a removing statement.
fn returned(
  conn: &Connection,
  sql: &str,
  params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<Removed>> {
  let mut statement = conn.prepare(sql)?;
  let rows = statement.query_map(params, |row| {
    let id: String = row.get(0)?;
    let id = id.parse::<Uuid>().map_err(|e| {
      rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    })?;
    Ok((id, row.get::<_, Option<String>>(1)?.unwrap_or_default()))
  })?;
  rows.collect()
}

/// Removes the transfer `id` of `direction` if it has completed and is not removed already;
/// returns its peer when it was.
pub fn remove_one(
  conn: &Connection,
  direction: TransferDirection,
  id: Uuid,
) -> rusqlite::Result<Option<String>> {
  let removed = returned(
    conn,
    &format!(
      "UPDATE Transfers SET Removed = 1, RemovedAt = ?3 WHERE Id = ?1 AND Direction = ?2 \
       AND Removed = 0 AND {COMPLETED} RETURNING Id, Username"
    ),
    params![
      id.to_string(),
      direction_text(direction),
      timestamp(Utc::now())
    ],
  )?;
  Ok(removed.into_iter().next().map(|(_, username)| username))
}

/// [`remove_one`] in `db`, pushing the removal.
pub async fn remove(
  db: &Database,
  live: &Live,
  direction: TransferDirection,
  id: Uuid,
) -> Result<bool, DbError> {
  let removed = db
    .write(move |conn| remove_one(conn, direction, id))
    .await?;
  match removed {
    Some(username) => {
      push(live, direction, &[(id, username)]);
      Ok(true)
    }
    None => Ok(false),
  }
}

/// Removes every completed transfer of `direction` that is not removed already, pushing each.
pub async fn remove_completed(
  db: &Database,
  live: &Live,
  direction: TransferDirection,
) -> Result<Vec<Removed>, DbError> {
  let removed = db
    .write(move |conn| {
      returned(
        conn,
        &format!(
          "UPDATE Transfers SET Removed = 1, RemovedAt = ?2 WHERE Direction = ?1 \
           AND Removed = 0 AND {COMPLETED} RETURNING Id, Username"
        ),
        params![direction_text(direction), timestamp(Utc::now())],
      )
    })
    .await?;
  push(live, direction, &removed);
  Ok(removed)
}

/// Removes the transfers of `direction` that match `condition`, a condition on `Transfers`
/// with one parameter, `?1`, bound to `value`; for the retention sweep.
pub fn remove_where(
  conn: &Connection,
  direction: TransferDirection,
  condition: &str,
  value: &str,
) -> rusqlite::Result<Vec<Removed>> {
  returned(
    conn,
    &format!(
      "UPDATE Transfers SET Removed = 1, RemovedAt = ?3 WHERE Direction = ?2 AND Removed = 0 \
       AND ({condition}) RETURNING Id, Username"
    ),
    params![value, direction_text(direction), timestamp(Utc::now())],
  )
}

/// Takes the records of `record`'s file, peer and direction other than `record` out of the
/// listings, for the new record that supersedes them.
pub fn supersede(conn: &Connection, record: &Record) -> rusqlite::Result<Vec<Removed>> {
  returned(
    conn,
    "UPDATE Transfers SET Removed = 1, RemovedAt = ?5 WHERE Username = ?1 AND Direction = ?2 \
     AND Filename = ?3 AND Id != ?4 AND Removed = 0 RETURNING Id, Username",
    params![
      record.username,
      direction_text(record.direction),
      record.filename,
      record.id.to_string(),
      timestamp(Utc::now()),
    ],
  )
}

/// Pushes `removed` to the event stream, one frame each.
pub fn push(live: &Live, direction: TransferDirection, removed: &[Removed]) {
  for (id, username) in removed {
    live.publish(LiveFrame::TransfersRemoved(TransferRemoved {
      id: *id,
      username: username.clone(),
      direction,
    }));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::transfers::fixtures::{Row, insert, transfers};

  #[tokio::test]
  async fn only_completed_transfers_of_the_direction_are_removed() {
    let db = transfers().await;
    let live = Live::new(tokio_util::sync::CancellationToken::new());
    let done = Row::new("alice", TransferDirection::Download, 48);
    let running = Row::new("alice", TransferDirection::Download, 8);
    let upload = Row::new("bob", TransferDirection::Upload, 48);
    insert(&db, vec![done.clone(), running.clone(), upload.clone()]).await;
    let download = TransferDirection::Download;
    assert!(!remove(&db, &live, download, running.id).await.unwrap());
    assert!(!remove(&db, &live, download, upload.id).await.unwrap());
    assert!(remove(&db, &live, download, done.id).await.unwrap());
    assert!(
      !remove(&db, &live, download, done.id).await.unwrap(),
      "once"
    );
    insert(
      &db,
      vec![Row::new("carol", TransferDirection::Download, 80)],
    )
    .await;
    let removed = remove_completed(&db, &live, download).await.unwrap();
    let names: Vec<&str> = removed.iter().map(|(_, u)| u.as_str()).collect();
    assert_eq!(names, ["carol"]);
  }
}
