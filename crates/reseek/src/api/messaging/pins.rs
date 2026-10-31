//! The pinned chats route: the chats above the others in the chat list, in the user's order.

use std::collections::HashSet;

use api_types::messaging::{ChatKind, ChatPin};
use axum::extract::State;
use axum::http::StatusCode;
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

/// The pinned chats in the user's order, those of a room left or a conversation closed included.
#[utoipa::path(
  get, path = "/chat/pins", tag = "chat",
  responses(
    (status = 200, body = Vec<ChatPin>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn pins(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<ChatPin>>, ApiError> {
  app.services.messaging.pins().await.map(Json)
}

/// Replaces the pinned chats with the list given, in its order. The server's notices cannot be
/// pinned, and a chat is named once.
#[utoipa::path(
  put, path = "/chat/pins", tag = "chat", request_body = Vec<ChatPin>,
  responses(
    (status = 204, description = "set"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_pins(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Json(pins): Json<Vec<ChatPin>>,
) -> Result<StatusCode, ApiError> {
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
  app.services.messaging.set_pins(pins).await?;
  Ok(StatusCode::NO_CONTENT)
}
