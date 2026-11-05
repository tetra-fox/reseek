//! Switching between the normal and the alternative speed limits.

use api_types::transfers::SpeedLimitsSwitch;
use axum::extract::State;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::Json;
use crate::app::AppState;
use crate::auth::{Authorized, ReadWrite};

/// The speed limits route.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new().routes(routes!(switch))
}

/// Puts the alternative speed limits in force, or the normal ones. The choice lasts across
/// restarts and shows as `alternativeSpeedLimits` in the state.
#[utoipa::path(
  put, path = "/transfers/speed-limits", tag = "transfers", request_body = SpeedLimitsSwitch,
  responses(
    (status = 204, description = "switched"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn switch(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(choice): Json<SpeedLimitsSwitch>,
) -> Result<StatusCode, ApiError> {
  crate::transfers::speed::switch(&app, choice.alternative, &caller.principal.subject).await?;
  Ok(StatusCode::NO_CONTENT)
}
