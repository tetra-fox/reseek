//! Changing our Soulseek account's password: the server is asked first, and once it took the new
//! password the option is written where the settings page would write it, so the next login
//! uses it. Whatever would keep the option from being written refuses the change beforehand.

use api_types::profile::PasswordChange;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::Value;
use soulseek_client::ClientState;
use tracing::{error, info};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::{target, write};
use crate::api::error::ApiError;
use crate::api::extract::Json;
use crate::api::options::remote;
use crate::app::AppState;
use crate::auth::{Administrator, Authorized};

/// The option that holds the password.
const PASSWORD: &str = "soulseek.password";

/// The password route.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(change_password))
}

/// Changes the password of our Soulseek account. Takes being logged in; refused when the
/// environment or the command line sets the password, since the next login would use that one.
#[utoipa::path(
  put, path = "/application/password", tag = "application", request_body = PasswordChange,
  responses(
    (status = 204, description = "changed"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn change_password(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Json(change): Json<PasswordChange>,
) -> Result<StatusCode, ApiError> {
  remote(
    &app.config.current(),
    "Blocked password change; remote configuration is disabled",
  )?;
  if change.password.trim().is_empty() {
    return Err(ApiError::bad_request("The password may not be blank"));
  }
  let client = app.network.client();
  if client.state() != ClientState::READY {
    return Err(ApiError::unavailable(Some(
      "The password can be changed while logged in to the Soulseek server".to_string(),
    )));
  }
  let _writing = app.profile_writes.lock().await;
  let target = target(&app.config.current(), &admin.principal, PASSWORD, false)?;
  client
    .change_password(&change.password)
    .await
    .map_err(|e| ApiError::upstream("The server did not change the password", Some(e.to_string())))?;
  let password = Value::String(change.password);
  write(&app, &admin.principal, PASSWORD, target, Some(password))
    .await
    .map_err(|e| {
      let e = ApiError::from(e);
      // the account has the new password and the next login will try the old one
      error!(
        error = e.message(),
        "the server took the new password but it was not saved; set soulseek.password by hand"
      );
      ApiError::conflict(
        "The server took the new password, but it was not saved",
        Some(e.message().to_string()),
      )
      .logged()
    })?;
  info!(
    by = admin.principal.subject,
    "Soulseek account password changed"
  );
  Ok(StatusCode::NO_CONTENT)
}
