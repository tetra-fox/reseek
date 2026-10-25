//! An in-memory daemon for handler tests: a configuration in a temporary directory, the whole
//! request pipeline, and requests sent to it without a socket.

use std::ffi::OsString;
use std::net::SocketAddr;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use cascade::{Overrides, Watcher};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::app::AppState;
use crate::config::load::{self, ConfigFiles, Sources};
use crate::http::{Peer, Transport, pipeline};
use crate::logging::LogBuffer;
use crate::paths::Defaults;
use crate::ui::Ui;

/// A daemon serving from a temporary directory.
pub struct TestApp {
  /// the application directory, which holds `config.toml`
  pub dir: tempfile::TempDir,
  /// the request pipeline
  pub router: Router,
  /// what the handlers share
  pub state: AppState,
}

/// What a request was answered with.
pub struct Answer {
  pub status: StatusCode,
  pub headers: HeaderMap,
  pub body: Bytes,
}

impl Answer {
  /// The body as JSON.
  pub fn json(&self) -> serde_json::Value {
    serde_json::from_slice(&self.body)
      .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&self.body)))
  }
}

/// A daemon whose `config.toml` holds `config`, with the environment `env`.
pub fn app(config: &str, env: &[(&str, &str)]) -> TestApp {
  let dir = tempfile::tempdir().unwrap();
  std::fs::write(dir.path().join("config.toml"), config).unwrap();
  let defaults = Defaults::new(Some(dir.path()));
  let sources = Sources {
    env: env
      .iter()
      .map(|(k, v)| (OsString::from(k), OsString::from(v)))
      .collect(),
    files: ConfigFiles::new(&[], dir.path()),
    cli: Vec::new(),
  };
  let overrides = Overrides::load(dir.path().join("overrides.toml")).unwrap();
  let watcher = Watcher::new(load::layers(&sources, &defaults, Some(overrides))).unwrap();
  let state = AppState::new(watcher, std::sync::Arc::new(LogBuffer::default())).unwrap();
  TestApp {
    dir,
    router: pipeline::app(state.clone(), Ui::Off),
    state,
  }
}

impl TestApp {
  /// Sends `request` as if from `from` over plain HTTP.
  pub async fn send_from(&self, from: &str, mut request: Request<Body>) -> Answer {
    request.extensions_mut().insert(Peer {
      remote: Some(from.parse::<SocketAddr>().unwrap()),
      transport: Transport::Plain,
    });
    let response = self.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    Answer {
      status,
      headers,
      body,
    }
  }

  /// Sends `request` from the loopback address.
  pub async fn send(&self, request: Request<Body>) -> Answer {
    self.send_from("127.0.0.1:40000", request).await
  }
}

/// A request to `uri` with `Host: localhost` and, when given, a JSON body.
pub fn request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
  let builder = Request::builder()
    .method(method)
    .uri(uri)
    .header("host", "localhost");
  match body {
    Some(body) => builder
      .header("content-type", "application/json")
      .body(Body::from(body.to_string()))
      .unwrap(),
    None => builder.body(Body::empty()).unwrap(),
  }
}
