//! The application routes: the state document, the version and the release check, shutting
//! down and restarting, the connection to the Soulseek server, our away status, and the log.
//! Diagnostics that the API document leaves out are here too.

use api_types::application::{DatabaseFile, DisconnectReason, StatusChange, VersionLatestQuery};
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
    .routes(routes!(get_state, shut_down, restart))
    .routes(routes!(get_version))
    .routes(routes!(get_latest_version))
    .routes(routes!(get_server, connect, disconnect))
    .routes(routes!(set_status))
    .routes(routes!(get_live_logs))
    .routes(routes!(get_log_files))
    .routes(routes!(get_log_file))
    .routes(routes!(get_databases))
    .routes(routes!(vacuum_database))
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

/// Stops the daemon in order and exits so it starts again: with code 75 for a supervisor such
/// as systemd to restart it, or by starting itself again when it runs unsupervised. Needs
/// someone at the UI, not an API key.
#[utoipa::path(
  put, path = "/application", tag = "application",
  responses(
    (status = 204, description = "restarting"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn restart(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
) -> Result<StatusCode, ApiError> {
  require_interactive(&admin.principal)?;
  info!(by = admin.principal.subject, "restart asked for");
  app.request_restart();
  Ok(StatusCode::NO_CONTENT)
}

/// The running version, such as `"0.1.0"`.
#[utoipa::path(
  get, path = "/application/version", tag = "application",
  responses((status = 200, body = String, content_type = "application/json"),
    (status = 401, body = api_types::ErrorBody)
  )
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
    (status = 403, body = api_types::ErrorBody), (status = 502, body = api_types::ErrorBody)
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
      "Checking for a release requires the ReadWrite role",
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
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody)
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
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody)
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

/// Sets us online or away, as other users see us; away lasts until it is set back. The server
/// is told at once while logged in, else at the next login.
#[utoipa::path(
  put, path = "/application/status", tag = "application", request_body = StatusChange,
  responses(
    (status = 204, description = "set"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody)
  )
)]
async fn set_status(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(change): Json<StatusChange>,
) -> Result<StatusCode, ApiError> {
  app
    .services
    .messaging
    .set_status(change.status, &caller.principal.subject)
    .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// The database files with their sizes on disk; empty while the databases are in memory.
#[utoipa::path(
  get, path = "/application/data", tag = "application",
  responses(
    (status = 200, body = Vec<DatabaseFile>), (status = 401, body = api_types::ErrorBody)
  )
)]
async fn get_databases(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<DatabaseFile>>, ApiError> {
  app.services.application.databases().await.map(Json)
}

/// Rebuilds a database file without its free pages, then empties its write-ahead log into
/// it, as a maintenance job of its own; answers once the job has ended.
#[utoipa::path(
  post, path = "/application/data/{name}/vacuum", tag = "application",
  params(("name" = String, Path, description = "the database's name, as the listing gives it")),
  responses(
    (status = 204, description = "vacuumed"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody)
  )
)]
async fn vacuum_database(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
  Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
  crate::jobs::maintenance::vacuum_one(&app, &name).await?;
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

/// One log file's text, ranges honored. The name is a bare file name ending in `.log`.
#[utoipa::path(
  get, path = "/logs/files/{filename}", tag = "logs",
  params(("filename" = String, Path, description = "the log file's name")),
  responses(
    (status = 200, content_type = "text/plain", body = String),
    (status = 206, content_type = "text/plain", body = String),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 416, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
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
  let invalid = crate::safety::Os::CURRENT.invalid_filename_chars();
  if name.chars().any(|c| invalid.contains(&c)) {
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
  ApiError::forbidden("Memory dumps are not available").logged()
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
  use crate::testing::{OUTSIDE, app, request, with};

  /// Loopback is a trusted network, so it is an administrator at the UI; the key is not.
  const ADMIN: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                       [web.authentication.api_keys.robot]\nkey = \"robot-key-0123456789\"\n\
                       role = \"administrator\"\n";

  #[tokio::test]
  async fn a_restart_stops_the_daemon_to_start_again_and_needs_someone_at_the_ui() {
    let app = app(ADMIN, &[]).await;
    let by_key = with(
      request("PUT", "/api/v0/application", None),
      "x-api-key",
      "robot-key-0123456789",
    );
    assert_eq!(app.send_from(OUTSIDE, by_key).await.status, 403);
    assert!(!app.state.shutdown.is_cancelled());
    let answer = app.send(request("PUT", "/api/v0/application", None)).await;
    assert_eq!(answer.status, 204);
    assert!(app.state.restart_requested());
    assert!(app.state.shutdown.is_cancelled());
  }

  #[tokio::test]
  async fn a_shutdown_stops_the_daemon_for_good() {
    let app = app(ADMIN, &[]).await;
    let answer = app
      .send(request("DELETE", "/api/v0/application", None))
      .await;
    assert_eq!(answer.status, 204);
    assert!(app.state.shutdown.is_cancelled());
    assert!(!app.state.restart_requested());
  }

  #[tokio::test]
  async fn the_data_tab_lists_the_database_files_and_vacuums_one() {
    let app = app(ADMIN, &[]).await;
    let listed = app
      .send(request("GET", "/api/v0/application/data", None))
      .await;
    assert_eq!(listed.status, 200);
    let files = listed.json();
    let names: Vec<_> = files
      .as_array()
      .unwrap()
      .iter()
      .map(|f| f["name"].as_str().unwrap().to_string())
      .collect();
    for name in ["auth", "events", "transfers"] {
      assert!(names.contains(&name.to_string()), "{names:?}");
    }
    let auth = files
      .as_array()
      .unwrap()
      .iter()
      .find(|f| f["name"] == "auth")
      .unwrap();
    assert!(auth["sizeBytes"].as_u64().unwrap() > 0);
    assert!(auth["path"].as_str().unwrap().ends_with("auth.db"));
    let vacuum = |name: &str| {
      request(
        "POST",
        &format!("/api/v0/application/data/{name}/vacuum"),
        None,
      )
    };
    assert_eq!(app.send(vacuum("transfers")).await.status, 204);
    assert_eq!(app.send(vacuum("nothing")).await.status, 404);
    let by_key = with(vacuum("events"), "x-api-key", "robot-key-0123456789");
    assert_eq!(app.send_from(OUTSIDE, by_key).await.status, 204);
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
