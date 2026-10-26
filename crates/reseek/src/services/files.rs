//! The local files under the downloads and incomplete directories, and the log files: listing,
//! deleting, serving them to the browser, and the links that load them.

use api_types::auth::Principal;
use api_types::files::{
  ContentDisposition, FileRoot, FileToken, FilesystemDirectory, FilesystemFile,
};
use async_trait::async_trait;
use axum::http::HeaderMap;
use axum::response::Response;
use uuid::Uuid;

use super::not_yet;
use crate::api::error::ApiError;

const DOMAIN: &str = "files";

/// The local file a call is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileTarget {
  /// a file under a root, by its path relative to the root
  File {
    /// the directory the path is in
    root: FileRoot,
    /// `/`-separated, relative, with no `.` or `..` segment
    path: String,
  },
  /// the file a succeeded download was moved to
  Download {
    /// the peer
    username: String,
    /// the download's id
    id: Uuid,
  },
}

/// How a file's bytes are asked for.
#[derive(Clone, Debug)]
pub struct ContentRequest {
  /// the request's headers: `Range`, `If-None-Match` and the like
  pub headers: HeaderMap,
  /// only the headers are wanted
  pub head: bool,
  /// whether the browser shows the file or saves it
  pub disposition: ContentDisposition,
  /// the name to save the file under in place of its own
  pub name: Option<String>,
  /// the request's principal; `None` when only `link` lets it in
  pub principal: Option<Principal>,
  /// the short-lived link token the request came with
  pub link: Option<String>,
}

/// The files under the download directories and the log directory. Paths are relative to their
/// root, `/`-separated, with no `.` or `..` segment; one that leads out of its root through a
/// link is refused with 403.
#[async_trait]
// the default bodies answer 503 without looking at their arguments
#[allow(unused_variables)]
pub trait FileService: Send + Sync {
  /// A directory under a root, the root itself for an empty path, with what is in it or, with
  /// `recursive`, everything under it.
  async fn list(
    &self,
    root: FileRoot,
    path: &str,
    recursive: bool,
  ) -> Result<FilesystemDirectory, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Deletes a directory under a root and everything in it, never the root itself.
  async fn delete_directory(&self, root: FileRoot, path: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// Deletes a file under a root.
  async fn delete_file(&self, root: FileRoot, path: &str) -> Result<(), ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A file's bytes as the whole response, ranges and caching headers honoured. A request
  /// without a principal is let in by its link token alone, or refused with 401.
  async fn content(
    &self,
    target: FileTarget,
    request: ContentRequest,
  ) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A directory under a root as a zip archive streamed as the whole response, named `name`
  /// or after the directory. One archive per principal at a time.
  async fn archive(
    &self,
    principal: &Principal,
    root: FileRoot,
    path: &str,
    name: Option<String>,
  ) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A short-lived token that lets a link load one file under a root for `principal`.
  async fn link_token(
    &self,
    principal: &Principal,
    root: FileRoot,
    path: &str,
  ) -> Result<FileToken, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// A short-lived token that lets a link load the file of the download `id` for `principal`.
  async fn download_link_token(
    &self,
    principal: &Principal,
    id: Uuid,
  ) -> Result<FileToken, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// The log files; 404 when the log directory does not exist.
  async fn log_files(&self) -> Result<Vec<FilesystemFile>, ApiError> {
    Err(not_yet(DOMAIN))
  }

  /// One log file's text as the whole response, ranges honoured. `name` is a bare `.log`
  /// file name.
  async fn log_file(&self, name: &str, request: ContentRequest) -> Result<Response, ApiError> {
    Err(not_yet(DOMAIN))
  }
}
