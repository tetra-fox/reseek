//! The messaging routes: private conversations, the rooms we are in, the stored history of
//! every room, the pinned chats, and searching what was said. Reading takes the ReadOnly role, sending, joining,
//! acknowledging and every other change ReadWrite. A username or room name is its path segment,
//! or `?username=` and `?roomName=` when given, which reach names a path cannot hold.

mod conversations;
mod history;
mod pins;
mod rooms;

use api_types::messaging::{MessageSearchQuery, MessageSearchResults};
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::{Json, Query};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized};

/// The most characters a message search query has.
const QUERY_CHARS: usize = 200;

/// The messaging routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .merge(conversations::routes())
    .merge(rooms::routes())
    .merge(history::routes())
    .merge(pins::routes())
    .routes(routes!(search))
}

/// Refuses a message that is empty or longer than `chat.max_message_length` characters.
fn message(app: &AppState, text: &str) -> Result<(), ApiError> {
  if text.is_empty() {
    return Err(ApiError::bad_request("The message is empty"));
  }
  let max = app.config.current().values.chat.max_message_length;
  if text.chars().count() > max as usize {
    return Err(ApiError::bad_request(format!(
      "The message is longer than {max} characters"
    )));
  }
  Ok(())
}

/// Searches what was said in conversations and rooms. `q` is a full-text query: words,
/// `"phrases"`, `OR`, `NOT` and `prefix*`.
#[utoipa::path(
  get, path = "/messages/search", tag = "messages", params(MessageSearchQuery),
  responses(
    (status = 200, body = MessageSearchResults), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn search(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<MessageSearchQuery>,
) -> Result<Json<MessageSearchResults>, ApiError> {
  if query.q.trim().is_empty() {
    return Err(ApiError::bad_request("A value for q is required"));
  }
  if query.q.chars().count() > QUERY_CHARS {
    return Err(ApiError::bad_request(format!(
      "The value of q must be at most {QUERY_CHARS} characters"
    )));
  }
  validate::positive("limit", query.limit)?;
  app.services.messaging.search(query).await.map(Json)
}
