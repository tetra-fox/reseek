//! The rooms routes: the rooms we are in, joining and leaving, what is said and who is there,
//! tickers, private room membership, read markers and notifications, and the server's room
//! list.

use api_types::messaging::rooms::{
  JoinRoom, JoinedRoom, ReadMarker, Room, RoomHistory, RoomInfo, RoomMemberName, RoomMessagesQuery,
  RoomNameQuery, RoomTicker, RoomUser, TickerText,
};
use api_types::messaging::{MessageText, NotifySetting};
use api_types::users::UsernameQuery;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::message;
use crate::api::error::ApiError;
use crate::api::extract::{Json, NamedRoom, NamedUser, Query};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};
use crate::services::Joined;

/// The most messages a page of a room's history holds.
pub(super) const PAGE_MAX: u32 = 500;

/// The rooms routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(joined, join))
    .routes(routes!(room, leave))
    .routes(routes!(users))
    .routes(routes!(messages, say))
    .routes(routes!(set_ticker))
    .routes(routes!(tickers))
    .routes(routes!(add_member))
    .routes(routes!(remove_member))
    .routes(routes!(add_operator))
    .routes(routes!(remove_operator))
    .routes(routes!(drop_membership))
    .routes(routes!(drop_ownership))
    .routes(routes!(mark_read))
    .routes(routes!(set_notify))
    .routes(routes!(available))
}

