//! Pausing and resuming downloads. A paused download stops wherever it is, keeping what
//! arrived in its incomplete file, and waits here, queued but held, across restarts too, until
//! it is resumed; its peer is not told, so it loses its place in that user's queue. Resuming it
//! starts its task again on the same record, which carries on from the bytes it has.

use std::collections::BTreeMap;
use std::collections::hash_map::Entry;

use api_types::transfers::{BulkFailure, TransferStates};
use tracing::{debug, info};
use uuid::Uuid;

use super::enqueue::gate_for;
use super::{Downloads, InFlight};
use crate::sync::lock;
use crate::transfers::{Record, store};

impl Downloads {
  /// Whether the running download `id` was canceled to be paused.
  pub(super) fn pausing(&self, id: Uuid) -> bool {
    lock(&self.in_flight)
      .get(&id)
      .is_some_and(|entry| entry.pause)
  }

  /// The download records `ids`, each or why it cannot be had.
  async fn records(&self, ids: Vec<Uuid>) -> Vec<Result<Record, BulkFailure>> {
    let read = self
      .db
      .read(move |conn| {
        ids
          .into_iter()
          .map(|id| store::get(conn, id).map(|record| (id, record)))
          .collect::<rusqlite::Result<Vec<_>>>()
      })
      .await;
    match read {
      Ok(found) => found
        .into_iter()
        .map(|(id, record)| {
          record
            .filter(|r| r.direction == api_types::transfers::TransferDirection::Download)
            .ok_or_else(|| failure(id, format!("no download {id}")))
        })
        .collect(),
      Err(e) => vec![Err(failure(Uuid::nil(), e.to_string()))],
    }
  }

  /// Pauses the downloads `ids`: a running one stops wherever it is and is written paused by
  /// its task, one without a task is written paused here. A download that finished or is
  /// paused already is refused.
  pub async fn pause(&self, ids: Vec<Uuid>) -> Vec<Result<Uuid, BulkFailure>> {
    let mut outcomes = Vec::new();
    for record in self.records(ids).await {
      let mut record = match record {
        Ok(record) => record,
        Err(failed) => {
          outcomes.push(Err(failed));
          continue;
        }
      };
      if record.is_completed() {
        outcomes.push(Err(failure(record.id, "the download has completed")));
        continue;
      }
      if record.paused {
        outcomes.push(Err(failure(record.id, "the download is paused already")));
        continue;
      }
      let running = {
        let mut in_flight = lock(&self.in_flight);
        match in_flight.get_mut(&record.id) {
          Some(entry) => {
            entry.pause = true;
            entry.cancel.cancel();
            true
          }
          None => false,
        }
      };
      if running {
        debug!(id = %record.id, "running download stopped to be paused");
      } else {
        record.paused = true;
        record.state = TransferStates::Queued | TransferStates::Locally;
        record.next_attempt_at = None;
        record.place_in_queue = None;
        self.save(&record, true).await;
        info!(id = %record.id, username = record.username, "download paused");
      }
      outcomes.push(Ok(record.id));
    }
    outcomes
  }

  /// Resumes the paused downloads `ids`: each starts its task again on its record, the files of
  /// one user taking their turns as an enqueue's do. A download whose task is still stopping to
  /// be paused is waited for first. One not paused is refused.
  pub async fn resume_paused(&self, ids: Vec<Uuid>) -> Vec<Result<Uuid, BulkFailure>> {
    for id in &ids {
      self.pause_done(*id).await;
    }
    let mut outcomes = Vec::new();
    let mut by_user: BTreeMap<String, Vec<Record>> = BTreeMap::new();
    for record in self.records(ids).await {
      match record {
        Ok(record) if record.paused && !record.is_completed() => {
          by_user
            .entry(record.username.clone())
            .or_default()
            .push(record);
        }
        Ok(record) => outcomes.push(Err(failure(record.id, "the download is not paused"))),
        Err(failed) => outcomes.push(Err(failed)),
      }
    }
    for (username, records) in by_user {
      let request = (self.starts.request(), gate_for(records.len()));
      for mut record in records {
        let cancel = self.shutdown.child_token();
        match lock(&self.in_flight).entry(record.id) {
          // another resume of it got here first
          Entry::Occupied(_) => {
            outcomes.push(Err(failure(record.id, "the download is resumed already")));
            continue;
          }
          Entry::Vacant(entry) => {
            entry.insert(InFlight::new(cancel.clone()));
          }
        }
        record.paused = false;
        record.state = TransferStates::Queued | TransferStates::Locally;
        self.save(&record, true).await;
        info!(id = %record.id, username, "download resumed");
        let id = record.id;
        self.spawn(record, request, cancel);
        outcomes.push(Ok(id));
      }
    }
    outcomes
  }

  /// Waits for the task of the download `id` to end when it was stopped to be paused, so its
  /// record is written paused before it is read.
  async fn pause_done(&self, id: Uuid) {
    let ended = lock(&self.in_flight)
      .get(&id)
      .filter(|entry| entry.pause)
      .map(|entry| entry.ended.subscribe());
    if let Some(mut ended) = ended {
      debug!(%id, "resume waits for the download to stop");
      // nothing is ever sent: the wait ends when the entry, and its sender, are dropped
      let _ = ended.changed().await;
    }
  }
}

fn failure(id: Uuid, error: impl Into<String>) -> BulkFailure {
  BulkFailure {
    id,
    error: error.into(),
  }
}
