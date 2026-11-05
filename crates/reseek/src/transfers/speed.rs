//! Which pair of speed limits is in force, the normal or the alternative one: kept in
//! `transfers.db` so it lasts across restarts, shown in the state document, and in force for the
//! client's global limits and the uploads' privileged bucket.

use rusqlite::{Connection, OptionalExtension};
use soulseek_client::ClientOptionsPatch;
use tracing::info;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::db::{Database, DbError};
use crate::network::options::client_options;

fn read(conn: &Connection) -> rusqlite::Result<bool> {
  let stored = conn
    .query_row(
      "SELECT alternative FROM speed_limits WHERE id = 1",
      [],
      |row| row.get(0),
    )
    .optional()?;
  // there is no row until the first switch, and until then the normal limits are in force
  Ok(stored.unwrap_or(false))
}

fn write(conn: &Connection, alternative: bool) -> rusqlite::Result<()> {
  conn.execute(
    "INSERT INTO speed_limits (id, alternative) VALUES (1, ?1) \
     ON CONFLICT (id) DO UPDATE SET alternative = excluded.alternative",
    [alternative],
  )?;
  Ok(())
}

/// Whether the alternative speed limits were in force when last switched.
pub async fn stored(db: &Database) -> Result<bool, DbError> {
  db.read(read).await
}

/// Puts the alternative speed limits in force, or the normal ones, `by` naming who asked: kept,
/// shown in the state, and applied to the uploads' buckets and the client.
pub async fn switch(app: &AppState, alternative: bool, by: &str) -> Result<(), ApiError> {
  let network = &app.network;
  let _reconfiguring = network.reconfiguring().lock().await;
  app
    .transfers
    .write(move |conn| write(conn, alternative))
    .await
    .map_err(ApiError::internal)?;
  app
    .state
    .update(|state| state.alternative_speed_limits = alternative);
  app.uploads.switch_speed_limits(alternative);
  let wanted =
    client_options(&app.config.current().values, alternative).map_err(ApiError::internal)?;
  let patch = ClientOptionsPatch {
    maximum_upload_speed: Some(wanted.maximum_upload_speed),
    maximum_download_speed: Some(wanted.maximum_download_speed),
    ..ClientOptionsPatch::default()
  };
  network
    .client()
    .reconfigure_options(patch)
    .await
    .map_err(ApiError::internal)?;
  info!(by, alternative, "speed limits switched");
  Ok(())
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use crate::testing::{TestApp, app, request};

  /// The client's global upload and download limits.
  fn limits(app: &TestApp) -> (u32, u32) {
    let options = app.state.network.client().options();
    (options.maximum_upload_speed, options.maximum_download_speed)
  }

  #[tokio::test]
  async fn the_switch_reaches_the_client_and_lasts_across_a_restart() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
       [transfers.upload]\nspeed_limit = 64\nalt_speed_limit = 16\n\
       [transfers.download]\nalt_speed_limit = 8\n",
      &[],
    )
    .await;
    assert_eq!(limits(&app), (64, 0));
    let on = json!({ "alternative": true });
    let answer = app
      .send(request("PUT", "/api/v0/transfers/speed-limits", Some(on)))
      .await;
    assert_eq!(answer.status, 204, "{:?}", answer.body);
    assert!(app.state.state.current().alternative_speed_limits);
    assert_eq!(limits(&app), (16, 8));

    let app = app.restarted(&[]).await;
    assert!(app.state.state.current().alternative_speed_limits);
    assert_eq!(limits(&app), (16, 8));
    let off = json!({ "alternative": false });
    let answer = app
      .send(request("PUT", "/api/v0/transfers/speed-limits", Some(off)))
      .await;
    assert_eq!(answer.status, 204);
    assert_eq!(limits(&app), (64, 0));
  }
}
