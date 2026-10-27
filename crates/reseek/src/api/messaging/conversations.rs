//! The conversations routes: listing conversations, reading one and its messages, sending,
//! acknowledging, closing, exporting, and a conversation's notifications and draft.

use api_types::chat::{ExportQuery, MessageText, NotifySetting};
use api_types::conversations::{
  CloseConversationQuery, Conversation, ConversationDraft, ConversationListQuery,
  ConversationMessagesQuery, ConversationQuery, PrivateMessage,
};
use api_types::users::UsernameQuery;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use tracing::{debug, info};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::message;
use crate::api::error::ApiError;
use crate::api::extract::{Id, Json, NamedUser, Query};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};

/// The most messages a page holds.
const PAGE_MAX: u32 = 1000;
/// The most bytes a draft holds.
const DRAFT_BYTES: usize = 4 * 1024;

/// The conversations routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(conversation, send, acknowledge_all, close))
    .routes(routes!(messages))
    .routes(routes!(acknowledge))
    .routes(routes!(export))
    .routes(routes!(set_notify))
    .routes(routes!(set_draft))
}

/// The conversations, without their messages, by username; the server's notices only with
/// `includeServer`.
#[utoipa::path(
  operation_id = "list_conversations",
  get, path = "/conversations", tag = "conversations", params(ConversationListQuery),
  responses(
    (status = 200, body = Vec<Conversation>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<ConversationListQuery>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
  app.services.messaging.conversations(query).await.map(Json)
}

/// One conversation, active or not, with its latest messages unless `includeMessages` is
/// false.
#[utoipa::path(
  get, path = "/conversations/{username}", tag = "conversations",
  params(("username" = String, Path, description = "the other user"), UsernameQuery, ConversationQuery),
  responses(
    (status = 200, body = Conversation), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn conversation(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
  Query(query): Query<ConversationQuery>,
) -> Result<Json<Conversation>, ApiError> {
  app
    .services
    .messaging
    .conversation(&username, query.include_messages)
    .await
    .map(Json)
}

/// A page of one conversation's messages, oldest first; `before` pages back from a message.
#[utoipa::path(
  operation_id = "conversation_messages",
  get, path = "/conversations/{username}/messages", tag = "conversations",
  params(
    ("username" = String, Path, description = "the other user"), UsernameQuery,
    ConversationMessagesQuery
  ),
  responses(
    (status = 200, body = Vec<PrivateMessage>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn messages(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
  Query(query): Query<ConversationMessagesQuery>,
) -> Result<Json<Vec<PrivateMessage>>, ApiError> {
  validate::limit("limit", query.limit, PAGE_MAX)?;
  app
    .services
    .messaging
    .conversation_messages(&username, query)
    .await
    .map(Json)
}

/// Sends a private message, the body a JSON string. The server keeps it for a user who is
/// offline.
#[utoipa::path(
  post, path = "/conversations/{username}", tag = "conversations", request_body = MessageText,
  params(("username" = String, Path, description = "the other user"), UsernameQuery),
  responses(
    (status = 201, description = "sent"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn send(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(MessageText(text)): Json<MessageText>,
) -> Result<StatusCode, ApiError> {
  if username == soulseek_proto::SERVER_USERNAME {
    return Err(ApiError::bad_request("\"server\" is a reserved name"));
  }
  message(&app, &text)?;
  app.services.messaging.send_private(&username, text).await?;
  debug!(
    by = caller.principal.subject,
    username, "private message sent"
  );
  Ok(StatusCode::CREATED)
}

/// Acknowledges every unacknowledged message from a user, so the server stops sending them
/// again.
#[utoipa::path(
  put, path = "/conversations/{username}", tag = "conversations",
  params(("username" = String, Path, description = "the other user"), UsernameQuery),
  responses(
    (status = 200, description = "acknowledged"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn acknowledge_all(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.acknowledge_all(&username).await?;
  Ok(StatusCode::OK)
}

/// Acknowledges one message from a user by the server's id for it.
#[utoipa::path(
  put, path = "/conversations/{username}/{id}", tag = "conversations",
  params(
    ("username" = String, Path, description = "the other user"),
    ("id" = u32, Path, description = "the server's id of the message"),
    UsernameQuery
  ),
  responses(
    (status = 200, description = "acknowledged"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn acknowledge(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Id(id): Id<u32>,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.acknowledge(&username, id).await?;
  Ok(StatusCode::OK)
}

/// Acknowledges a conversation and hides it until the next message; `purge` also deletes its
/// messages.
#[utoipa::path(
  delete, path = "/conversations/{username}", tag = "conversations",
  params(("username" = String, Path, description = "the other user"), UsernameQuery, CloseConversationQuery),
  responses(
    (status = 204, description = "closed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn close(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Query(query): Query<CloseConversationQuery>,
) -> Result<StatusCode, ApiError> {
  app.services.messaging.close(&username, query.purge).await?;
  info!(
    by = caller.principal.subject,
    username,
    purge = query.purge,
    "conversation closed"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// The whole conversation as a file to save, JSON or text.
#[utoipa::path(
  operation_id = "export_conversation",
  get, path = "/conversations/{username}/export", tag = "conversations",
  params(("username" = String, Path, description = "the other user"), UsernameQuery, ExportQuery),
  responses(
    (status = 200, content(
      (Vec<PrivateMessage> = "application/json"), (String = "text/plain")
    )),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn export(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
  Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
  app
    .services
    .messaging
    .export_conversation(&username, query.format)
    .await
}

/// When a conversation notifies: every message, mentions only, or never.
#[utoipa::path(
  operation_id = "set_conversation_notify",
  put, path = "/conversations/{username}/notify", tag = "conversations", request_body = NotifySetting,
  params(("username" = String, Path, description = "the other user"), UsernameQuery),
  responses(
    (status = 204, description = "saved"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_notify(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(setting): Json<NotifySetting>,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .messaging
    .set_conversation_notify(&username, setting.notify)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Keeps the unsent text of a conversation, up to 4 KiB.
#[utoipa::path(
  put, path = "/conversations/{username}/draft", tag = "conversations", request_body = ConversationDraft,
  params(("username" = String, Path, description = "the other user"), UsernameQuery),
  responses(
    (status = 204, description = "saved"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_draft(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(body): Json<ConversationDraft>,
) -> Result<StatusCode, ApiError> {
  validate::text_size("draft", &body.draft, DRAFT_BYTES)?;
  app
    .services
    .messaging
    .set_draft(&username, body.draft)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}
