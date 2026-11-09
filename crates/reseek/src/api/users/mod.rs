//! The users routes: asking the network about a user, the directory of users we know with our
//! notes and groups for them, the groups themselves, and blocking. A username is the
//! `{username}` path segment, or `?username=` when given, which reaches names a path cannot hold
//! such as `..`.
//!
//! Reading takes the ReadOnly role and changing what we keep about a user, their groups
//! included, ReadWrite; forgetting a user, importing blocks and giving privileges take
//! Administrator.

mod blocking;
mod directory;
mod groups;
mod network;
mod privileges;

use utoipa_axum::router::OpenApiRouter;

use crate::app::AppState;

/// The users routes.
pub fn routes() -> OpenApiRouter<AppState> {
  network::routes()
    .merge(directory::routes())
    .merge(groups::routes())
    .merge(blocking::routes())
    .merge(privileges::routes())
}
