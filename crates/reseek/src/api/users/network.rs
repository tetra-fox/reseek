//! What the network says about a user: their address, browsing their shares, one of their
//! directories, their info, presence, figures and interests, and watching them.

use api_types::profile::Interests;
use api_types::users::{
  BrowseProgress, BrowseResponse, CachedUser, Directory, DirectoryRequest, StatisticsSnapshot,
  UserEndpoint, UserInfo, UserStatus, UsernameQuery,
};
use axum::extract::State;
use axum::http::StatusCode;
use tracing::debug;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::{Json, NamedUser};
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};

/// The network user routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(endpoint))
    .routes(routes!(browse))
    .routes(routes!(browse_status))
    .routes(routes!(directory))
    .routes(routes!(info))
    .routes(routes!(status))
    .routes(routes!(statistics))
    .routes(routes!(interests))
    .routes(routes!(watch, unwatch))
    .routes(routes!(watched))
    .routes(routes!(cache))
}

/// The address the server gives for a user.
#[utoipa::path(
  get, path = "/users/{username}/endpoint", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = UserEndpoint), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn endpoint(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<UserEndpoint>, ApiError> {
  app.services.users.endpoint(&username).await.map(Json)
}

/// Everything a user shares, asked of the user; the progress shows on
/// `/users/{username}/browse/status` and the event stream while it arrives.
#[utoipa::path(
  get, path = "/users/{username}/browse", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = BrowseResponse), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn browse(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<BrowseResponse>, ApiError> {
  app.services.users.browse(&username).await.map(Json)
}

/// How far the browse of a user got.
#[utoipa::path(
  get, path = "/users/{username}/browse/status", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = BrowseProgress), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn browse_status(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<BrowseProgress>, ApiError> {
  app.services.users.browse_status(&username).await.map(Json)
}

/// The contents of one of a user's directories, named exactly as their browse gave it. The
/// answer may hold its subdirectories as well.
#[utoipa::path(
  post, path = "/users/{username}/directory", tag = "users", request_body = DirectoryRequest,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = Vec<Directory>), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 502, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn directory(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
  Json(request): Json<DirectoryRequest>,
) -> Result<Json<Vec<Directory>>, ApiError> {
  if request.directory.is_empty() {
    return Err(ApiError::bad_request("A directory is required"));
  }
  app
    .services
    .users
    .directory(&username, request.directory)
    .await
    .map(Json)
}

/// A user's description, picture and upload slots, asked of the user.
#[utoipa::path(
  get, path = "/users/{username}/info", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = UserInfo), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn info(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<UserInfo>, ApiError> {
  app.services.users.info(&username).await.map(Json)
}

/// A user's presence, asked of the server now.
#[utoipa::path(
  get, path = "/users/{username}/status", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = UserStatus), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn status(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<UserStatus>, ApiError> {
  app.services.users.status(&username).await.map(Json)
}

/// A user's share and speed figures, asked of the server now.
#[utoipa::path(
  get, path = "/users/{username}/statistics", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = StatisticsSnapshot), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn statistics(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<StatisticsSnapshot>, ApiError> {
  app.services.users.statistics(&username).await.map(Json)
}

/// What a user likes and hates, asked of the server now.
#[utoipa::path(
  get, path = "/users/{username}/interests", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 200, body = Interests), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn interests(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  NamedUser(username): NamedUser,
) -> Result<Json<Interests>, ApiError> {
  app.services.users.interests(&username).await.map(Json)
}

/// Asks the server to report a user's presence and figures as they change.
#[utoipa::path(
  put, path = "/users/{username}/watch", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "watched"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn watch(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app.services.users.watch(&username).await?;
  debug!(by = caller.principal.subject, username, "user watched");
  Ok(StatusCode::NO_CONTENT)
}

/// Stops watching a user; refused while the user has uploads that have not completed.
#[utoipa::path(
  delete, path = "/users/{username}/watch", tag = "users",
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "no longer watched"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn unwatch(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
) -> Result<StatusCode, ApiError> {
  app.services.users.unwatch(&username).await?;
  debug!(
    by = caller.principal.subject,
    username, "user no longer watched"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// The users being watched.
#[utoipa::path(
  get, path = "/users/watched", tag = "users",
  responses(
    (status = 200, body = Vec<String>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn watched(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<String>>, ApiError> {
  app.services.users.watched().await.map(Json)
}

/// What is remembered of the users seen since the start.
#[utoipa::path(
  get, path = "/users/cache", tag = "users",
  responses(
    (status = 200, body = Vec<CachedUser>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn cache(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<CachedUser>>, ApiError> {
  app.services.users.cache().await.map(Json)
}
