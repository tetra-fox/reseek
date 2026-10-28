//! Recording an accepted upload request: unless an upload of the same file to the same user is
//! queued or running already, a record is written, the user's earlier records of the file are
//! taken out of the listings, the upload enters the queue, and its task starts. An upload whose
//! client side has ended does not stand for a new request, though its task may not have written
//! how it ended yet: a peer told the upload failed asks again at once.

use std::collections::HashSet;

use api_types::live::LiveFrame;
use api_types::transfers::TransferDirection;
use rusqlite::{Connection, params};
use soulseek_client::ClientError;
use tracing::info;

use super::intake::Located;
use super::{InFlight, Uploads};
use crate::sync::lock;
use crate::transfers::categories::UNFINISHED;
use crate::transfers::removal::{self, Removed};
use crate::transfers::{Record, direction_text, store};
use crate::users::Touch;

/// Why a record of an upload the client already runs is not written.
const DUPLICATE: &str =
  "A duplicate upload of the same file to the same user is already registered";

/// What the transaction found or wrote.
enum Written {
  /// an upload of the file to the user is queued or running already: these
  Running(Vec<String>),
  /// the client runs an upload of the file to the user that no record knows of
  Duplicate,
  /// the record, and the user's earlier records of the file taken out of the listings
  Inserted(Vec<Removed>),
}

/// Writes `record` and takes the user's earlier records of the file out of the listings, in
/// one transaction, unless an unfinished upload of the file to the user is among `live`, the
/// uploads whose client side has not ended, or `tracked` says the client runs one.
fn write(
  conn: &mut Connection,
  record: &Record,
  tracked: bool,
  live: &HashSet<String>,
) -> rusqlite::Result<Written> {
  let tx = conn.transaction()?;
  let running: Vec<String> = tx
    .prepare(&format!(
      "SELECT Id FROM Transfers WHERE Username = ?1 AND Filename = ?2 AND Direction = ?3 \
       AND {UNFINISHED}"
    ))?
    .query_map(
      params![
        record.username,
        record.filename,
        direction_text(TransferDirection::Upload)
      ],
      |row| row.get(0),
    )?
    .collect::<rusqlite::Result<Vec<String>>>()?
    .into_iter()
    .filter(|id| live.contains(id))
    .collect();
  if !running.is_empty() {
    return Ok(Written::Running(running));
  }
  if tracked {
    return Ok(Written::Duplicate);
  }
  store::insert(&tx, record)?;
  let superseded = removal::supersede(&tx, record)?;
  tx.commit()?;
  Ok(Written::Inserted(superseded))
}

impl Uploads {
  /// Records the upload of `filename` to `username` from `located`, queues it and starts its
  /// task; an upload of the file to the user queued or running already stands for it.
  pub(super) async fn enqueue(
    &self,
    username: &str,
    filename: &str,
    located: Located,
  ) -> Result<(), ClientError> {
    let client = self.client()?;
    let record = Record::upload(username, filename, located.size);
    let id = record.id;
    // the handle exists before the record can be seen, so a cancel always finds it
    let cancel = self.shutdown.child_token();
    let live: HashSet<String> = {
      let mut in_flight = lock(&self.in_flight);
      let live = in_flight
        .iter()
        .filter(|(_, entry)| !entry.ended)
        .map(|(id, _)| id.to_string())
        .collect();
      in_flight.insert(
        id,
        InFlight {
          cancel: cancel.clone(),
          remove: false,
          reason: None,
          ended: false,
        },
      );
      live
    };
    // read after the snapshot: an upload is marked ended just before the client lets go of it,
    // so one the snapshot saw end is no longer the client's either
    let tracked = client
      .uploads()
      .iter()
      .any(|t| t.username == username && t.filename.as_str() == filename);
    let written = record.clone();
    let result = self
      .db
      .write(move |conn| write(conn, &written, tracked, &live))
      .await;
    let superseded = match result {
      Ok(Written::Inserted(superseded)) => superseded,
      Ok(Written::Running(ids)) => {
        lock(&self.in_flight).remove(&id);
        info!(
          username,
          filename,
          ?ids,
          "upload already queued or in progress"
        );
        return Ok(());
      }
      Ok(Written::Duplicate) => {
        lock(&self.in_flight).remove(&id);
        return Err(ClientError::client(DUPLICATE, None));
      }
      Err(e) => {
        lock(&self.in_flight).remove(&id);
        return Err(ClientError::client(
          format!("the upload record was not written: {e}"),
          None,
        ));
      }
    };
    removal::push(&self.live, TransferDirection::Upload, &superseded);
    self.users.touch(username, Touch::Transfer).await;
    self
      .live
      .publish(LiveFrame::TransfersAdded(Box::new(record.transfer())));
    self.queue.enqueue(id, username, filename);
    self.spawn(record, located, cancel);
    Ok(())
  }
}