/// The rooms we are in, with their unread counts.
#[utoipa::path(
  get, path = "/rooms/joined", tag = "rooms",
  responses(
    (status = 200, body = Vec<JoinedRoom>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn joined(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<JoinedRoom>>, ApiError> {
  app.services.messaging.joined_rooms().await.map(Json)
}

/// Joins a room, creating it, as a private room with `private`, when it does not exist.
/// Answered 304 when we are in it already.
#[utoipa::path(
  post, path = "/rooms/joined", tag = "rooms", request_body = JoinRoom,
  responses(
    (status = 201, body = Room), (status = 304, description = "in the room already"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn join(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(request): Json<JoinRoom>,
) -> Result<Response, ApiError> {
  validate::room_name(&request.name)?;
  let name = request.name.clone();
  match app.services.messaging.join(request).await? {
    Joined::New(room) => {
      info!(by = caller.principal.subject, room = name, "room joined");
      Ok((StatusCode::CREATED, Json(room)).into_response())
    }
    Joined::Already => Ok(StatusCode::NOT_MODIFIED.into_response()),
  }
}

/// A room we are in, with its users and tickers.
#[utoipa::path(
  get, path = "/rooms/joined/{roomName}", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 200, body = Room), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn room(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedRoom(room): NamedRoom,
) -> Result<Json<Room>, ApiError> {
  app.services.messaging.room(&room).await.map(Json)
}

/// Leaves a room.
#[utoipa::path(
  delete, path = "/rooms/joined/{roomName}", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "left"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn leave(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.leave(&room).await?;
  info!(by = caller.principal.subject, room, "room left");
  Ok(StatusCode::NO_CONTENT)
}

/// The users in a room we are in.
#[utoipa::path(
  get, path = "/rooms/joined/{roomName}/users", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 200, body = Vec<RoomUser>), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn users(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedRoom(room): NamedRoom,
) -> Result<Json<Vec<RoomUser>>, ApiError> {
  app.services.messaging.room_users(&room).await.map(Json)
}

/// A page of what was said in a room we are in, oldest first, with the gaps where we were not
/// there to hear it; `before` pages back from a message.
#[utoipa::path(
  operation_id = "room_messages",
  get, path = "/rooms/joined/{roomName}/messages", tag = "rooms",
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
    .room_messages(&room, query)
    .await
    .map(Json)
}

/// Says something in a room, the body a JSON string. It shows once the server sends it back.
#[utoipa::path(
  post, path = "/rooms/joined/{roomName}/messages", tag = "rooms", request_body = MessageText,
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 201, description = "sent"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn say(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  Json(MessageText(text)): Json<MessageText>,
) -> Result<StatusCode, ApiError> {
  message(&app, &text)?;
  app.services.messaging.send_room(&room, text).await?;
  Ok(StatusCode::CREATED)
}

/// Sets our ticker in a room, the body a JSON string; an empty one clears it.
#[utoipa::path(
  post, path = "/rooms/joined/{roomName}/ticker", tag = "rooms", request_body = TickerText,
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 201, description = "set"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_ticker(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  Json(TickerText(ticker)): Json<TickerText>,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.set_ticker(&room, ticker).await?;
  Ok(StatusCode::CREATED)
}

/// The tickers in a room we are in.
#[utoipa::path(
  get, path = "/rooms/joined/{roomName}/tickers", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 200, body = Vec<RoomTicker>), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn tickers(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedRoom(room): NamedRoom,
) -> Result<Json<Vec<RoomTicker>>, ApiError> {
  app.services.messaging.tickers(&room).await.map(Json)
}

/// Adds a member to a private room we own or operate, the body the username as a JSON string.
#[utoipa::path(
  post, path = "/rooms/joined/{roomName}/members", tag = "rooms", request_body = RoomMemberName,
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 201, description = "added"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn add_member(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  Json(RoomMemberName(username)): Json<RoomMemberName>,
) -> Result<StatusCode, ApiError> {
  if username.is_empty() {
    return Err(ApiError::bad_request("the username is empty"));
  }
  app.services.messaging.add_member(&room, &username).await?;
  info!(
    by = caller.principal.subject,
    room, username, "room member added"
  );
  Ok(StatusCode::CREATED)
}

/// Takes a member out of a private room we own or operate.
#[utoipa::path(
  delete, path = "/rooms/joined/{roomName}/members/{username}", tag = "rooms",
  params(
    ("roomName" = String, Path, description = "the room"),
    ("username" = String, Path, description = "the member"),
    RoomNameQuery, UsernameQuery
  ),
  responses(
    (status = 204, description = "removed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn remove_member(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .messaging
    .remove_member(&room, &username)
    .await?;
  info!(
    by = caller.principal.subject,
    room, username, "room member removed"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Makes a member of a private room we own an operator, the body the username as a JSON
/// string.
#[utoipa::path(
  post, path = "/rooms/joined/{roomName}/operators", tag = "rooms", request_body = RoomMemberName,
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "made operator"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn add_operator(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  Json(RoomMemberName(username)): Json<RoomMemberName>,
) -> Result<StatusCode, ApiError> {
  if username.is_empty() {
    return Err(ApiError::bad_request("the username is empty"));
  }
  app
    .services
    .messaging
    .add_operator(&room, &username)
    .await?;
  info!(
    by = caller.principal.subject,
    room, username, "room operator added"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Takes a private room's operator back to member.
#[utoipa::path(
  delete, path = "/rooms/joined/{roomName}/operators/{username}", tag = "rooms",
  params(
    ("roomName" = String, Path, description = "the room"),
    ("username" = String, Path, description = "the operator"),
    RoomNameQuery, UsernameQuery
  ),
  responses(
    (status = 204, description = "no longer operator"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn remove_operator(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .messaging
    .remove_operator(&room, &username)
    .await?;
  info!(
    by = caller.principal.subject,
    room, username, "room operator removed"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Gives up our membership of a private room.
#[utoipa::path(
  delete, path = "/rooms/private/{roomName}/membership", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "membership given up"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn drop_membership(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.drop_membership(&room).await?;
  info!(
    by = caller.principal.subject,
    room, "private room membership given up"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Gives up our ownership of a private room.
#[utoipa::path(
  delete, path = "/rooms/private/{roomName}/ownership", tag = "rooms",
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "ownership given up"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn drop_ownership(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.drop_ownership(&room).await?;
  info!(
    by = caller.principal.subject,
    room, "private room ownership given up"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Marks a room read up to a message.
#[utoipa::path(
  put, path = "/rooms/joined/{roomName}/read", tag = "rooms", request_body = ReadMarker,
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "marked"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn mark_read(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  Json(marker): Json<ReadMarker>,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .messaging
    .mark_read(&room, marker.local_id)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// When a room notifies: every message, mentions only, or never.
#[utoipa::path(
  operation_id = "set_room_notify",
  put, path = "/rooms/joined/{roomName}/notify", tag = "rooms", request_body = NotifySetting,
  params(("roomName" = String, Path, description = "the room"), RoomNameQuery),
  responses(
    (status = 204, description = "saved"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_notify(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedRoom(room): NamedRoom,
  Json(setting): Json<NotifySetting>,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .messaging
    .set_room_notify(&room, setting.notify)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// The server's list of rooms, asked of it now: public rooms, then private ones we are a
/// member of, then those we own.
#[utoipa::path(
  get, path = "/rooms/available", tag = "rooms",
  responses(
    (status = 200, body = Vec<RoomInfo>), (status = 401, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn available(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<RoomInfo>>, ApiError> {
  app.services.messaging.available_rooms().await.map(Json)
}
