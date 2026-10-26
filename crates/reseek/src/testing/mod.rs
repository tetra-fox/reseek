//! An in-memory daemon for handler tests: a configuration in a temporary directory, the whole
//! request pipeline, and requests sent to it without a socket; a reader for its event stream;
//! and a stand-in OpenID Connect provider.

pub mod issuer;
pub mod routes;

use std::ffi::OsString;
use std::net::SocketAddr;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use cascade::{Overrides, Watcher};
use http_body_util::BodyExt;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use crate::app::AppState;
use crate::auth::Auth;
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
  /// the status
  pub status: StatusCode,
  /// the headers
  pub headers: HeaderMap,
  /// the whole body
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
pub async fn app(config: &str, env: &[(&str, &str)]) -> TestApp {
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
  let options = watcher.current().values.clone();
  let paths = defaults.resolve(&options).unwrap();
  paths.create().unwrap();
  let auth = Auth::open(&paths.databases(&options)).await.unwrap();
  let state = AppState::new(
    watcher,
    std::sync::Arc::new(LogBuffer::default()),
    auth,
    paths,
    CancellationToken::new(),
  )
  .await
  .unwrap();
  state.spawn_tasks();
  TestApp {
    dir,
    router: pipeline::app(state.clone(), Ui::Off),
    state,
  }
}

impl TestApp {
  /// The same daemon serving a web UI whose page is `<html>app</html>`.
  pub fn with_page(mut self) -> Self {
    let web = self.dir.path().join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<html>app</html>").unwrap();
    self.router = pipeline::app(self.state.clone(), Ui::Directory(web));
    self
  }

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

  /// Sends `request` as if from `from` and reads the answer as an event stream.
  pub async fn stream_from(&self, from: &str, mut request: Request<Body>) -> Stream {
    request.extensions_mut().insert(Peer {
      remote: Some(from.parse::<SocketAddr>().unwrap()),
      transport: Transport::Plain,
    });
    let response = self.router.clone().oneshot(request).await.unwrap();
    Stream {
      status: response.status(),
      headers: response.headers().clone(),
      body: response.into_body(),
      text: String::new(),
    }
  }
}

/// An answer read as server-sent events.
pub struct Stream {
  /// the status
  pub status: StatusCode,
  /// the headers
  pub headers: HeaderMap,
  body: Body,
  text: String,
}

/// One server-sent event.
#[derive(Debug)]
pub struct Sse {
  /// its `id:` line
  pub id: Option<u64>,
  /// its `event:` line
  pub event: String,
  /// its `data:` line, read as JSON
  pub data: serde_json::Value,
}

impl Stream {
  /// The next event, skipping comments and `retry`; `None` when the stream ended.
  pub async fn next(&mut self) -> Option<Sse> {
    loop {
      if let Some(end) = self.text.find("\n\n") {
        let block: String = self.text.drain(..end + 2).collect();
        let mut sse = Sse {
          id: None,
          event: String::new(),
          data: serde_json::Value::Null,
        };
        for line in block.lines() {
          if let Some(id) = line.strip_prefix("id: ") {
            sse.id = Some(id.parse().unwrap());
          } else if let Some(event) = line.strip_prefix("event: ") {
            sse.event = event.to_string();
          } else if let Some(data) = line.strip_prefix("data: ") {
            sse.data = serde_json::from_str(data).unwrap();
          }
        }
        if !sse.event.is_empty() {
          return Some(sse);
        }
        continue;
      }
      let frame = tokio::time::timeout(std::time::Duration::from_secs(10), self.body.frame())
        .await
        .expect("the stream sent nothing for 10 s")?
        .unwrap();
      if let Ok(data) = frame.into_data() {
        self.text.push_str(std::str::from_utf8(&data).unwrap());
      }
    }
  }

  /// The type of each event until `stop` is seen, `stop` included.
  pub async fn until(&mut self, stop: &str) -> Vec<Sse> {
    let mut seen = Vec::new();
    while let Some(sse) = self.next().await {
      let done = sse.event == stop;
      seen.push(sse);
      if done {
        return seen;
      }
    }
    panic!("the stream ended before {stop}: {seen:?}");
  }
}

/// `request` with the header `name` set to `value`.
pub fn with(mut request: Request<Body>, name: &'static str, value: &str) -> Request<Body> {
  request.headers_mut().append(name, value.parse().unwrap());
  request
}

/// The `name=value` part of each `Set-Cookie` of `headers` naming `name`.
pub fn set_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
  headers
    .get_all("set-cookie")
    .iter()
    .filter_map(|value| value.to_str().ok())
    .map(|value| value.split(';').next().unwrap_or_default().to_string())
    .find(|pair| pair.starts_with(&format!("{name}=")))
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
