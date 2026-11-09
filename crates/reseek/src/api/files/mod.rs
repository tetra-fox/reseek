//! The files routes: listing and deleting under the downloads and incomplete directories,
//! serving a file or a zipped directory to the browser, and the short-lived token that lets a
//! link load a file.
//!
//! A path in these routes is the base64 of its UTF-8 text relative to its root, in the standard
//! or the URL-safe alphabet, padded or not. Listing and serving take the ReadOnly role;
//! deleting takes ReadWrite and `remote_file_management`, and serving `web.files.serve`.

use api_types::auth::Principal;
use api_types::files::{
  ArchiveQuery, ContentQuery, DirectoryListingQuery, FileRoot, FileToken, FileTokenRequest,
  FilesystemDirectory,
};
use api_types::users::UsernameQuery;
use axum::Extension;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Id, Json, NamedUser, Path, Query};
use super::validate;
use crate::app::AppState;
use crate::auth::{AnyRole, Authentication, Authorized, ReadWrite};
use crate::services::{ContentRequest, FileTarget};

/// The files routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list_root))
    .routes(routes!(list_directory, delete_directory))
    .routes(routes!(delete_file))
    .routes(routes!(file_content, file_content_head))
    .routes(routes!(archive))
    .routes(routes!(download_content, download_content_head))
    .routes(routes!(file_wav, file_wav_head))
    .routes(routes!(download_wav, download_wav_head))
    .routes(routes!(link_token))
}

/// The root a path parameter names; any other name is no route.
pub(super) fn root(name: &str) -> Result<FileRoot, ApiError> {
  name.parse().map_err(ApiError::not_found)
}

/// The root a route that lists or removes files names. Any principal reaches a temporary file by
/// its path, under a random folder name only its uploader is told, so that root is no route here.
fn listed(name: &str) -> Result<FileRoot, ApiError> {
  match root(name)? {
    FileRoot::Temporary => Err(ApiError::not_found(format!(
      "No directory root named {name}"
    ))),
    root => Ok(root),
  }
}

/// How a request asks for a file's bytes, from its method, headers, principal and query.
pub(super) fn content_request(
  method: &Method,
  headers: HeaderMap,
  principal: Option<Principal>,
  query: ContentQuery,
) -> ContentRequest {
  ContentRequest {
    headers,
    head: method == Method::HEAD,
    disposition: query.disposition,
    name: query.name,
    principal,
    link: query.token,
  }
}

/// Refuses to serve files when `web.files.serve` is off.
fn serving(app: &AppState) -> Result<(), ApiError> {
  if app.config.current().values.web.files.serve {
    return Ok(());
  }
  Err(ApiError::forbidden("File serving is disabled"))
}

/// A request for a file's bytes: by its principal, or else by its link token alone.
fn by_principal_or_link(
  authentication: &Authentication,
  query: &ContentQuery,
) -> Result<Option<Principal>, ApiError> {
  match (&authentication.0, &query.token) {
    (Some(principal), _) => Ok(Some(principal.clone())),
    (None, Some(_)) => Ok(None),
    (None, None) => Err(ApiError::unauthorized("Authentication required")),
  }
}

