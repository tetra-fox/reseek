//! Our Soulseek privileges, read from the server now, and giving a user some of them. The server
//! takes the days given off what we have left and does not answer; a gift it refuses is
//! reported later in a private message from `server`.

use api_types::state::Privileges;
use api_types::users::{PrivilegesGift, UsernameQuery};
use axum::extract::State;
use axum::http::StatusCode;
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::{Json, NamedUser};
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized};

/// Seconds in one day of privileges.
const DAY: u64 = 86_400;

/// Refuses a call that needs the server while we are not logged in.
fn logged_in(app: &AppState) -> Result<(), ApiError> {
  if !app.network.client().state().is_logged_in() {
    return Err(ApiError::unavailable(Some(
      "Not logged in to the Soulseek server".to_string(),
    )));
  }
  Ok(())
}

/// The privileges routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(own_privileges))
    .routes(routes!(give_privileges))
}

/// Our privileges, asked of the server now; the state document takes them too.
#[utoipa::path(
  get, path = "/application/privileges", tag = "application",
  responses(
    (status = 200, body = Privileges), (status = 401, body = api_types::ErrorBody),
    (status = 502, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn own_privileges(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Privileges>, ApiError> {
  logged_in(&app)?;
  app
    .network
    .refresh_user_statistics(true)
    .await
    .map_err(|e| {
      ApiError::upstream(
        "The server did not say how many privileges are left",
        Some(e.to_string()),
      )
    })?;
  Ok(Json(app.state.current().user.privileges.clone()))
}

/// Gives a user whole days of our privileges. Refused for more days than we have left, asked of
/// the server first, and for ourselves; our own privileges are read again afterwards.
#[utoipa::path(
  post, path = "/users/{username}/privileges", tag = "users", request_body = PrivilegesGift,
  params(("username" = String, Path, description = "the user"), UsernameQuery),
  responses(
    (status = 204, description = "given"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn give_privileges(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  NamedUser(username): NamedUser,
  Json(gift): Json<PrivilegesGift>,
) -> Result<StatusCode, ApiError> {
  if gift.days == 0 {
    return Err(ApiError::bad_request("Give at least one day"));
  }
  logged_in(&app)?;
  let client = app.network.client();
  if client.username().as_deref() == Some(username.as_str()) {
    return Err(ApiError::conflict(
      "Privileges cannot be given to ourselves",
      None,
    ));
  }
  let left = client.get_privileges().await.map_err(|e| {
    ApiError::upstream(
      "The server did not say how many privileges are left",
      Some(e.to_string()),
    )
  })?;
  let days_left = left.as_secs() / DAY;
  if u64::from(gift.days) > days_left {
    return Err(ApiError::conflict(
      format!("Only {days_left} whole days of privileges are left to give"),
      None,
    ));
  }
  client
    .grant_user_privileges(&username, gift.days)
    .await
    .map_err(|e| ApiError::upstream("The privileges were not given", Some(e.to_string())))?;
  info!(
    by = admin.principal.subject,
    username,
    days = gift.days,
    "privileges given"
  );
  if let Err(e) = app.network.refresh_user_statistics(true).await {
    warn!(error = %e, "own privileges not read again after a gift");
  }
  Ok(StatusCode::NO_CONTENT)
}
