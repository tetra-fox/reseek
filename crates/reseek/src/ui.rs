//! The web UI: the bundle built into the program or a directory given by `web.content_path`,
//! with `index.html` served at the URL base.
//!
//! The UI is built with relative asset paths and a hash router, so one build works under any
//! URL base: assets are served as they are, `index.html` is never rewritten, and the page is
//! served only at the base itself, where its relative paths resolve. Any other path without a
//! file extension, such as a bookmark of `/searches/x`, is sent to the app's own address for
//! it, `{base}/#/searches/x`.

use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use tracing::{info, warn};

use crate::config::Options;

/// The page served when the program was built without a UI build.
const NOT_BUILT: &str = "<!doctype html><title>reseek</title><p>UI not built, run `just \
                         web-build`.</p>\n";

/// Hashed assets never change under their name.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Where the UI is served from.
#[derive(Clone, Debug)]
pub enum Ui {
  /// not served: headless or turned off
  Off,
  /// the bundle built into the program, or a page saying it was not built
  Embedded,
  /// a directory of the user's
  Directory(PathBuf),
}

#[derive(rust_embed::RustEmbed)]
#[folder = "$WEBUI_DIR"]
#[allow_missing = true]
struct Assets;

impl Ui {
  /// The UI `options` ask for. `program_dir` is where `web.content_path` is relative to.
  pub fn new(options: &Options, program_dir: &Path) -> Self {
    if options.headless || !options.web.ui {
      warn!("Running in headless mode; web UI is DISABLED");
      return Self::Off;
    }
    if let Some(dir) = &options.web.content_path {
      let dir = program_dir.join(dir);
      info!(path = %dir.display(), "Serving static content from directory");
      return Self::Directory(dir);
    }
    if Assets::get("index.html").is_none() {
      warn!("the web UI was not built into this program; it serves a page saying so");
    } else {
      info!("Serving the built-in web UI");
    }
    Self::Embedded
  }

  /// Whether a request for `path`, with the URL base taken off, is answered with the app's
  /// page, `index.html`: the base itself or the page by name.
  pub fn is_page(&self, method: &Method, path: &str) -> bool {
    self.serves(method) && (path == "/" || path == "/index.html")
  }

  /// Answers a request the API did not take, whose `uri` has the URL `base` taken off:
  /// `index.html` at the base, a permanent redirect to the base with the app's address after
  /// `#` for any other path without a file extension, an asset, or 404. `bare_base` says the
  /// request named the base without its trailing `/`, which is sent to the base with it.
  pub async fn respond(&self, method: &Method, uri: &Uri, base: &str, bare_base: bool) -> Response {
    let path = uri.path();
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    if bare_base && self.is_page(method, path) {
      return moved(&format!("{base}/{query}"));
    }
    if self.is_page(method, path) {
      return self.index().await;
    }
    if !self.serves(method) {
      return StatusCode::NOT_FOUND.into_response();
    }
    let relative = path.trim_start_matches('/');
    let last = relative.rsplit('/').next().unwrap_or("");
    if !last.contains('.') {
      return moved(&format!("{base}/#/{relative}{query}"));
    }
    match self.asset(relative).await {
      Some((bytes, mime)) => {
        let mut response = ([(header::CONTENT_TYPE, mime)], Body::from(bytes)).into_response();
        if relative.starts_with("_app/immutable/") {
          response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
        }
        response
      }
      None => StatusCode::NOT_FOUND.into_response(),
    }
  }

  /// Whether a request with `method` can be answered from the UI at all.
  fn serves(&self, method: &Method) -> bool {
    !matches!(self, Self::Off) && (method == Method::GET || method == Method::HEAD)
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

/// A 301 to `location`, which a browser keeps in place of the address it asked for.
fn moved(location: &str) -> Response {
  match HeaderValue::from_str(location) {
    Ok(location) => (
      StatusCode::MOVED_PERMANENTLY,
      [(header::LOCATION, location)],
    )
      .into_response(),
    // the request's path and query are header text, but a URL base written with characters no
    // header holds, such as non-ASCII ones, is not
    Err(e) => {
      warn!(location, error = %e, "redirect not sent; the URL base cannot be sent in a header");
      StatusCode::NOT_FOUND.into_response()
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

  /// The answer of `ui` to `method` on `uri`, under the base `/reseek`.
  async fn answer(ui: &Ui, method: Method, uri: &str) -> Response {
    ui.respond(&method, &uri.parse().unwrap(), "/reseek", false)
      .await
  }

  fn location(response: &Response) -> &str {
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    response.headers()[header::LOCATION].to_str().unwrap()
  }

  #[tokio::test]
  async fn a_content_directory_serves_assets_and_the_index_at_the_base() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>app</html>").unwrap();
    std::fs::create_dir_all(dir.path().join("_app/immutable")).unwrap();
    std::fs::write(dir.path().join("_app/immutable/x.js"), "js").unwrap();
    let ui = Ui::Directory(dir.path().to_path_buf());
    for page in ["/", "/index.html"] {
      let index = answer(&ui, Method::GET, page).await;
      assert_eq!(index.status(), StatusCode::OK);
      assert_eq!(index.headers()[header::CACHE_CONTROL], "no-cache");
    }
    let asset = answer(&ui, Method::GET, "/_app/immutable/x.js").await;
    assert_eq!(asset.headers()[header::CACHE_CONTROL], IMMUTABLE);
    assert_eq!(asset.headers()[header::CONTENT_TYPE], "text/javascript");
    for (uri, status) in [
      ("/missing.png", StatusCode::NOT_FOUND),
      ("/../secret.txt", StatusCode::NOT_FOUND),
    ] {
      assert_eq!(
        answer(&ui, Method::GET, uri).await.status(),
        status,
        "{uri}"
      );
    }
    assert_eq!(
      answer(&ui, Method::POST, "/").await.status(),
      StatusCode::NOT_FOUND
    );
    assert_eq!(
      answer(&Ui::Off, Method::GET, "/").await.status(),
      StatusCode::NOT_FOUND
    );
  }

  #[tokio::test]
  async fn an_app_path_is_sent_to_its_address_in_the_app() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>app</html>").unwrap();
    let ui = Ui::Directory(dir.path().to_path_buf());
    let moved = answer(&ui, Method::GET, "/searches/abc?tab=files&x=%20y").await;
    assert_eq!(location(&moved), "/reseek/#/searches/abc?tab=files&x=%20y");
    let head = answer(&ui, Method::HEAD, "/system/").await;
    assert_eq!(location(&head), "/reseek/#/system/");
    let at_root = Ui::Directory(dir.path().to_path_buf())
      .respond(&Method::GET, &"/downloads".parse().unwrap(), "", false)
      .await;
    assert_eq!(location(&at_root), "/#/downloads");
    let bare = ui
      .respond(&Method::GET, &"/?a=1".parse().unwrap(), "/reseek", true)
      .await;
    assert_eq!(location(&bare), "/reseek/?a=1");
    assert_eq!(
      answer(&Ui::Off, Method::GET, "/searches").await.status(),
      StatusCode::NOT_FOUND
    );
    let embedded = answer(&Ui::Embedded, Method::GET, "/browse?user=a").await;
    assert_eq!(location(&embedded), "/reseek/#/browse?user=a");
    let page = answer(&Ui::Embedded, Method::GET, "/").await;
    assert_eq!(page.status(), StatusCode::OK);
  }
}