/// A root directory with what is in it, or everything under it with `recursive`.
#[utoipa::path(
  get, path = "/files/{root}/directories", tag = "files",
  params(("root" = FileRoot, Path, description = "the directory root"), DirectoryListingQuery),
  responses(
    (status = 200, body = FilesystemDirectory), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list_root(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(name): Path<String>,
  Query(query): Query<DirectoryListingQuery>,
) -> Result<Json<FilesystemDirectory>, ApiError> {
  let root = listed(&name)?;
  app
    .services
    .files
    .list(root, "", query.recursive)
    .await
    .map(Json)
}

/// A directory under a root with what is in it, or everything under it with `recursive`.
#[utoipa::path(
  get, path = "/files/{root}/directories/{path}", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the directory's path under the root, base64"),
    DirectoryListingQuery
  ),
  responses(
    (status = 200, body = FilesystemDirectory), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list_directory(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<DirectoryListingQuery>,
) -> Result<Json<FilesystemDirectory>, ApiError> {
  let root = listed(&name)?;
  let path = validate::files_path(&path, false)?;
  app
    .services
    .files
    .list(root, &path, query.recursive)
    .await
    .map(Json)
}

/// Deletes a directory under a root and everything in it.
#[utoipa::path(
  delete, path = "/files/{root}/directories/{path}", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the directory's path under the root, base64")
  ),
  responses(
    (status = 204, description = "deleted"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn delete_directory(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((name, path)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
  let root = listed(&name)?;
  let path = validate::files_path(&path, true)?;
  deleting(&app, &caller.principal, &path)?;
  app.services.files.delete_directory(root, &path).await?;
  info!(
    by = caller.principal.subject,
    ?root,
    path,
    "directory deleted"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Deletes a file under a root.
#[utoipa::path(
  delete, path = "/files/{root}/files/{path}", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64")
  ),
  responses(
    (status = 204, description = "deleted"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn delete_file(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Path((name, path)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
  let root = listed(&name)?;
  let path = validate::files_path(&path, true)?;
  deleting(&app, &caller.principal, &path)?;
  app.services.files.delete_file(root, &path).await?;
  info!(by = caller.principal.subject, ?root, path, "file deleted");
  Ok(StatusCode::NO_CONTENT)
}

/// Refuses a deletion while `remote_file_management` is off.
fn deleting(app: &AppState, by: &Principal, path: &str) -> Result<(), ApiError> {
  if app.config.current().values.remote_file_management {
    return Ok(());
  }
  warn!(
    by = by.subject,
    path, "deletion refused; remote file management is disabled"
  );
  Err(ApiError::forbidden("Remote file management is disabled").logged())
}

/// A file's bytes for the browser to show or save, a single range honored. A link without a
/// session loads it with the token `t`.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/content", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    ContentQuery
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent),
    (status = 206, content_type = "application/octet-stream", body = api_types::files::FileContent),
    (status = 304, description = "unchanged"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 416, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn file_content(
  State(app): State<AppState>,
  Extension(authentication): Extension<Authentication>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let principal = by_principal_or_link(&authentication, &query)?;
  let root = root(&name)?;
  let path = validate::files_path(&path, false)?;
  serving(&app)?;
  let request = content_request(&method, headers, principal, query);
  app
    .services
    .files
    .content(FileTarget::File { root, path }, request)
    .await
}

/// The headers a file's bytes would be served with, without the bytes.
#[utoipa::path(
  head, path = "/files/{root}/files/{path}/content", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    ContentQuery
  ),
  responses(
    (status = 200, description = "the headers of the whole file"),
    (status = 206, description = "the headers of the range asked for"),
    (status = 304, description = "unchanged"), (status = 400), (status = 401), (status = 403),
    (status = 404), (status = 409), (status = 416), (status = 503)
  )
)]
async fn file_content_head(
  state: State<AppState>,
  authentication: Extension<Authentication>,
  path: Path<(String, String)>,
  query: Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  file_content(state, authentication, path, query, method, headers).await
}

/// A file as a WAV made as it is read, for AIFF and ALAC, which browsers cannot play: never
/// stored, a single range honored. Anything else, and AIFF that is compressed, is refused.
#[utoipa::path(
  get, path = "/files/{root}/files/{path}/wav", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    ContentQuery
  ),
  responses(
    (status = 200, content_type = "audio/wav", body = api_types::files::FileContent),
    (status = 206, content_type = "audio/wav", body = api_types::files::FileContent),
    (status = 304, description = "unchanged"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 409, body = api_types::ErrorBody),
    (status = 415, body = api_types::ErrorBody), (status = 416, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn file_wav(
  State(app): State<AppState>,
  Extension(authentication): Extension<Authentication>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let principal = by_principal_or_link(&authentication, &query)?;
  let root = root(&name)?;
  let path = validate::files_path(&path, false)?;
  serving(&app)?;
  let request = content_request(&method, headers, principal, query);
  app
    .services
    .files
    .wav(FileTarget::File { root, path }, request)
    .await
}

/// The headers a file's WAV would be served with, without the bytes.
#[utoipa::path(
  head, path = "/files/{root}/files/{path}/wav", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the file's path under the root, base64"),
    ContentQuery
  ),
  responses(
    (status = 200, description = "the headers of the whole WAV"),
    (status = 206, description = "the headers of the range asked for"),
    (status = 304, description = "unchanged"), (status = 400), (status = 401), (status = 403),
    (status = 404), (status = 409), (status = 415), (status = 416), (status = 503)
  )
)]
async fn file_wav_head(
  state: State<AppState>,
  authentication: Extension<Authentication>,
  path: Path<(String, String)>,
  query: Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  file_wav(state, authentication, path, query, method, headers).await
}

/// The file a succeeded download was moved to as a WAV, served as a file's is.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/wav", tag = "files",
  params(
    ("username" = String, Path, description = "the peer"), UsernameQuery,
    ("id" = Uuid, Path, description = "the download's id"),
    ContentQuery
  ),
  responses(
    (status = 200, content_type = "audio/wav", body = api_types::files::FileContent),
    (status = 206, content_type = "audio/wav", body = api_types::files::FileContent),
    (status = 304, description = "unchanged"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 415, body = api_types::ErrorBody),
    (status = 416, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn download_wav(
  State(app): State<AppState>,
  Extension(authentication): Extension<Authentication>,
  NamedUser(username): NamedUser,
  Id(id): Id<Uuid>,
  Query(query): Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let principal = by_principal_or_link(&authentication, &query)?;
  serving(&app)?;
  let request = content_request(&method, headers, principal, query);
  app
    .services
    .files
    .wav(FileTarget::Download { username, id }, request)
    .await
}

/// The headers a succeeded download's WAV would be served with, without the bytes.
#[utoipa::path(
  head, path = "/transfers/downloads/{username}/{id}/wav", tag = "files",
  params(
    ("username" = String, Path, description = "the peer"), UsernameQuery,
    ("id" = Uuid, Path, description = "the download's id"),
    ContentQuery
  ),
  responses(
    (status = 200, description = "the headers of the whole WAV"),
    (status = 206, description = "the headers of the range asked for"),
    (status = 304, description = "unchanged"), (status = 400), (status = 401), (status = 403),
    (status = 404), (status = 415), (status = 416), (status = 503)
  )
)]
async fn download_wav_head(
  state: State<AppState>,
  authentication: Extension<Authentication>,
  user: NamedUser,
  id: Id<Uuid>,
  query: Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  download_wav(state, authentication, user, id, query, method, headers).await
}

/// The file a succeeded download was moved to, served as a file is.
#[utoipa::path(
  get, path = "/transfers/downloads/{username}/{id}/content", tag = "files",
  params(
    ("username" = String, Path, description = "the peer"), UsernameQuery,
    ("id" = Uuid, Path, description = "the download's id"),
    ContentQuery
  ),
  responses(
    (status = 200, content_type = "application/octet-stream", body = api_types::files::FileContent),
    (status = 206, content_type = "application/octet-stream", body = api_types::files::FileContent),
    (status = 304, description = "unchanged"), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 416, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn download_content(
  State(app): State<AppState>,
  Extension(authentication): Extension<Authentication>,
  NamedUser(username): NamedUser,
  Id(id): Id<Uuid>,
  Query(query): Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  let principal = by_principal_or_link(&authentication, &query)?;
  serving(&app)?;
  let request = content_request(&method, headers, principal, query);
  app
    .services
    .files
    .content(FileTarget::Download { username, id }, request)
    .await
}

/// The headers a succeeded download's file would be served with, without the bytes.
#[utoipa::path(
  head, path = "/transfers/downloads/{username}/{id}/content", tag = "files",
  params(
    ("username" = String, Path, description = "the peer"), UsernameQuery,
    ("id" = Uuid, Path, description = "the download's id"),
    ContentQuery
  ),
  responses(
    (status = 200, description = "the headers of the whole file"),
    (status = 206, description = "the headers of the range asked for"),
    (status = 304, description = "unchanged"), (status = 400), (status = 401), (status = 403),
    (status = 404), (status = 416), (status = 503)
  )
)]
async fn download_content_head(
  state: State<AppState>,
  authentication: Extension<Authentication>,
  user: NamedUser,
  id: Id<Uuid>,
  query: Query<ContentQuery>,
  method: Method,
  headers: HeaderMap,
) -> Result<Response, ApiError> {
  download_content(state, authentication, user, id, query, method, headers).await
}

/// A directory under a root as a zip archive, streamed. One archive per principal at a time.
#[utoipa::path(
  get, path = "/files/{root}/directories/{path}/archive", tag = "files",
  params(
    ("root" = FileRoot, Path, description = "the directory root"),
    ("path" = String, Path, description = "the directory's path under the root, base64"),
    ArchiveQuery
  ),
  responses(
    (status = 200, content_type = "application/zip", body = api_types::files::FileContent),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 413, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn archive(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Path((name, path)): Path<(String, String)>,
  Query(query): Query<ArchiveQuery>,
) -> Result<Response, ApiError> {
  let root = listed(&name)?;
  let path = validate::files_path(&path, false)?;
  serving(&app)?;
  app
    .services
    .files
    .archive(&caller.principal, root, &path, query.name)
    .await
}

/// A token that lets a link load one file for ten minutes, for a browser that cannot send the
/// session with the link.
#[utoipa::path(
  post, path = "/session/file-token", tag = "files", request_body = FileTokenRequest,
  responses(
    (status = 200, body = FileToken), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn link_token(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Json(request): Json<FileTokenRequest>,
) -> Result<Json<FileToken>, ApiError> {
  let files = &app.services.files;
  let token = match request {
    FileTokenRequest::Path { path } => {
      let malformed = || ApiError::bad_request("The path must be <root>/<base64 path>");
      let (name, encoded) = path.split_once('/').ok_or_else(malformed)?;
      let root = root(name).map_err(|_| malformed())?;
      let path = validate::files_path(encoded, false)?;
      serving(&app)?;
      files.link_token(&caller.principal, root, &path).await
    }
    FileTokenRequest::Transfer { transfer_id } => {
      serving(&app)?;
      files
        .download_link_token(&caller.principal, transfer_id)
        .await
    }
  };
  token.map(Json)
}

#[cfg(test)]
mod tests;
