//! The web UI: the bundle built into the program or a directory given by `web.content_path`,
//! with `index.html` answering every path of the single-page app.
//!
//! The UI is built with relative asset paths and a hash router, so one build works under any
//! URL base: assets are served as they are, `index.html` is never rewritten.

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tracing::{info, warn};

use crate::config::Options;

/// What the program carries of the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bundle {
  /// the UI built into the program, or a page saying it was not built
  #[cfg(feature = "webui")]
  Embedded,
  /// no UI
  None,
}

/// The page served when the program was built without a UI build.
const NOT_BUILT: &str = "<!doctype html><title>reseek</title><p>UI not built, run `just \
                         web-build`.</p>\n";

/// Hashed assets never change under their name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Where the UI is served from.
#[derive(Clone, Debug)]
pub enum Ui {
  /// not served: headless, turned off, or a build without one
  Off,
  /// the bundle built into the program
  #[cfg(feature = "webui")]
  Embedded,
  /// a directory of the user's
  Directory(PathBuf),
}

#[cfg(feature = "webui")]
#[derive(rust_embed::RustEmbed)]
#[folder = "$WEBUI_DIR"]
#[allow_missing = true]
struct Assets;

impl Ui {
  /// The UI `options` ask for from what `bundle` carries. `program_dir` is where
  /// `web.content_path` is relative to.
  pub fn new(bundle: Bundle, options: &Options, program_dir: &Path) -> Self {
    if options.headless || !options.web.ui || bundle == Bundle::None {
      warn!("Running in headless mode; web UI is DISABLED");
      return Self::Off;
    }
    if let Some(dir) = &options.web.content_path {
      let dir = program_dir.join(dir);
      info!(path = %dir.display(), "Serving static content from directory");
      return Self::Directory(dir);
    }
    #[cfg(feature = "webui")]
    {
      if Assets::get("index.html").is_none() {
        warn!("the web UI was not built into this program; it serves a page saying so");
      } else {
        info!("Serving the built-in web UI");
      }
      Self::Embedded
    }
    #[cfg(not(feature = "webui"))]
    Self::Off
  }

  /// Answers a request the API did not: an asset, `index.html` for a path of the app, or 404.
  pub async fn respond(&self, method: &Method, path: &str) -> Response {
    if self.is_off() || !(method == Method::GET || method == Method::HEAD) {
      return StatusCode::NOT_FOUND.into_response();
    }
    let relative = path.trim_start_matches('/');
    let last = relative.rsplit('/').next().unwrap_or("");
    if !last.contains('.') {
      return self.index().await;
    }
    match self.asset(relative).await {
      Some((bytes, mime)) => {
        let mut response = ([(header::CONTENT_TYPE, mime)], Body::from(bytes)).into_response();
        if relative.starts_with("_app/immutable/") {
          response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
        } else if last == "index.html" {
          response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
        response
      }
      None => StatusCode::NOT_FOUND.into_response(),
    }
  }

  fn is_off(&self) -> bool {
    matches!(self, Self::Off)
  }

  async fn index(&self) -> Response {
    let body = match self.asset("index.html").await {
      Some((bytes, _)) => Body::from(bytes),
      None => Body::from(NOT_BUILT),
    };
    (
      [
        (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
        (header::CACHE_CONTROL, "no-cache".to_string()),
      ],
      body,
    )
      .into_response()
  }

  /// The bytes of the asset at `relative` and its media type.
  async fn asset(&self, relative: &str) -> Option<(Vec<u8>, String)> {
    let mime = || {
      mime_guess::from_path(relative)
        .first_or_octet_stream()
        .to_string()
    };
    match self {
      Self::Off => None,
      #[cfg(feature = "webui")]
      Self::Embedded => Assets::get(relative).map(|file| (file.data.into_owned(), mime())),
      Self::Directory(dir) => {
        let path = inside(dir, relative)?;
        match tokio::fs::read(&path).await {
          Ok(bytes) => Some((bytes, mime())),
          Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
          Err(e) => {
            warn!(path = %path.display(), error = %e, "UI file not readable");
            None
          }
        }
      }
    }
  }
}

/// `relative` under `dir`, `None` when a part of it would leave `dir`.
fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
  let relative = Path::new(relative);
  relative
    .components()
    .all(|c| matches!(c, Component::Normal(_)))
    .then(|| dir.join(relative))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn a_content_directory_serves_assets_and_the_index_for_app_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>app</html>").unwrap();
    std::fs::create_dir_all(dir.path().join("_app/immutable")).unwrap();
    std::fs::write(dir.path().join("_app/immutable/x.js"), "js").unwrap();
    let ui = Ui::Directory(dir.path().to_path_buf());
    let index = ui.respond(&Method::GET, "/searches/abc").await;
    assert_eq!(index.status(), StatusCode::OK);
    assert_eq!(index.headers()[header::CACHE_CONTROL], "no-cache");
    let asset = ui.respond(&Method::GET, "/_app/immutable/x.js").await;
    assert_eq!(asset.headers()[header::CACHE_CONTROL], IMMUTABLE);
    assert_eq!(asset.headers()[header::CONTENT_TYPE], "text/javascript");
    assert_eq!(
      ui.respond(&Method::GET, "/missing.png").await.status(),
      StatusCode::NOT_FOUND
    );
    assert_eq!(
      ui.respond(&Method::GET, "/../secret.txt").await.status(),
      StatusCode::NOT_FOUND
    );
    assert_eq!(
      ui.respond(&Method::POST, "/").await.status(),
      StatusCode::NOT_FOUND
    );
    assert_eq!(
      Ui::Off.respond(&Method::GET, "/").await.status(),
      StatusCode::NOT_FOUND
    );
  }
}
