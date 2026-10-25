//! The application routes: the state document, the version and the release check, shutting
//! down, the connection to the Soulseek server, and the log. Diagnostics that
//! the API document leaves out are here too.

use api_types::application::{DisconnectReason, VersionLatestQuery};
use api_types::auth::Role;
use api_types::files::ContentQuery;
use api_types::logs::LogRecord;
use api_types::state::{ServerState, State as StateDocument, VersionInfo};
use api_types::transfers::DebugTransfers;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::files::content_request;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, ReadWrite, require_interactive};
use crate::services::Connected;

/// The application routes in the API document.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(get_state, shut_down))
    .routes(routes!(get_version))
    .routes(routes!(get_latest_version))
    .routes(routes!(get_server, connect, disconnect))
    .routes(routes!(get_live_logs))
    .routes(routes!(get_log_files))
    .routes(routes!(get_log_file))
}

/// The diagnostic routes, which the API document leaves out.
pub fn hidden() -> Router<AppState> {
  Router::new()
    .route("/application/gc", post(collect_garbage))
    .route("/application/dump", get(dump))
    .route("/debug/transfers", get(debug_transfers))
}

/// The application state: the version, the server connection, our user, shares, rooms and
/// what waits for a restart or a reconnect.
#[utoipa::path(
  get, path = "/application", tag = "application",
  responses((status = 200, body = StateDocument), (status = 401, body = api_types::ErrorBody))
)]
async fn get_state(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Json<StateDocument> {
  Json(StateDocument::clone(&app.state.current()))
}

/// Stops the daemon in order and exits. Needs someone at the UI, not an API key.
#[utoipa::path(
  delete, path = "/application", tag = "application",
  responses(
    (status = 204, description = "stopping"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn shut_down(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
) -> Result<StatusCode, ApiError> {
  require_interactive(&admin.principal)?;
  info!(by = admin.principal.subject, "shutdown asked for");
  app.shutdown.cancel();
  Ok(StatusCode::NO_CONTENT)
}

/// The running version, such as `"0.1.0"`.
#[utoipa::path(
  get, path = "/application/version", tag = "application",
  responses((status = 200, body = String), (status = 401, body = api_types::ErrorBody))
)]
async fn get_version(State(app): State<AppState>, _caller: Authorized<AnyRole>) -> Json<String> {
  Json(app.state.current().version.current.clone())
}

/// The running version and the newest release the last check found. `forceCheck` checks now,
/// which takes the ReadWrite role.
#[utoipa::path(
  get, path = "/application/version/latest", tag = "application", params(VersionLatestQuery),
  responses(
    (status = 200, body = VersionInfo), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn get_latest_version(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Query(query): Query<VersionLatestQuery>,
) -> Result<Json<VersionInfo>, ApiError> {
  if !query.force_check {
    return Ok(Json(app.state.current().version.clone()));
  }
  if caller.principal.role < Role::ReadWrite {
    return Err(ApiError::forbidden(
      "checking for a release now needs the ReadWrite role",
    ));
  }
  app.services.application.check_version().await.map(Json)
}

/// The connection to the Soulseek server.
#[utoipa::path(
  get, path = "/server", tag = "server",
  responses((status = 200, body = ServerState), (status = 401, body = api_types::ErrorBody))
)]
async fn get_server(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Json<ServerState> {
  Json(app.state.current().server.clone())
}

/// Connects to the Soulseek server and keeps reconnecting. Answered 205 when the client was
/// already trying, which makes it try again at once.
#[utoipa::path(
  put, path = "/server", tag = "server",
  responses(
    (status = 200, description = "connecting"),
    (status = 205, description = "was trying already; trying again now"),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn connect(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
) -> Result<StatusCode, ApiError> {
  let connected = app.services.application.connect().await?;
  info!(
    by = caller.principal.subject,
    ?connected,
    "connect asked for"
  );
  Ok(match connected {
    Connected::Started => StatusCode::OK,
    Connected::Restarted => StatusCode::RESET_CONTENT,
  })
}

/// Disconnects from the Soulseek server and stops reconnecting. The body, a JSON string, says
/// why.
#[utoipa::path(
  delete, path = "/server", tag = "server", request_body = Option<DisconnectReason>,
  responses(
    (status = 204, description = "disconnected"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn disconnect(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  reason: Option<Json<DisconnectReason>>,
) -> Result<StatusCode, ApiError> {
  let reason = reason.map(|Json(DisconnectReason(reason))| reason);
  app.services.application.disconnect(reason).await?;
  info!(by = caller.principal.subject, "disconnect asked for");
  Ok(StatusCode::NO_CONTENT)
}

/// The latest log records kept in memory, oldest first.
#[utoipa::path(
  get, path = "/logs/live", tag = "logs",
  responses(
    (status = 200, body = Vec<LogRecord>), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn get_live_logs(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
) -> Json<Vec<LogRecord>> {
  Json(app.logs.snapshot())
}

/// The log files.
#[utoipa::path(
  get, path = "/logs/files", tag = "logs",
  responses(
    (status = 200, body = Vec<api_types::files::FilesystemFile>),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn get_log_files(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
) -> Result<Json<Vec<api_types::files::FilesystemFile>>, ApiError> {
  app.services.files.log_files().await.map(Json)
}

/// One log file's text, ranges honoured. The name is a bare file name ending in `.log`.
#[utoipa::path(
  get, path = "/logs/files/{filename}", tag = "logs",
  params(("filename" = String, Path, description = "the log file's name")),
  responses(
    (status = 200, content_type = "text/plain", body = String),
    (status = 206, content_type = "text/plain", body = String),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn get_log_file(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Path(filename): Path<String>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  log_file_name(&filename)?;
  let request = content_request(
    &method,
    headers,
    Some(admin.principal),
    ContentQuery::default(),
  );
  app.services.files.log_file(&filename, request).await
}

/// Refuses a log file name that is blank, holds a separator or a character no file name may
/// hold, or does not end in `.log`.
fn log_file_name(name: &str) -> Result<(), ApiError> {
  if name.trim().is_empty() {
    return Err(ApiError::bad_request("Filename is required"));
  }
  if name.contains(['/', '\\']) {
    return Err(ApiError::bad_request("Filename must not contain a path"));
  }
  let invalid = |c: char| {
    c == '\0'
      || (cfg!(windows) && (c.is_control() || matches!(c, '"' | '<' | '>' | '|' | ':' | '*' | '?')))
  };
  if name.chars().any(invalid) {
    return Err(ApiError::bad_request(
      "Filename contains one or more invalid characters",
    ));
  }
  if !name.to_ascii_lowercase().ends_with(".log") {
    return Err(ApiError::forbidden("Only .log files may be retrieved"));
  }
  Ok(())
}

/// Answers a request to collect garbage, which a program without a collector has nothing to do
/// for.
async fn collect_garbage(_admin: Authorized<Administrator>) -> StatusCode {
  StatusCode::OK
}

/// Refuses a memory dump; the daemon does not write them.
async fn dump(_admin: Authorized<Administrator>) -> ApiError {
  warn!("memory dump asked for; dumps are not written");
  ApiError::forbidden("memory dumps are not available").logged()
}

/// The Soulseek client's own records of its transfers.
async fn debug_transfers(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
) -> Result<Json<DebugTransfers>, ApiError> {
  app.services.transfers.client_transfers().await.map(Json)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{app, request};

  /// Loopback is a trusted network, so it is an administrator at the UI; the key is not.
  const ADMIN: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                       [web.authentication.api_keys.robot]\nkey = \"robot-key-0123456789\"\n\
                       role = \"administrator\"\n";

  #[tokio::test]
  async fn a_shutdown_stops_the_daemon_for_good() {
    let app = app(ADMIN, &[]).await;
    let answer = app
      .send(request("DELETE", "/api/v0/application", None))
      .await;
    assert_eq!(answer.status, 204);
    assert!(app.state.shutdown.is_cancelled());
  }

  #[test]
  fn log_file_names_are_bare_log_files() {
    assert!(log_file_name("reseek-20261025.log").is_ok());
    assert!(log_file_name("Upper.LOG").is_ok());
    for (name, status) in [
      ("", 400),
      ("  ", 400),
      ("../reseek.log", 400),
      ("dir\\x.log", 400),
      ("a\0.log", 400),
      ("reseek.txt", 403),
    ] {
      assert_eq!(
        log_file_name(name).unwrap_err().status(),
        status,
        "{name:?}"
      );
    }
  }
}
