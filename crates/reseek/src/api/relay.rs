//! The relay routes: the relay's state, a controller's agents, and the calls agents make to
//! their controller: sending their shares, the files peers download through the controller,
//! fetching finished downloads, and the agent socket.
//!
//! Each route answers only when the daemon started with relay mode on, and 403
//! `relay_disabled` otherwise. An agent's calls take an API key or token, never a session.

use api_types::auth::{Principal, Via};
use api_types::relay::{RelayAgentState, RelayFileUploadResult, RelayState};
use axum::Extension;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{DefaultBodyLimit, Multipart, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use base64::Engine;
use reseek_shares::traverses;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, Path};
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, Client, ReadWrite};
use crate::config::Options;
use crate::services::AgentCredential;

/// The relay routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(state))
    .routes(routes!(agents))
    .routes(routes!(kick))
    .routes(routes!(receive_shares))
    .routes(routes!(receive_file))
    .routes(routes!(serve_download))
    .routes(routes!(socket))
    // an agent's share database and its files are as large as they are; the share upload
    // caps its database itself
    .layer(DefaultBodyLimit::disable())
}

/// Refuses every relay route when the daemon did not start with relay mode on.
fn enabled(options: &Options) -> Result<(), ApiError> {
  if options.relay.enabled {
    return Ok(());
  }
  Err(ApiError::disabled(
    "relay_disabled",
    "Relay mode is disabled on this instance",
  ))
}

/// Refuses a principal that is not an API key or token, unless authentication is off.
fn stored_credential(principal: &Principal, options: &Options) -> Result<(), ApiError> {
  match principal.via {
    Via::ApiToken { .. } | Via::ConfigApiKey { .. } => Ok(()),
    Via::TrustedNetwork { .. } if options.web.authentication.disabled => Ok(()),
    _ => Err(ApiError::forbidden("An agent must call with an API key")),
  }
}

/// The agent a request from `client` names in `X-Relay-Agent` with its `X-Relay-Credential`; a
/// request missing either is refused with 401.
fn agent_credential(headers: &HeaderMap, client: &Client) -> Result<AgentCredential, ApiError> {
  let header = |name: &str| {
    headers
      .get(name)
      .and_then(|value| value.to_str().ok())
      .filter(|value| !value.is_empty())
      .map(str::to_string)
  };
  match (header("x-relay-agent"), header("x-relay-credential")) {
    (Some(agent), Some(credential)) => Ok(AgentCredential {
      agent,
      credential,
      address: client.ip,
    }),
    _ => Err(ApiError::unauthorized(
      "X-Relay-Agent and X-Relay-Credential are required",
    )),
  }
}

