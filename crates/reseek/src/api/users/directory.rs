//! The users directory: every user we have dealt with, their profile and picture, our notes,
//! tags and mutes for them, our buddies, the upload group we put them in, their transfers, and
//! forgetting them.

use api_types::users::{
  BuddyChange, DeleteUserQuery, GroupAssignment, ProfileQuery, UserList, UserListQuery, UserNotes,
  UserProfile, UserSummary, UserTags, UserTransfers, UserTransfersQuery, UsernameQuery,
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
/// The most tags a user has.
const TAGS: usize = 32;
/// The most characters a tag has.
const TAG_CHARS: usize = 32;

/// The users directory routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list))
    .routes(routes!(summary))
    .routes(routes!(profile))
    .routes(routes!(picture))
    .routes(routes!(set_notes))
    .routes(routes!(set_tags))
    .routes(routes!(set_buddy, remove_buddy))
    .routes(routes!(mute, unmute))
    .routes(routes!(set_group))
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

/// Replaces our tags on a user: up to 32, each 1 to 32 of `a-z`, `0-9`, `_` and `-` once made
/// lower case.
#[utoipa::path(
  put, path = "/users/{username}/tags", tag = "users", request_body = UserTags,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "saved"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_tags(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  NamedUser(username): NamedUser,
  Json(body): Json<UserTags>,
) -> Result<StatusCode, ApiError> {
  let tags = tags(body.tags)?;
  app.services.users.set_tags(&username, tags).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// The tags made lower case, refused when there are too many or one is empty, too long or
/// holds anything but `a-z`, `0-9`, `_` and `-`.
fn tags(tags: Vec<String>) -> Result<Vec<String>, ApiError> {
  if tags.len() > TAGS {
    return Err(ApiError::bad_request("A user can have at most 32 tags"));
  }
  tags
    .into_iter()
    .map(|tag| {
      let tag = tag.to_lowercase();
      let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
      if tag.is_empty() || tag.chars().count() > TAG_CHARS || !tag.chars().all(allowed) {
        return Err(ApiError::invalid(
          "Invalid tag",
          format!("{tag:?}: A tag has 1 to 32 characters from a-z, 0-9, _ and -"),
        ));
      }
      Ok(tag)
    })
    .collect()
}

/// Makes a user our buddy, or changes whether they are trusted and whether we are told of their
/// presence; a field left out keeps its value, and an empty body makes a buddy with neither.
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

/// Puts a user in one of the configuration's user-defined upload groups, or with `null` takes
/// them out of the one they were put in. A user the configuration lists for another group stays
/// there and the call is refused.
#[utoipa::path(
  put, path = "/users/{username}/group", tag = "users", request_body = GroupAssignment,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "assigned"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn set_group(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  NamedUser(username): NamedUser,
  Json(body): Json<GroupAssignment>,
) -> Result<StatusCode, ApiError> {
  if let Some(group) = &body.group
    && !app
      .config
      .current()
      .values
      .transfers
      .groups
      .user_defined
      .contains_key(group)
  {
    return Err(ApiError::bad_request(format!(
      "{group} is not a user-defined group"
    )));
  }
  app
    .services
    .users
    .set_group(&username, body.group.clone())
    .await?;
  info!(
    by = admin.principal.subject,
    username,
    group = body.group,
    "user group assigned"
  );
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

/// Forgets a user and their block; `purge` also removes their transfers from the listings and
/// deletes the conversation with them.
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
  app.services.users.remove(&username, query.purge).await?;
  info!(
    by = admin.principal.subject,
    username,
    purge = query.purge,
    "user forgotten"
  );
  Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tags_are_lower_case_words_of_up_to_32_characters() {
    let words = |list: &[&str]| list.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    assert_eq!(
      tags(words(&["Jazz", "lo-fi", "a_1"])).unwrap(),
      ["jazz", "lo-fi", "a_1"]
    );
    assert!(tags(words(&[""])).is_err());
    assert!(tags(words(&["two words"])).is_err());
    assert!(tags(words(&["caf\u{e9}"])).is_err());
    assert!(tags(words(&[&"x".repeat(33)])).is_err());
    assert!(tags(words(&[&"x".repeat(32)])).is_ok());
    assert!(tags(vec!["t".to_string(); 33]).is_err());
  }
}
