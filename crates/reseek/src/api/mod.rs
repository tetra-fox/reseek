//! The HTTP API under `/api/v0`: the routes, their OpenAPI document, and the error type every
//! handler answers failures with.

mod activity;
mod analysis;
mod application;
mod auth;
pub mod error;
mod events;
pub mod extract;
mod files;
mod jobs;
mod live;
mod messaging;
mod options;
mod profile;
mod relay;
mod searches;
mod shares;
mod telemetry;
mod transfers;
mod users;
mod validate;

use axum::Router;
use axum::middleware;
use utoipa::OpenApi;
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{AdditionalProperties, ArrayItems, Schema, SchemaType, Type};
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::{Config, SwaggerUi};

use crate::app::AppState;

/// The OpenAPI document's description of the API as a whole.
#[derive(OpenApi)]
#[openapi(
  // utoipa leaves out empty response descriptions, which 3.1 requires and 3.2 does not
  version = "3.2.0",
  info(
    title = "reseek",
    description = "A Soulseek client daemon: its configuration, transfers, searches, chat and \
                   shares.",
    license(name = "AGPL-3.0-only", identifier = "AGPL-3.0-only")
  ),
  // enums that only query parameters name, which the handlers' descriptions refer to without
  // listing, and the headers of the binary analysis answers
  components(schemas(
    api_types::analysis::SpectrogramTile,
    api_types::analysis::WaveformTile,
    api_types::analysis::WindowSpectra,
    api_types::messaging::ExportFormat,
    api_types::messaging::MessageSearchKind,
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
    (name = "profile", description = "Our own profile, as other users see it"),
    (name = "session", description = "Signing in and out, and browser sessions"),
    (name = "auth", description = "Signing in with an OpenID Connect provider"),
    (name = "tokens", description = "API tokens for scripts"),
    (name = "live", description = "The event stream"),
    (name = "searches", description = "Searches we start on the network"),
    (name = "transfers", description = "Downloads and uploads"),
    (name = "jobs", description = "Downloads, share scans, analyses and maintenance as jobs, and the schedules"),
    (name = "users", description = "Other users and the directory of users we know"),
    (name = "blocking", description = "Blocked users"),
    (name = "conversations", description = "Private messages"),
    (name = "rooms", description = "Chat rooms and their history"),
    (name = "chat", description = "The chat list, across rooms and conversations"),
    (name = "messages", description = "Searching what was said"),
    (name = "shares", description = "What we share"),
    (name = "files", description = "The downloaded files, served to the browser"),
    (name = "analysis", description = "Checking whether a downloaded file is what it claims"),
    (name = "relay", description = "Relay controller and agents"),
    (name = "events", description = "The event log"),
    (name = "activity", description = "What other users did with us"),
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
    .merge(jobs::routes())
    .merge(users::routes())
    .merge(messaging::routes())
    .merge(shares::routes())
    .merge(files::routes())
    .merge(analysis::routes())
    .merge(relay::routes())
    .merge(events::routes())
    .merge(activity::routes())
    .merge(telemetry::routes())
    .merge(options::routes())
    .merge(profile::routes())
    .merge(auth::routes())
    .merge(live::routes());
  OpenApiRouter::with_openapi(ApiDoc::openapi()).nest("/api/v0", api)
}

/// The API's routes for `state`, the diagnostic ones the document leaves out included, each
/// request authenticated first.
pub fn router(state: AppState) -> Router {
  let (router, _) = routes().split_for_parts();
  router
    .nest("/api/v0", application::hidden())
    .layer(middleware::from_fn_with_state(
      state.clone(),
      crate::auth::authenticate,
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
  let mut document = routes().split_for_parts().1;
  let schemas = document
    .components
    .iter_mut()
    .flat_map(|c| c.schemas.values_mut());
  schemas.for_each(lift_nullable_descriptions);
  document
}

/// Moves the description of every nullable reference in `schema` from its reference arm up to
/// its `oneOf`. utoipa writes an optional field of a named type as `oneOf: [{$ref, description},
/// {type: null}]`, and generated clients read a field's description only from the `oneOf`.
fn lift_nullable_descriptions(schema: &mut RefOr<Schema>) {
  let RefOr::T(schema) = schema else { return };
  match schema {
    Schema::Object(object) => {
      object
        .properties
        .values_mut()
        .for_each(lift_nullable_descriptions);
      if let Some(AdditionalProperties::RefOr(values)) = object.additional_properties.as_deref_mut()
      {
        lift_nullable_descriptions(values);
      }
    }
    Schema::Array(array) => {
      if let ArrayItems::RefOrSchema(items) = &mut array.items {
        lift_nullable_descriptions(items);
      }
    }
    Schema::AllOf(all) => all.items.iter_mut().for_each(lift_nullable_descriptions),
    Schema::AnyOf(any) => any.items.iter_mut().for_each(lift_nullable_descriptions),
    Schema::OneOf(one) => {
      one.items.iter_mut().for_each(lift_nullable_descriptions);
      let null = |item: &RefOr<Schema>| match item {
        RefOr::T(Schema::Object(o)) => o.schema_type == SchemaType::Type(Type::Null),
        _ => false,
      };
      if let [RefOr::Ref(reference), other] | [other, RefOr::Ref(reference)] = &mut one.items[..]
        && null(other)
        && one.description.is_none()
        && !reference.description.is_empty()
      {
        one.description = Some(std::mem::take(&mut reference.description));
      }
    }
    // `Schema` is non-exhaustive
    _ => {}
  }
}

#[cfg(test)]
mod tests;