/// The relay's state: whether it is on, and the agents connected to this controller.
#[utoipa::path(
  get, path = "/relay", tag = "relay",
  responses(
    (status = 200, body = RelayState), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn state(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<RelayState>, ApiError> {
  enabled(&app.startup.values)?;
  Ok(Json(app.state.current().relay.clone()))
}

/// The agents logged in to this controller.
#[utoipa::path(
  get, path = "/relay/agents", tag = "relay",
  responses(
    (status = 200, body = Vec<RelayAgentState>), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn agents(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<RelayAgentState>>, ApiError> {
  enabled(&app.startup.values)?;
  app.services.relay.agents().await.map(Json)
}

/// Closes an agent's connection and unloads its shares; it connects again unless its
/// configuration was removed too.
#[utoipa::path(
  delete, path = "/relay/agents/{name}", tag = "relay",
  params(("name" = String, Path, description = "the agent's instance name")),
  responses(
    (status = 204, description = "disconnected"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn kick(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
  enabled(&app.startup.values)?;
  app.services.relay.kick(&name).await?;
  info!(
    by = admin.principal.subject,
    agent = name,
    "relay agent disconnected"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// The token of an agent's upload, refused with `message` when it is no UUID.
fn token(text: &str, message: &str) -> Result<Uuid, ApiError> {
  Uuid::parse_str(text).map_err(|_| ApiError::bad_request(message))
}

/// The form of an agent's upload; a body that is not `multipart/form-data` is refused with 415.
fn form(form: Result<Multipart, MultipartRejection>) -> Result<Multipart, ApiError> {
  form.map_err(|e| ApiError::unsupported_media(e.body_text()))
}

/// An agent sends its share list (`shares`, JSON) and share database (`database`) for the
/// upload the controller asked for with `token`.
#[utoipa::path(
  post, path = "/relay/controller/shares/{token}", tag = "relay",
  params(
    ("token" = Uuid, Path, description = "the token the controller asked with"),
    ("X-Relay-Agent" = String, Header, description = "the agent's instance name"),
    ("X-Relay-Credential" = String, Header, description = "the agent's credential for the token")
  ),
  request_body(content_type = "multipart/form-data", content = Vec<u8>),
  responses(
    (status = 200, description = "loaded"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 413, body = api_types::ErrorBody),
    (status = 415, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn receive_shares(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(text): Path<String>,
  Extension(client): Extension<Client>,
  headers: HeaderMap,
  upload: Result<Multipart, MultipartRejection>,
) -> Result<StatusCode, ApiError> {
  let options = &app.startup.values;
  enabled(options)?;
  stored_credential(&caller.principal, options)?;
  let token = token(&text, "Token is not a valid Guid")?;
  let upload = form(upload)?;
  let agent = agent_credential(&headers, &client)?;
  app
    .services
    .relay
    .receive_shares(token, agent, upload)
    .await?;
  Ok(StatusCode::OK)
}

/// An agent sends the file (`file`, named by its remote filename) a peer downloads through the
/// controller, for the upload asked for with `token`; answered once that upload has ended,
/// with how it ended.
#[utoipa::path(
  post, path = "/relay/controller/files/{token}", tag = "relay",
  params(
    ("token" = Uuid, Path, description = "the token the controller asked with"),
    ("X-Relay-Agent" = String, Header, description = "the agent's instance name"),
    ("X-Relay-Credential" = String, Header, description = "the agent's credential for the token")
  ),
  request_body(content_type = "multipart/form-data", content = Vec<u8>),
  responses(
    (status = 200, body = RelayFileUploadResult), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody), (status = 415, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn receive_file(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path(text): Path<String>,
  Extension(client): Extension<Client>,
  headers: HeaderMap,
  upload: Result<Multipart, MultipartRejection>,
) -> Result<Json<RelayFileUploadResult>, ApiError> {
  let options = &app.startup.values;
  enabled(options)?;
  stored_credential(&caller.principal, options)?;
  let token = token(&text, "Token is not a valid Guid")?;
  let upload = form(upload)?;
  let agent = agent_credential(&headers, &client)?;
  app
    .services
    .relay
    .receive_file(token, agent, upload)
    .await
    .map(Json)
}

/// An agent fetches a finished download, named in `X-Relay-Filename-Base64` by its path under
/// the downloads directory, with the token the controller announced it with.
#[utoipa::path(
  get, path = "/relay/controller/downloads/{token}", tag = "relay",
  params(
    ("token" = Uuid, Path, description = "the token the download was announced with"),
    ("X-Relay-Agent" = String, Header, description = "the agent's instance name"),
    ("X-Relay-Credential" = String, Header, description = "the agent's credential for the token"),
    ("X-Relay-Filename-Base64" = String, Header, description = "the file's relative path, base64")
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn serve_download(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Path(text): Path<String>,
  Extension(client): Extension<Client>,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let options = &app.startup.values;
  enabled(options)?;
  stored_credential(&caller.principal, options)?;
  let token = token(&text, "Token is not in a valid format")?;
  let agent = agent_credential(&headers, &client)?;
  let filename = download_filename(&headers)?;
  app
    .services
    .relay
    .serve_download(token, agent, filename)
    .await
}

/// The relative path `X-Relay-Filename-Base64` names, refused when it is missing, does not
/// decode or steps outside the downloads directory.
fn download_filename(headers: &HeaderMap) -> Result<String, ApiError> {
  let encoded = headers
    .get("x-relay-filename-base64")
    .and_then(|value| value.to_str().ok())
    .ok_or_else(|| ApiError::bad_request("X-Relay-Filename-Base64 is required"))?;
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(encoded)
    .map_err(|e| ApiError::invalid("X-Relay-Filename-Base64 is not valid base64", e.to_string()))?;
  let filename = String::from_utf8(bytes)
    .map_err(|_| ApiError::bad_request("X-Relay-Filename-Base64 is not valid UTF-8"))?;
  if traverses(&filename) {
    return Err(ApiError::bad_request(
      "The filename contains a traversal segment",
    ));
  }
  Ok(filename)
}

/// The socket an agent keeps open to its controller, opened with a WebSocket upgrade and the
/// `reseek-relay-v1` sub-protocol.
#[utoipa::path(
  get, path = "/relay/ws", tag = "relay",
  responses(
    (status = 101, description = "the socket is open"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 426, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn socket(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  request: Request,
) -> Result<Response, ApiError> {
  let options = &app.startup.values;
  enabled(options)?;
  stored_credential(&caller.principal, options)?;
  app.services.relay.socket(request).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_agent_names_its_download_in_standard_base64() {
    let with = |value: &'static str| {
      let mut headers = HeaderMap::new();
      headers.insert("x-relay-filename-base64", value.parse().unwrap());
      headers
    };
    assert_eq!(
      download_filename(&with("QXJ0aXN0L0FsYnVtLzAxLmZsYWM=")).unwrap(),
      "Artist/Album/01.flac"
    );
    assert_eq!(
      download_filename(&HeaderMap::new()).unwrap_err().status(),
      400
    );
    assert_eq!(
      download_filename(&with("not base64!"))
        .unwrap_err()
        .status(),
      400
    );
    assert_eq!(
      download_filename(&with("Li4vc2VjcmV0"))
        .unwrap_err()
        .status(),
      400,
      "../secret"
    );
  }
}
