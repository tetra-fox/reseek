//! The users routes: asking the network about a user, the directory of users we know with our
//! notes, tags and groups for them, and blocking. A username is the `{username}` path segment,
//! or `?username=` when given, which reaches names a path cannot hold such as `..`.
//!
//! Reading takes the ReadOnly role and changing what we keep about a user ReadWrite; putting a
//! user in a group, forgetting a user, importing blocks and giving privileges take
//! Administrator.

mod blocking;
mod directory;
mod network;
mod privileges;

use utoipa_axum::router::OpenApiRouter;

use crate::app::AppState;

/// The users routes.
pub fn routes() -> OpenApiRouter<AppState> {
  network::routes()
    .merge(directory::routes())
    .merge(blocking::routes())
    .merge(privileges::routes())
}
