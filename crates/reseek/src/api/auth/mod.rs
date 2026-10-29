//! The sign-in routes: local accounts and browser sessions, OpenID Connect providers, and API
//! tokens.

mod oidc;
mod session;
mod tokens;

use utoipa_axum::router::OpenApiRouter;

use crate::app::AppState;

/// The sign-in routes.
pub fn routes() -> OpenApiRouter<AppState> {
  session::routes()
    .merge(oidc::routes())
    .merge(tokens::routes())
}
