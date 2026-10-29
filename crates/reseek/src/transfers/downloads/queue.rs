//! A download's place in its peer's queue: asked of the peer, at most once every
//! [`ASK_EVERY`] per download, and taken when the peer says it on its own. The place is kept
//! on the record until the download leaves the peer's queue.

use std::time::Duration;

use api_types::transfers::TransferStates;
use api_types::users::blocking::BlockScope;
use soulseek_client::ClientError;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};
use uuid::Uuid;

use super::Downloads;
use crate::api::error::ApiError;
use crate::sync::lock;
use crate::transfers::store;

/// How long a place a peer gave is answered again without asking it.
const ASK_EVERY: Duration = Duration::from_secs(10);

impl Downloads {
  /// The place of the download `id` of `username` in the peer's queue, asked of the peer and
  /// kept on the record. A download not queued by its peer is a conflict and a peer that does
  /// not answer in time a 504; within `ASK_EVERY` of the last answer that answer is given
  /// again.
  pub async fn place_in_queue(&self, username: &str, id: Uuid) -> Result<u32, ApiError> {
    if self.users.is_blocked(username, None, BlockScope::Downloads) {
      return Err(ApiError::not_found(format!(
        "no download {id} from {username}"
      )));
    }
    let mut record = self.record_of(username, id).await?;
    if record.state != TransferStates::Queued | TransferStates::Remotely {
      return Err(ApiError::conflict("Transfer is not queued remotely", None));
    }
    let recent = lock(&self.asked)
      .get(&id)
      .is_some_and(|at| at.elapsed() < ASK_EVERY);
    if let (true, Some(place)) = (recent, record.place_in_queue) {
      debug!(%id, place, "place in queue answered from the last answer");
      return Ok(place);
    }
    let timeout = self
      .config
      .current()
      .values
      .soulseek
      .connection
      .timeout
      .inactivity;
    let cancel = CancellationToken::new();
    let client = self.network.client();
    let asked = client.get_download_place_in_queue(username, record.filename.clone(), &cancel);
    let place = match tokio::time::timeout(timeout, asked).await {
      Ok(Ok(place)) => place,
      Ok(Err(e)) => return Err(asked_failed(username, e)),
      Err(_) => {
        cancel.cancel();
        return Err(ApiError::gateway_timeout(format!(
          "{username} did not say the place in its queue in time"
        )));
      }
    };
    if self.running(id) {
      lock(&self.asked).insert(id, Instant::now());
    }
    record.place_in_queue = Some(place);
    self.place(record.id, place).await;
    info!(%id, username, place, "place in the peer's queue");
    Ok(place)
  }

  /// A place in queue the peer `username` sent on its own for `filename`: kept on the running
  /// download of that file while the peer has it queued.
  pub async fn place_received(&self, username: &str, filename: &str, place: u32) {
    let name = username.to_string();
    let file = filename.to_string();
    let found = self
      .db
      .read(move |conn| {
        conn
          .query_row(
            "SELECT Id FROM Transfers WHERE Username = ?1 AND Filename = ?2 AND Direction = \
             'Download' AND State = ?3 AND Removed = 0",
            rusqlite::params![
              name,
              file,
              (TransferStates::Queued | TransferStates::Remotely).bits()
            ],
            |row| row.get::<_, String>(0),
          )
          .map(Some)
          .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
          })
      })
      .await;
    match found {
      Ok(Some(id)) => match id.parse() {
        Ok(id) => self.place(id, place).await,
        Err(e) => debug!(id, error = %e, "download with a malformed id not given its place"),
      },
      Ok(None) => debug!(
        username,
        filename, place, "place in queue for no queued download"
      ),
      Err(e) => debug!(username, filename, error = %e, "place in queue not recorded"),
    }
  }

  /// Writes `place` on the download `id` while it is queued by its peer, and pushes it.
  async fn place(&self, id: Uuid, place: u32) {
    let remotely = (TransferStates::Queued | TransferStates::Remotely).bits();
    let written = self
      .db
      .write(move |conn| {
        let changed = conn.execute(
          "UPDATE Transfers SET PlaceInQueue = ?2 WHERE Id = ?1 AND State = ?3",
          rusqlite::params![id.to_string(), place, remotely],
        )?;
        if changed == 0 {
          return Ok(None);
        }
        store::get(conn, id)
      })
      .await;
    match written {
      Ok(Some(record)) => self.push(&record),
      Ok(None) => debug!(%id, "download left the peer's queue before its place was written"),
      Err(e) => debug!(%id, error = %e, "place in queue not written"),
    }
  }

  /// Whether the download `id` has a task running.
  fn running(&self, id: Uuid) -> bool {
    lock(&self.in_flight).contains_key(&id)
  }
}

/// The answer to a peer that could not say a place in its queue.
fn asked_failed(username: &str, error: ClientError) -> ApiError {
  match error {
    ClientError::TransferNotFound { .. } => {
      ApiError::conflict("Transfer is not queued remotely", Some(error.to_string()))
    }
    ClientError::Timeout(message) => ApiError::gateway_timeout(message),
    ClientError::UserOffline(_) | ClientError::UserNotFound(_) => ApiError::offline(username),
    ClientError::InvalidOperation(message) => ApiError::unavailable(Some(message)),
    other => ApiError::upstream(
      format!("{username} did not say the place in its queue"),
      Some(other.to_string()),
    ),
  }
}
