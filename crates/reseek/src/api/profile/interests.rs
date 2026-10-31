//! What we like and hate: read by any role, changed by an administrator. They are no
//! configuration, so changing them does not take `remote_configuration`.

use api_types::profile::Interests;
use axum::extract::State;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::Json;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized};

/// The interests routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(get_interests, put_interests))
}

/// What we like and hate.
#[utoipa::path(
  get, path = "/interests", tag = "profile",
  responses((status = 200, body = Interests), (status = 401, body = api_types::ErrorBody))
)]
async fn get_interests(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Interests>, ApiError> {
  app.users.interests().await.map(Json)
}

/// Replaces what we like and hate and answers with the lists kept, trimmed and without repeats.
/// The server is told what changed, now when logged in and at the next login otherwise.
#[utoipa::path(
  put, path = "/interests", tag = "profile", request_body = Interests,
  responses(
    (status = 200, body = Interests), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody)
  )
)]
async fn put_interests(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Json(interests): Json<Interests>,
) -> Result<Json<Interests>, ApiError> {
  let kept = app.users.set_interests(interests).await?;
  info!(
    by = admin.principal.subject,
    liked = kept.liked.len(),
    hated = kept.hated.len(),
    "interests changed"
  );
  Ok(Json(kept))
}
