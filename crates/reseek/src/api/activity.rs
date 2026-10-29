//! The activity log: what other users on the network did with us, as kept in memory.

use api_types::activity::ActivityEntry;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::extract::Json;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized};

/// The activity routes in the API document.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(get_activity))
}

/// The activity log's entries, oldest first: the last few hundred things other users did with
/// us since the daemon started. The `activity` topic of the event stream sends the same entries,
/// then each new one.
#[utoipa::path(
  get, path = "/activity", tag = "activity",
  responses(
    (status = 200, body = Vec<ActivityEntry>), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn get_activity(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Json<Vec<ActivityEntry>> {
  Json(app.activity.snapshot())
}

#[cfg(test)]
mod tests {
  use api_types::activity::ActivityKind;

  use crate::testing::{TRUSTED, app, request};

  #[tokio::test]
  async fn the_log_is_read_whole_then_followed_entry_by_entry() {
    let app = app(TRUSTED, &[]).await;
    app
      .state
      .activity
      .record(Some("alice"), ActivityKind::Browse);
    let read = app.send(request("GET", "/api/v0/activity", None)).await;
    assert_eq!(read.status, 200);
    assert_eq!(read.json()[0]["username"], "alice");
    assert_eq!(read.json()[0]["kind"]["type"], "browse");

    let mut stream = app
      .stream_from(
        "127.0.0.1:1",
        request("GET", "/api/v0/live?topics=activity", None),
      )
      .await;
    let buffer = stream.until("activity.buffer").await.pop().unwrap();
    assert_eq!(buffer.data.as_array().unwrap().len(), 1);
    app.state.activity.queued("bob", "Music\\a.flac");
    app.state.activity.queued("bob", "Music\\b.flac");
    let first = stream.until("activity.entry").await.pop().unwrap();
    let grown = stream.until("activity.entry").await.pop().unwrap();
    assert_eq!(first.data["id"], grown.data["id"]);
    assert_eq!(grown.data["kind"]["files"], 2);
    assert_eq!(grown.data["kind"]["filename"], "Music\\b.flac");
  }
}
