//! The shares routes: every host's shares, what one share holds, the folders a new share can be
//! picked from, and scanning this host's shares. Reading takes the ReadOnly role, scanning
//! ReadWrite, and the folders an administrator with `remote_configuration` on.

use api_types::shares::{Share, ShareFolders, ShareFoldersQuery, ShareScanQuery, SharesByHost};
use api_types::users::Directory;
use axum::extract::State;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use super::extract::{Json, Path, Query};
use super::options::remote;
use crate::app::AppState;
use crate::auth::{Administrator, AnyRole, Authorized, ReadWrite};

/// The shares routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list, scan, cancel_scan))
    .routes(routes!(contents))
    .routes(routes!(folders))
    .routes(routes!(share))
    .routes(routes!(share_contents))
}

/// The shares of every host by host name, this one as `local`.
#[utoipa::path(
  operation_id = "list_shares",
  get, path = "/shares", tag = "shares",
  responses(
    (status = 200, body = SharesByHost), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn list(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<SharesByHost>, ApiError> {
  app.services.shares.shares().await.map(Json)
}

/// One share, on any host.
#[utoipa::path(
  get, path = "/shares/{id}", tag = "shares",
  params(("id" = String, Path, description = "the share's id")),
  responses(
    (status = 200, body = Share), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn share(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<String>,
) -> Result<Json<Share>, ApiError> {
  app.services.shares.share(&id).await.map(Json)
}

/// Everything every host shares, as other users browsing us see it.
#[utoipa::path(
  get, path = "/shares/contents", tag = "shares",
  responses(
    (status = 200, body = Vec<Directory>), (status = 401, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn contents(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
) -> Result<Json<Vec<Directory>>, ApiError> {
  app.services.shares.contents().await.map(Json)
}

/// The folders of this machine a new share can be picked from, directly in a folder or at the top
/// of the machine. Files never appear, nor anything a scan would leave out; that and a path that
/// does not exist answer alike.
#[utoipa::path(
  get, path = "/shares/folders", tag = "shares", params(ShareFoldersQuery),
  responses(
    (status = 200, body = ShareFolders), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn folders(
  State(app): State<AppState>,
  _caller: Authorized<Administrator>,
  Query(query): Query<ShareFoldersQuery>,
) -> Result<Json<ShareFolders>, ApiError> {
  remote(
    &app.config.current(),
    "Blocked folder listing; remote configuration is disabled",
  )?;
  app.services.shares.folders(query.path).await.map(Json)
}

/// What one share holds, its own directory included.
#[utoipa::path(
  get, path = "/shares/{id}/contents", tag = "shares",
  params(("id" = String, Path, description = "the share's id")),
  responses(
    (status = 200, body = Vec<Directory>), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody), (status = 503, body = api_types::ErrorBody)
  )
)]
async fn share_contents(
  State(app): State<AppState>,
  _caller: Authorized<AnyRole>,
  Path(id): Path<String>,
) -> Result<Json<Vec<Directory>>, ApiError> {
  app.services.shares.share_contents(&id).await.map(Json)
}

/// Starts a scan of this host's shares; `force` reads every file again. The scan's progress is
/// in the application state.
#[utoipa::path(
  put, path = "/shares", tag = "shares", params(ShareScanQuery),
  responses(
    (status = 204, description = "the scan started"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody, description = "a scan is running"),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn scan(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
  Query(query): Query<ShareScanQuery>,
) -> Result<StatusCode, ApiError> {
  app.services.shares.scan(query.force).await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Stops the running scan.
#[utoipa::path(
  delete, path = "/shares", tag = "shares",
  responses(
    (status = 204, description = "the scan is stopping"),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 409, body = api_types::ErrorBody, description = "no scan is running"),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn cancel_scan(
  State(app): State<AppState>,
  _caller: Authorized<ReadWrite>,
) -> Result<StatusCode, ApiError> {
  app.services.shares.cancel_scan().await?;
  Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
  use tempfile::TempDir;

  use crate::testing::{TestApp, app, request};

  /// A folder holding one of everything the route tells apart. Its name is not hidden, as the
  /// default of a temporary directory is.
  fn tree() -> TempDir {
    let dir = tempfile::Builder::new()
      .prefix("folders")
      .tempdir()
      .unwrap();
    for folder in [
      "Music/Albums",
      "photos",
      ".hidden/in",
      "Skipped/in",
      "Gone/in",
    ] {
      std::fs::create_dir_all(dir.path().join(folder)).unwrap();
    }
    std::fs::write(dir.path().join("secret.txt"), b"a file").unwrap();
    std::fs::write(dir.path().join("Music/track.flac"), b"a file").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("photos"), dir.path().join("link")).unwrap();
    dir
  }

  /// Loopback is a trusted administrator, `Gone` is excluded and `Skipped` filtered; `follow` is
  /// whether links are followed.
  fn config(dir: &Path, remote: bool, follow: bool) -> String {
    format!(
      "remote_configuration = {remote}\n[shares]\nfollow_symlinks = {follow}\n\
       directories = [\"!{}\"]\nfilters = [\"Skipped$\"]\n[web.authentication]\n\
       trusted_networks = [\"127.0.0.0/8\"]\n",
      dir.join("Gone").display()
    )
  }

  fn uri(path: &Path) -> String {
    let path = path.to_str().unwrap();
    format!(
      "/api/v0/shares/folders?path={}",
      utf8_percent_encode(path, NON_ALPHANUMERIC)
    )
  }

  async fn names(app: &TestApp, path: &Path) -> Vec<String> {
    let listed = app.send(request("GET", &uri(path), None)).await;
    assert_eq!(listed.status, 200, "{path:?}: {:?}", listed.body);
    let listed = listed.json();
    let names = listed["folders"].as_array().unwrap().iter();
    names
      .map(|folder| folder["name"].as_str().unwrap().to_string())
      .collect()
  }

  #[tokio::test]
  async fn only_folders_a_scan_would_walk_into_are_listed() {
    let dir = tree();
    let app = app(&config(dir.path(), true, false), &[]).await;
    assert_eq!(names(&app, dir.path()).await, ["Music", "photos"]);
    assert_eq!(names(&app, &dir.path().join("Music")).await, ["Albums"]);
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn links_are_listed_only_when_a_scan_follows_them() {
    let dir = tree();
    let app = app(&config(dir.path(), true, true), &[]).await;
    assert_eq!(names(&app, dir.path()).await, ["link", "Music", "photos"]);
    assert!(names(&app, &dir.path().join("link")).await.is_empty());
  }

  #[tokio::test]
  async fn what_a_scan_refuses_answers_as_what_does_not_exist() {
    let dir = tree();
    let app = app(&config(dir.path(), true, false), &[]).await;
    let missing = app
      .send(request("GET", &uri(&dir.path().join("nothing")), None))
      .await;
    assert_eq!(missing.status, 404);
    for refused in [
      "secret.txt",
      "Music/track.flac",
      ".hidden",
      ".hidden/in",
      "Skipped",
      "Skipped/in",
      "Gone",
      "Gone/in",
      #[cfg(unix)]
      "link",
    ] {
      let answer = app
        .send(request("GET", &uri(&dir.path().join(refused)), None))
        .await;
      assert_eq!(answer.status, 404, "{refused}");
      assert_eq!(answer.body, missing.body, "{refused}");
    }
    for bad in ["Music", "../etc"] {
      let answer = app.send(request("GET", &uri(Path::new(bad)), None)).await;
      assert_eq!(answer.status, 400, "{bad}");
    }
    let out = dir.path().join("Music/../.hidden");
    assert_eq!(app.send(request("GET", &uri(&out), None)).await.status, 400);
  }

  #[tokio::test]
  async fn listing_folders_needs_remote_configuration() {
    let dir = tree();
    let off = app(&config(dir.path(), false, false), &[]).await;
    let listed = off.send(request("GET", &uri(dir.path()), None)).await;
    assert_eq!(listed.status, 403);
  }
}
