//! The groups: every group with how many users are in it, and moving or letting go all the
//! members of one, such as a group the configuration no longer has. Which groups exist and their
//! settings are options; who is in them is changed per user with `PUT /users/{username}/groups`.

use api_types::users::{GroupList, GroupMembersChanged, GroupMove};
use axum::extract::State;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::{Json, Path};
use crate::app::AppState;
use crate::auth::{AnyRole, Authorized, ReadWrite};

/// The groups routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(move_members))
    .routes(routes!(let_go))
}

/// Every group: privileged, the others in the order they go, then the names of groups the
/// configuration no longer has that still have members.
#[utoipa::path(
  operation_id = "list_groups",
  get, path = "/groups", tag = "users",
  responses(
    (status = 200, body = GroupList), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<GroupList>, ApiError> {
  app.services.users.groups().await.map(Json)
}

/// Moves every member of a group into a user-defined group of the configuration; a member of
/// both is kept once.
#[utoipa::path(
  post, path = "/groups/{name}/members/move", tag = "users", request_body = GroupMove,
  params(("name" = String, Path, description = "the group whose members move")),
  responses(
    (status = 200, body = GroupMembersChanged), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn move_members(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(name): Path<String>,
  Json(body): Json<GroupMove>,
) -> Result<Json<GroupMembersChanged>, ApiError> {
  let users = app.services.users.move_members(&name, &body.to).await?;
  info!(
    by = caller.principal.subject,
    from = name,
    to = body.to,
    users,
    "group moved"
  );
  Ok(Json(GroupMembersChanged { users }))
}

/// Takes every member out of a group.
#[utoipa::path(
  delete, path = "/groups/{name}/members", tag = "users",
  params(("name" = String, Path, description = "the group")),
  responses(
    (status = 200, body = GroupMembersChanged), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn let_go(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(name): Path<String>,
) -> Result<Json<GroupMembersChanged>, ApiError> {
  let users = app.services.users.let_go_members(&name).await?;
  info!(
    by = caller.principal.subject,
    group = name,
    users,
    "group members let go"
  );
  Ok(Json(GroupMembersChanged { users }))
}
