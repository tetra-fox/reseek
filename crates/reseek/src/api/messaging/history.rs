//! The room history routes: every room with stored messages, the rooms we left included,
//! reading and exporting one room's history, and deleting it.

use api_types::messaging::ExportQuery;
use api_types::messaging::rooms::{
  RoomHistory, RoomHistoryList, RoomHistoryListQuery, RoomMessage, RoomMessagesQuery, RoomNameQuery,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::rooms::PAGE_MAX;
use crate::api::error::ApiError;
use crate::api::extract::{Json, NamedRoom, Query};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};

/// The room history routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(rooms))
    .routes(routes!(messages))
    .routes(routes!(delete))
    .routes(routes!(export))
}

/// The rooms with stored messages, or the one `room` names.
#[utoipa::path(
  get, path = "/rooms/history", tag = "rooms", params(RoomHistoryListQuery),
  responses(
    (status = 200, body = RoomHistoryList), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn rooms(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<RoomHistoryListQuery>,
) -> Result<Json<RoomHistoryList>, ApiError> {
  app
    .services
    .messaging
    .room_history(query.room)
    .await
    .map(Json)
}

/// A page of a room's stored messages, oldest first, with the gaps in it; for any room, one we
/// left included.
#[utoipa::path(
  operation_id = "room_history_messages",
  get, path = "/rooms/history/{roomName}/messages", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery, RoomMessagesQuery),
  responses(
    (status = 200, body = RoomHistory), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn messages(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedRoom(room): NamedRoom,
  Query(query): Query<RoomMessagesQuery>,
) -> Result<Json<RoomHistory>, ApiError> {
  validate::limit("limit", query.limit, PAGE_MAX)?;
  app
    .services
    .messaging
    .stored_messages(&room, query)
    .await
    .map(Json)
}

/// Deletes a room's stored messages.
#[utoipa::path(
  operation_id = "delete_room_history",
  delete, path = "/rooms/history/{roomName}", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "deleted"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn delete(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.delete_history(&room).await?;
  info!(by = caller.principal.subject, room, "room history deleted");
  Ok(StatusCode::NO_CONTENT)
}

/// A room's whole stored history as a file to save, JSON or text.
#[utoipa::path(
  operation_id = "export_room_history",
  get, path = "/rooms/history/{roomName}/export", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery, ExportQuery),
  responses(
    (status = 200, content((Vec<RoomMessage> = "application/json"), (String = "text/plain"))),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn export(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedRoom(room): NamedRoom,
  Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
  app
    .services
    .messaging
    .export_room(&room, query.format)
    .await
}
