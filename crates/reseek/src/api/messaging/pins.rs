//! The pinned chats route: the chats above the others in the chat list, in the user's order.

use std::collections::HashSet;

use api_types::messaging::{ChatKind, ChatPin, ChatPins};
use axum::extract::State;
use soulseek_proto::SERVER_USERNAME;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::Json;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};

/// The pinned chats route.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(pins, set_pins))
}

/// The pinned chats in the user's order, those of a room left or a conversation closed included,
/// with the list's revision.
#[utoipa::path(
  get, path = "/chat/pins", tag = "chat",
  responses(
    (status = 200, body = ChatPins), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn pins(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<ChatPins>, ApiError> {
  app.services.messaging.pins().await.map(Json)
}

/// Replaces the pinned chats with the list given, in its order, and answers the list with the
/// revision the change made. The server's notices cannot be pinned, and a chat is named once.
#[utoipa::path(
  put, path = "/chat/pins", tag = "chat", request_body = Vec<ChatPin>,
  responses(
    (status = 200, body = ChatPins), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_pins(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Json(pins): Json<Vec<ChatPin>>,
) -> Result<Json<ChatPins>, ApiError> {
  let mut seen = HashSet::new();
  for pin in &pins {
    if pin.name.is_empty() {
      return Err(ApiError::bad_request("A pinned chat has no name"));
    }
    if pin.kind == ChatKind::User && pin.name == SERVER_USERNAME {
      return Err(ApiError::bad_request(
        "The server's notices cannot be pinned",
      ));
    }
    if !seen.insert(pin) {
      return Err(ApiError::bad_request(format!(
        "{} is pinned twice",
        pin.name
      )));
    }
  }
  app.services.messaging.set_pins(pins).await.map(Json)
}
