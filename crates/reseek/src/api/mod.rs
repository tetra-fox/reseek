//! The HTTP API under `/api/v0`: the routes, their OpenAPI document, and the error type every
//! handler answers failures with.

pub mod error;
mod options;

use axum::Router;
use axum::middleware;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::app::AppState;
use crate::auth;

/// The OpenAPI document's description of the API as a whole.
#[derive(OpenApi)]
#[openapi(
  info(
    title = "reseek",
    description = "A Soulseek client daemon: its configuration, transfers, searches, chat and \
                   shares.",
    license(name = "AGPL-3.0-only", identifier = "AGPL-3.0-only")
  ),
  tags((name = "options", description = "The configuration and its files"))
)]
struct ApiDoc;

/// Every route with its OpenAPI description.
fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api/v0", options::routes())
}

/// The API's routes for `state`, each request authenticated first.
pub fn router(state: AppState) -> Router {
  let (router, _) = routes().split_for_parts();
  router
    .layer(middleware::from_fn_with_state(
      state.clone(),
      auth::authenticate,
    ))
    .with_state(state)
}

/// The OpenAPI document of every route.
pub fn openapi() -> utoipa::openapi::OpenApi {
  routes().split_for_parts().1
}

#[cfg(test)]
mod tests {
  #[test]
  fn the_committed_openapi_document_is_current() {
    let generated = super::openapi().to_pretty_json().unwrap() + "\n";
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../api/openapi.json");
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
      committed == generated,
      "api/openapi.json does not match the handlers; run `just api-spec`"
    );
  }
}
