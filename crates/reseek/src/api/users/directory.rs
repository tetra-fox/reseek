//! The users directory: every user we have dealt with, their profile and picture, our notes,
//! mutes and groups for them, our buddies, their transfers, and forgetting them.

use api_types::users::{
  BuddyChange, DeleteUserQuery, ProfileQuery, UserGroups, UserList, UserListQuery, UserNotes,
  UserProfile, UserSummary, UserTransfers, UserTransfersQuery, UsernameQuery,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::{Json, NamedUser, Query};
use crate::api::validate;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, ReadWrite};

/// The most users a page of the directory holds.
const PAGE_MAX: u32 = 500;
/// The most bytes our notes on a user hold.
const NOTES_BYTES: usize = 64 * 1024;
/// The most groups we put a user in.
const GROUPS: usize = 64;

/// The users directory routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(summary))
    .routes(routes!(profile))
    .routes(routes!(picture))
    .routes(routes!(set_notes))
    .routes(routes!(set_groups))
    .routes(routes!(set_buddy, remove_buddy))
    .routes(routes!(mute, unmute))
    .routes(routes!(transfers))
    .routes(routes!(remove))
}

/// A page of the users we know, filtered and sorted.
#[utoipa::path(
  operation_id = "list_users",
  get, path = "/users", tag = "users", params(UserListQuery),
  responses(
    (status = 200, body = UserList), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Query(query): Query<UserListQuery>,
) -> Result<Json<UserList>, ApiError> {
  validate::limit("limit", query.limit, PAGE_MAX)?;
  app.services.users.list(query).await.map(Json)
}

/// What we keep about a user, without asking the network; `known` is false for a user we have
/// not dealt with.
#[utoipa::path(
  operation_id = "user_summary",
  get, path = "/users/{username}/summary", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = UserSummary), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn summary(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<UserSummary>, ApiError> {
  app.services.users.summary(&username).await.map(Json)
}

/// A user's profile; `refresh` asks the network first.
#[utoipa::path(
  get, path = "/users/{username}/profile", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery, ProfileQuery),
  responses(
    (status = 200, body = UserProfile), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn profile(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
  Query(query): Query<ProfileQuery>,
) -> Result<Json<UserProfile>, ApiError> {
  app
    .services
    .users
    .profile(&username, query.refresh)
    .await
    .map(Json)
}

/// A user's picture as they last sent it.
#[utoipa::path(
  get, path = "/users/{username}/picture", tag = "users",
  params(
    ("username" = String, Path, description = "the user"), UsernameQuery,
    api_types::profile::PictureVersion
  ),
  responses(
    (status = 200, content_type = "image/*", body = api_types::files::FileContent),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn picture(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Response, ApiError> {
  app.services.users.picture(&username).await
}

/// Replaces our notes on a user, up to 64 KiB.
#[utoipa::path(
  put, path = "/users/{username}/notes", tag = "users", request_body = UserNotes,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "saved"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_notes(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(body): Json<UserNotes>,
) -> Result<StatusCode, ApiError> {
  validate::text_size("notes", &body.notes, NOTES_BYTES)?;
  app.services.users.set_notes(&username, body.notes).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Puts a user in exactly the groups named, up to 64: user-defined groups of the configuration,
/// and any the user is in already that it no longer has. An empty list takes them out of every
/// group we put them in.
#[utoipa::path(
  put, path = "/users/{username}/groups", tag = "users", request_body = UserGroups,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "saved"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_groups(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(body): Json<UserGroups>,
) -> Result<StatusCode, ApiError> {
  if body.groups.len() > GROUPS {
    return Err(ApiError::bad_request("A user can be in at most 64 groups"));
  }
  app
    .services
    .users
    .set_groups(&username, body.groups.clone())
    .await?;
  info!(
    by = caller.principal.subject,
    username,
    groups = ?body.groups,
    "user groups set"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Makes a user our buddy, or changes whether we are told of their presence; a field left out
/// keeps its value, and an empty body makes a buddy who is not.
/// Our own account and the server's notices cannot be a buddy.
#[utoipa::path(
  put, path = "/users/{username}/buddy", tag = "users", request_body = Option<BuddyChange>,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "a buddy"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_buddy(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  change: Option<Json<BuddyChange>>,
) -> Result<StatusCode, ApiError> {
  let change = change.map(|Json(change)| change).unwrap_or_default();
  app.services.users.set_buddy(&username, change).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Takes a user off our buddy list.
#[utoipa::path(
  delete, path = "/users/{username}/buddy", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "no longer a buddy"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn remove_buddy(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app.services.users.remove_buddy(&username).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Mutes a user's notifications.
#[utoipa::path(
  put, path = "/users/{username}/mute", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "muted"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn mute(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app.services.users.set_muted(&username, true).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Unmutes a user.
#[utoipa::path(
  delete, path = "/users/{username}/mute", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "unmuted"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn unmute(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app.services.users.set_muted(&username, false).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// A page of a user's transfers, newest first, removed ones included.
#[utoipa::path(
  get, path = "/users/{username}/transfers", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery, UserTransfersQuery),
  responses(
    (status = 200, body = UserTransfers), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn transfers(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
  Query(query): Query<UserTransfersQuery>,
) -> Result<Json<UserTransfers>, ApiError> {
  validate::positive("limit", query.limit)?;
  app
    .services
    .users
    .transfers(&username, query)
    .await
    .map(Json)
}

/// Forgets a user and their block; `purge` also cancels their transfers that have not finished,
/// removes their transfers from the listings and deletes the conversation with them. The 204
/// comes once every unfinished transfer was told to stop: one no task runs is ended and removed
/// by then, a running one leaves the listings when it has written its end.
#[utoipa::path(
  delete, path = "/users/{username}", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery, DeleteUserQuery),
  responses(
    (status = 204, description = "forgotten"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn remove(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  NamedUser(username): NamedUser,
  Query(query): Query<DeleteUserQuery>,
) -> Result<StatusCode, ApiError> {
  let purge = query.purge;
  let forget = {
    let (app, name) = (app.clone(), username.clone());
    async move {
      if purge {
        // first, so their removals are pushed, and a forget that fails after them can be
        // asked again
        app.downloads.purge(&name).await?;
        app.uploads.purge(&name).await?;
      }
      app.services.users.remove(&name, purge).await
    }
  };
  app.downloads.forget_user(&username, forget).await?;
  info!(
    by = admin.principal.subject,
    username,
    purge = query.purge,
    "user forgotten"
  );
  Ok(StatusCode::NO_CONTENT)
}
