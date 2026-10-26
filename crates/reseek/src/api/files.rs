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
use axum::Extension;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
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
    .routes(routes!(file_content))
    .routes(routes!(archive))
    .routes(routes!(download_content))
    .routes(routes!(link_token))
}

/// The root a path parameter names; any other name is no route.
pub(super) fn root(name: &str) -> Result<FileRoot, ApiError> {
  match name {
    "downloads" => Ok(FileRoot::Downloads),
    "incomplete" => Ok(FileRoot::Incomplete),
    _ => Err(ApiError::not_found(format!(
      "no directory root named {name}"
    ))),
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
  Err(ApiError::forbidden("serving files is turned off"))
}

/// A request for a file's bytes: by its principal, or else by its link token alone.
fn by_principal_or_link(
  authentication: &Authentication,
  query: &ContentQuery,
) -> Result<Option<Principal>, ApiError> {
  match (&authentication.0, &query.token) {
    (Some(principal), _) => Ok(Some(principal.clone())),
    (None, Some(_)) => Ok(None),
    (None, None) => Err(ApiError::unauthorized("authentication required")),
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
  let root = root(&name)?;
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
  let root = root(&name)?;
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
  let root = root(&name)?;
  let path = validate::files_path(&path, true)?;
  deleting(&app, &caller.principal, &path)?;
  if path.is_empty() {
    return Err(ApiError::forbidden(
      "a directory root itself is not deleted",
    ));
  }
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
  let root = root(&name)?;
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
  Err(ApiError::forbidden("remote file management is disabled").logged())
}

/// A file's bytes for the browser to show or save, a single range honoured. A link without a
/// session loads it with the token `t`.
#[utoipa::path(
  method(get, head), path = "/files/{root}/files/{path}/content", tag = "files",
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

/// The file a succeeded download was moved to, served as a file is.
#[utoipa::path(
  method(get, head), path = "/transfers/downloads/{username}/{id}/content", tag = "files",
  params(
    ("username" = String, Path, description = "the peer"),
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
  Path((username, id)): Path<(String, Uuid)>,
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
  let root = root(&name)?;
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
  if let FileTokenRequest::Path { path } = &request {
    let malformed = || ApiError::bad_request("path must be <root>/<base64 path>");
    let (name, encoded) = path.split_once('/').ok_or_else(malformed)?;
    root(name).map_err(|_| malformed())?;
    validate::files_path(encoded, false)?;
  }
  serving(&app)?;
  app
    .services
    .files
    .link_token(&caller.principal, request)
    .await
    .map(Json)
}

#[cfg(test)]
mod tests {
  use std::io::Read;
  use std::path::PathBuf;

  use base64::Engine;
  use base64::engine::general_purpose::URL_SAFE_NO_PAD;
  use serde_json::json;

  use crate::testing::{TestApp, app, request, with};

  /// Loopback is a trusted administrator; deleting is allowed.
  const CONFIG: &str = "remote_file_management = true\n[web.authentication]\n\
                        trusted_networks = [\"127.0.0.0/8\"]\n";

  fn b64(path: &str) -> String {
    URL_SAFE_NO_PAD.encode(path)
  }

  fn downloads(app: &TestApp) -> PathBuf {
    app.state.paths.downloads.clone()
  }

  async fn with_files(config: &str) -> TestApp {
    let app = app(config, &[]).await;
    let root = downloads(&app);
    std::fs::create_dir_all(root.join("Album/Scans")).unwrap();
    std::fs::write(root.join("Album/01.flac"), b"0123456789").unwrap();
    std::fs::write(root.join("Album/Scans/cover.jpg"), b"jpg").unwrap();
    std::fs::write(root.join("page.html"), b"<script></script>").unwrap();
    app
  }

  #[tokio::test]
  async fn a_file_is_served_whole_in_a_range_or_not_at_all_when_unchanged() {
    let app = with_files(CONFIG).await;
    let uri = format!(
      "/api/v0/files/downloads/files/{}/content",
      b64("Album/01.flac")
    );
    let whole = app.send(request("GET", &uri, None)).await;
    assert_eq!(whole.status, 200);
    assert_eq!(&whole.body[..], b"0123456789");
    let headers = &whole.headers;
    assert_eq!(headers["content-type"], "audio/flac");
    assert_eq!(headers["accept-ranges"], "bytes");
    assert_eq!(headers["content-length"], "10");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["cache-control"], "private, no-cache");
    assert_eq!(
      headers["content-disposition"],
      "inline; filename*=UTF-8''01.flac"
    );
    let etag = headers["etag"].to_str().unwrap().to_string();

    let ranged = app
      .send(with(request("GET", &uri, None), "range", "bytes=2-4"))
      .await;
    assert_eq!(ranged.status, 206);
    assert_eq!(&ranged.body[..], b"234");
    assert_eq!(ranged.headers["content-range"], "bytes 2-4/10");
    assert_eq!(ranged.headers["content-length"], "3");
    let tail = app
      .send(with(request("GET", &uri, None), "range", "bytes=-3"))
      .await;
    assert_eq!(&tail.body[..], b"789");
    for range in ["bytes=10-", "bytes=0-1,4-5"] {
      let refused = app
        .send(with(request("GET", &uri, None), "range", range))
        .await;
      assert_eq!(refused.status, 416, "{range}");
      assert_eq!(refused.headers["content-range"], "bytes */10");
    }
    let unchanged = app
      .send(with(request("GET", &uri, None), "if-none-match", &etag))
      .await;
    assert_eq!(unchanged.status, 304);
    assert!(unchanged.body.is_empty());
    let head = app.send(request("HEAD", &uri, None)).await;
    assert_eq!(head.status, 200);
    assert_eq!(head.headers["content-length"], "10");
    assert!(head.body.is_empty());

    let page = format!("/api/v0/files/downloads/files/{}/content", b64("page.html"));
    let saved = app.send(request("GET", &page, None)).await;
    assert!(
      saved.headers["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment;"),
      "HTML is never shown inline"
    );
    let missing = format!("/api/v0/files/downloads/files/{}/content", b64("nothing"));
    assert_eq!(app.send(request("GET", &missing, None)).await.status, 404);
  }

  #[tokio::test]
  async fn an_incomplete_file_is_served_only_once_no_download_writes_it() {
    let app = with_files(CONFIG).await;
    let file = app.state.paths.incomplete.join("partial.flac");
    std::fs::write(&file, b"part").unwrap();
    let uri = format!(
      "/api/v0/files/incomplete/files/{}/content",
      b64("partial.flac")
    );
    let delete = format!("/api/v0/files/incomplete/files/{}", b64("partial.flac"));
    let guard = app.state.files.writing(&file);
    assert_eq!(app.send(request("GET", &uri, None)).await.status, 409);
    assert_eq!(app.send(request("DELETE", &delete, None)).await.status, 409);
    drop(guard);
    assert_eq!(app.send(request("GET", &uri, None)).await.status, 200);
    assert_eq!(app.send(request("DELETE", &delete, None)).await.status, 204);
    assert!(!file.exists());
  }

  #[tokio::test]
  async fn a_link_token_opens_its_one_file_without_a_session() {
    let app = with_files(CONFIG).await;
    let path = format!("downloads/{}", b64("Album/01.flac"));
    let made = app
      .send(request(
        "POST",
        "/api/v0/session/file-token",
        Some(json!({ "path": path })),
      ))
      .await;
    assert_eq!(made.status, 200, "{:?}", made.body);
    let token = made.json()["token"].as_str().unwrap().to_string();
    let content = |file: &str, token: Option<&str>| {
      let mut uri = format!("/api/v0/files/downloads/files/{}/content", b64(file));
      if let Some(token) = token {
        uri.push_str(&format!("?t={token}"));
      }
      request("GET", &uri, None)
    };
    let stranger = "192.0.2.1:1";
    let opened = app
      .send_from(stranger, content("Album/01.flac", Some(&token)))
      .await;
    assert_eq!(opened.status, 200);
    assert_eq!(&opened.body[..], b"0123456789");
    let other = app
      .send_from(stranger, content("page.html", Some(&token)))
      .await;
    assert_eq!(other.status, 401, "made for another file");
    let none = app
      .send_from(stranger, content("Album/01.flac", None))
      .await;
    assert_eq!(none.status, 401);
    let forged = app
      .send_from(stranger, content("Album/01.flac", Some("a.b.c")))
      .await;
    assert_eq!(forged.status, 401);
    let missing = app
      .send(request(
        "POST",
        "/api/v0/session/file-token",
        Some(json!({ "path": format!("downloads/{}", b64("nothing")) })),
      ))
      .await;
    assert_eq!(missing.status, 404);
  }

  #[tokio::test]
  async fn listings_and_deletions_stay_inside_their_roots() {
    let app = with_files(CONFIG).await;
    let root = downloads(&app);
    let listed = app
      .send(request("GET", "/api/v0/files/downloads/directories", None))
      .await;
    assert_eq!(listed.status, 200);
    let body = listed.json();
    assert_eq!(body["fullName"], "");
    assert_eq!(body["directories"][0]["name"], "Album");
    assert_eq!(body["files"][0]["name"], "page.html");
    let delete = |path: &str| {
      request(
        "DELETE",
        &format!("/api/v0/files/downloads/files/{}", b64(path)),
        None,
      )
    };
    #[cfg(unix)]
    {
      let outside = app.dir.path().join("outside");
      std::fs::create_dir_all(&outside).unwrap();
      std::fs::write(outside.join("passwd"), b"x").unwrap();
      std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
      assert_eq!(app.send(delete("link/passwd")).await.status, 403);
      assert!(outside.join("passwd").exists());
      let through = format!("/api/v0/files/downloads/directories/{}", b64("link"));
      assert_eq!(app.send(request("GET", &through, None)).await.status, 403);
    }
    assert_eq!(app.send(delete("Album/01.flac")).await.status, 204);
    assert_eq!(app.send(delete("Album/01.flac")).await.status, 404);
    let directory = format!("/api/v0/files/downloads/directories/{}", b64("Album"));
    assert_eq!(
      app.send(request("DELETE", &directory, None)).await.status,
      204
    );
    assert!(!root.join("Album").exists());

    let off = with_files("[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n").await;
    assert_eq!(
      off.send(delete("page.html")).await.status,
      403,
      "remote file management is off"
    );
  }

  #[tokio::test]
  async fn a_directory_is_sent_as_a_zip_one_at_a_time_and_within_the_limit() {
    let app = with_files(CONFIG).await;
    let uri = format!(
      "/api/v0/files/downloads/directories/{}/archive",
      b64("Album")
    );
    let answer = app.send(request("GET", &uri, None)).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.headers["content-type"], "application/zip");
    assert_eq!(
      answer.headers["content-disposition"],
      "attachment; filename=\"Album.zip\""
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(answer.body.to_vec())).unwrap();
    let names: Vec<_> = zip.file_names().map(str::to_string).collect();
    assert_eq!(names, ["01.flac", "Scans/cover.jpg"]);
    let mut data = Vec::new();
    zip
      .by_name("01.flac")
      .unwrap()
      .read_to_end(&mut data)
      .unwrap();
    assert_eq!(data, b"0123456789");

    // a file too large for the writer's backlog keeps the first archive open
    std::fs::write(downloads(&app).join("Album/big.wav"), vec![0u8; 4 << 20]).unwrap();
    let first = app
      .stream_from("127.0.0.1:1", request("GET", &uri, None))
      .await;
    assert_eq!(first.status, 200);
    let second = app.send(request("GET", &uri, None)).await;
    assert_eq!(second.status, 429);
    drop(first);

    let small = with_files(&format!(
      "{CONFIG}[web.files]\narchive_max_bytes = \"5B\"\n"
    ))
    .await;
    assert_eq!(small.send(request("GET", &uri, None)).await.status, 413);
  }

  #[tokio::test]
  async fn log_files_are_listed_and_read_in_ranges() {
    let app = with_files(CONFIG).await;
    let logs = app.state.paths.logs.clone();
    std::fs::write(
      logs.join("reseek-20261025.log"),
      b"first line\nsecond line\n",
    )
    .unwrap();
    std::fs::write(logs.join("notes.txt"), b"").unwrap();
    let listed = app.send(request("GET", "/api/v0/logs/files", None)).await;
    assert_eq!(listed.status, 200);
    let names: Vec<_> = listed
      .json()
      .as_array()
      .unwrap()
      .iter()
      .map(|f| f["name"].clone())
      .collect();
    assert_eq!(names, [json!("reseek-20261025.log")]);
    let uri = "/api/v0/logs/files/reseek-20261025.log";
    let read = app
      .send(with(request("GET", uri, None), "range", "bytes=11-"))
      .await;
    assert_eq!(read.status, 206);
    assert_eq!(&read.body[..], b"second line\n");
    assert_eq!(read.headers["content-type"], "text/plain; charset=utf-8");
    let missing = app
      .send(request("GET", "/api/v0/logs/files/gone.log", None))
      .await;
    assert_eq!(missing.status, 404);
  }
}
