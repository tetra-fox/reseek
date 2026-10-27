//! The HTTP API under `/api/v0`: the routes, their OpenAPI document, and the error type every
//! handler answers failures with.

pub mod agent;
mod analysis;
mod application;
pub mod error;
mod events;
pub mod extract;
mod files;
mod live;
mod messaging;
mod oidc;
mod options;
mod relay;
mod searches;
mod session;
mod shares;
mod telemetry;
mod tokens;
mod transfers;
mod users;
mod validate;

use axum::Router;
use axum::middleware;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::{Config, SwaggerUi};

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
  // enums that only query parameters name, which the handlers' descriptions refer to without
  // listing
  components(schemas(
    api_types::chat::ExportFormat,
    api_types::chat::MessageSearchKind,
    api_types::files::ContentDisposition,
    api_types::files::FileRoot,
    api_types::reports::LeaderboardSort,
    api_types::transfers::SortOrder,
    api_types::transfers::TransferSort,
    api_types::transfers::TransferStateCategory,
    api_types::transfers::TransferStateFilter,
    api_types::users::UserSort
  )),
  tags(
    (name = "application", description = "The daemon's state, version, restart and shutdown"),
    (name = "server", description = "The connection to the Soulseek server"),
    (name = "logs", description = "The daemon's log"),
    (name = "options", description = "The configuration and its files"),
    (name = "session", description = "Signing in and out, and browser sessions"),
    (name = "auth", description = "Signing in with an OpenID Connect provider"),
    (name = "tokens", description = "API tokens for scripts"),
    (name = "live", description = "The event stream"),
    (name = "searches", description = "Searches we start on the network"),
    (name = "transfers", description = "Downloads and uploads"),
    (name = "users", description = "Other users and the directory of users we know"),
    (name = "blocking", description = "Blocked users"),
    (name = "conversations", description = "Private messages"),
    (name = "rooms", description = "Chat rooms and their history"),
    (name = "messages", description = "Searching what was said"),
    (name = "shares", description = "What we share"),
    (name = "files", description = "The downloaded files, served to the browser"),
    (name = "analysis", description = "Checking whether a downloaded file is what it claims"),
    (name = "relay", description = "Relay controller and agents"),
    (name = "events", description = "The event log"),
    (name = "telemetry", description = "Metrics and reports over the transfer history")
  )
)]
struct ApiDoc;

/// Every route with its OpenAPI description.
fn routes() -> OpenApiRouter<AppState> {
  let api = OpenApiRouter::new()
    .merge(application::routes())
    .merge(searches::routes())
    .merge(transfers::routes())
    .merge(users::routes())
    .merge(messaging::routes())
    .merge(shares::routes())
    .merge(files::routes())
    .merge(analysis::routes())
    .merge(relay::routes())
    .merge(events::routes())
    .merge(telemetry::routes())
    .merge(options::routes())
    .merge(session::routes())
    .merge(oidc::routes())
    .merge(tokens::routes())
    .merge(live::routes());
  OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api/v0", api)
}

/// The API's routes for `state`, the diagnostic ones the document leaves out included, each
/// request authenticated first.
pub fn router(state: AppState) -> Router {
  let (router, _) = routes().split_for_parts();
  router
    .nest("/api/v0", application::hidden())
    .route_layer(middleware::from_fn_with_state(
      state.clone(),
      agent::refuse_in_agent_mode,
    ))
    .layer(middleware::from_fn_with_state(
      state.clone(),
      auth::authenticate,
    ))
    .with_state(state)
}

/// The OpenAPI document at `/swagger/v0/swagger.json` and the Swagger UI at `/swagger`, which
/// reads the document from under `url_base`.
pub fn swagger(url_base: &str) -> Router {
  let document = format!("{url_base}/swagger/v0/swagger.json");
  SwaggerUi::new("/swagger")
    .url("/swagger/v0/swagger.json", openapi())
    .config(Config::new([document]))
    .into()
}

/// The OpenAPI document of every route.
pub fn openapi() -> utoipa::openapi::OpenApi {
  routes().split_for_parts().1
}

#[cfg(test)]
mod tests;
