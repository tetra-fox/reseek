//! The request pipeline, outermost first: CORS, the HTTPS redirect and HSTS, the URL base taken
//! off the path, a span and log line per request, a panic turned into a 500, then the API
//! with its authentication, `/health`, the Prometheus endpoint when `metrics.enabled` is on,
//! the OpenAPI document and Swagger UI when `feature.swagger` is on, the 404 for unknown API
//! paths, and the web UI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};
use tracing::{Instrument, debug, info, info_span};

use cascade::Watcher;

use super::{Peer, Transport};
use crate::api::{self, error::ApiError};
use crate::app::AppState;
use crate::auth::{self, Presented, client, networks};
use crate::config::Options;
use crate::telemetry;
use crate::ui::Ui;

/// HSTS for 30 days.
const HSTS: &str = "max-age=2592000";

/// How long a browser may keep a CORS preflight answer.
const PREFLIGHT_MAX_AGE: Duration = Duration::from_secs(3600);

/// The whole application for `state`, with the UI served as `ui` says. The options that shape
/// the pipeline are the ones the daemon started with.
pub fn app(state: AppState, ui: Ui) -> Router {
  let startup = state.startup.values.clone();
  let base = state.url_base.clone();
  let ui = Arc::new(ui);
  let mut inner = Router::new()
    .merge(api::router(state.clone()))
    .route("/health", get(health));
  if startup.metrics.enabled {
    inner = inner.merge(telemetry::exposition::routes(&state));
  }
  if startup.feature.swagger {
    info!("Publishing Swagger documentation to /swagger");
    inner = inner.merge(api::swagger(&base));
  }
  let inner = inner
    .fallback({
      let app = state.clone();
      move |request: Request| {
        let ui = ui.clone();
        let app = app.clone();
        async move { fallback(&app, &ui, request).await }
      }
    })
    .layer(CatchPanicLayer::custom(panicked));
  let logging = startup.web.logging;
  let https = startup.web.https.clone();
  let force = (https.force && !https.disabled).then_some(https.port);
  let config = state.config.clone();
  let service = ServiceBuilder::new()
    .option_layer(cors(&startup))
    .layer(middleware::from_fn(move |request, next| {
      let config = config.clone();
      async move {
        match force {
          Some(port) => https_only(&config, port, request, next).await,
          None => next.run(request).await,
        }
      }
    }))
    .map_request(move |request| without_base(&base, request))
    .layer(middleware::from_fn(move |request, next| {
      traced(logging, request, next)
    }))
    .service(inner);
  Router::new().fallback_service(service)
}

/// The URL base as the pipeline uses it: a leading `/`, no trailing one, empty for the root.
pub fn url_base(configured: &str) -> String {
  let trimmed = configured.trim().trim_end_matches('/');
  if trimmed.is_empty() {
    return String::new();
  }
  if trimmed.starts_with('/') {
    trimmed.to_string()
  } else {
    format!("/{trimmed}")
  }
}

/// Marks a request for the URL base without its trailing `/`, whose path is `/` once the base
/// is taken off.
#[derive(Clone, Copy)]
struct BareBase;

/// `request` with the URL base taken off its path. A path outside the base is left as it is.
fn without_base(base: &str, mut request: Request) -> Request {
  if base.is_empty() {
    return request;
  }
  let path = request.uri().path();
  let rest = match path.strip_prefix(base) {
    Some("") => {
      request.extensions_mut().insert(BareBase);
      "/"
    }
    Some(rest) if rest.starts_with('/') => rest,
    _ => return request,
  };
  let rewritten = match request.uri().query() {
    Some(query) => format!("{rest}?{query}"),
    None => rest.to_string(),
  };
  if let Ok(uri) = rewritten.parse::<Uri>() {
    *request.uri_mut() = uri;
  }
  request
}

/// Sends plain HTTP requests to HTTPS on `port` and adds HSTS to HTTPS answers, except for
/// `localhost`. The scheme is the one the client used, which a trusted reverse proxy reports in
/// `X-Forwarded-Proto`, so a proxy that ends TLS in front does not loop. Requests over the unix
/// socket are left alone.
async fn https_only(
  config: &Watcher<Options>,
  port: u16,
  request: Request,
  next: Next,
) -> Response {
  let Some(peer) = request.extensions().get::<Peer>().copied() else {
    return ApiError::internal("a request arrived without its connection").into_response();
  };
  if peer.transport == Transport::Unix {
    return next.run(request).await;
  }
  let proxies = networks::ranges(&config.current().values.web.trusted_proxies);
  let client = client::resolve(&peer, request.uri(), request.headers(), &proxies);
  if client.https {
    let mut response = next.run(request).await;
    if client.host.as_deref() != Some("localhost") {
      response.headers_mut().insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static(HSTS),
      );
    }
    return response;
  }
  let Some(host) = client.host else {
    return ApiError::bad_request("A Host header is required to redirect to HTTPS").into_response();
  };
  let path = request.uri().path_and_query().map_or("/", |p| p.as_str());
  // an IPv6 address takes its brackets back in the URL
  let host = if host.parse::<std::net::Ipv6Addr>().is_ok() {
    format!("[{host}]")
  } else {
    host
  };
  Redirect::temporary(&format!("https://{host}:{port}{path}")).into_response()
}

/// The CORS layer `web.cors.allowed_origins` asks for: none when it is empty, any origin
/// without credentials for `*`, else the listed origins with credentials. A request from
/// another site's page is refused a trusted network's or a proxy's principal, so `*` shows it
/// nothing those alone would open.
fn cors(options: &Options) -> Option<CorsLayer> {
  let origins = &options.web.cors.allowed_origins;
  if origins.is_empty() {
    return None;
  }
  let layer = CorsLayer::new()
    .allow_methods(AllowMethods::mirror_request())
    .allow_headers(AllowHeaders::mirror_request())
    .expose_headers([
      header::HeaderName::from_static("x-total-count"),
      header::HeaderName::from_static("x-url-base"),
    ])
    .max_age(PREFLIGHT_MAX_AGE);
  if origins.iter().any(|o| o == "*") {
    return Some(layer.allow_origin(AllowOrigin::any()));
  }
  let listed: Vec<HeaderValue> = origins
    .iter()
    .filter_map(|o| match HeaderValue::from_str(o) {
      Ok(value) => Some(value),
      Err(e) => {
        tracing::warn!(origin = o, error = %e, "CORS origin is not a valid header value; ignored");
        None
      }
    })
    .collect();
  Some(
    layer
      .allow_origin(AllowOrigin::list(listed))
      .allow_credentials(true),
  )
}

/// Runs the request in its own span and logs it when it is answered: at debug always, and at
/// info with `web.logging`. The path is logged without the credentials a query may carry.
async fn traced(logging: bool, request: Request, next: Next) -> Response {
  let method = request.method().clone();
  let path = loggable(request.uri());
  let id = uuid::Uuid::new_v4();
  let span = info_span!(
    "request",
    %method,
    path,
    principal = tracing::field::Empty,
    %id
  );
  let started = Instant::now();
  let response = next.run(request).instrument(span.clone()).await;
  let status = response.status().as_u16();
  let ms = started.elapsed().as_millis();
  span.in_scope(|| {
    if logging {
      info!(status, ms, "HTTP request answered");
    } else {
      debug!(status, ms, "HTTP request answered");
    }
  });
  response
}

/// The query parameters that carry a credential: the event stream's `access_token` and a file
/// link's `t`.
const SECRET_PARAMETERS: &[&str] = &["access_token", "t"];

/// The path and query of `uri` with the value of every credential parameter taken out. A name
/// is compared decoded, as the handlers read it, so `access%5Ftoken` is masked too.
fn loggable(uri: &Uri) -> String {
  let Some(query) = uri.query() else {
    return uri.path().to_string();
  };
  let secret = |name: &str| {
    url::form_urlencoded::parse(name.as_bytes())
      .next()
      .is_some_and(|(decoded, _)| SECRET_PARAMETERS.contains(&decoded.as_ref()))
  };
  let query = query
    .split('&')
    .map(|pair| match pair.split_once('=') {
      Some((name, _)) if secret(name) => format!("{name}=*****"),
      _ => pair.to_string(),
    })
    .collect::<Vec<_>>()
    .join("&");
  format!("{}?{query}", uri.path())
}

/// The answer to a handler that panicked: a 500 whose log line, in the request's span, carries
/// the panic's message.
fn panicked(payload: Box<dyn std::any::Any + Send>) -> Response {
  let message = crate::logging::panic_message(payload.as_ref());
  ApiError::internal(format!("a handler panicked: {message}")).into_response()
}

async fn health() -> &'static str {
  "Healthy"
}

/// A request no route took: 404 with no body under `/api`, the metrics path or `/swagger`,
/// else the web UI. The app's page carries the cookies the page response sets, such as a
/// reverse proxy user's new session.
async fn fallback(app: &AppState, ui: &Ui, request: Request) -> Response {
  let path = request.uri().path();
  let under = |prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
  let metrics = telemetry::exposition::path(&app.startup.values.metrics.url);
  if under("/api") || under(&metrics) || under("/swagger") {
    return StatusCode::NOT_FOUND.into_response();
  }
  let page = ui.is_page(request.method(), path);
  let bare_base = request.extensions().get::<BareBase>().is_some();
  let mut response = ui
    .respond(request.method(), request.uri(), &app.url_base, bare_base)
    .await;
  if !page || response.status() != StatusCode::OK {
    return response;
  }
  let Some(peer) = request.extensions().get::<Peer>().copied() else {
    return ApiError::internal("a request arrived without its connection").into_response();
  };
  let presented = Presented::read(
    &peer,
    request.uri(),
    request.headers(),
    &app.config.current().values,
  );
  match auth::page_session(app, &presented).await {
    Ok(cookies) => {
      for cookie in cookies {
        response.headers_mut().append(header::SET_COOKIE, cookie);
      }
    }
    // the page is still served; its requests then authenticate as they can
    Err(e) => tracing::warn!(error = ?e, "no session started for the page"),
  }
  response
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_base_comes_off_paths_under_it_only() {
    let strip = |uri: &str| {
      without_base(
        "/reseek",
        Request::builder()
          .uri(uri)
          .body(axum::body::Body::empty())
          .unwrap(),
      )
      .uri()
      .to_string()
    };
    assert_eq!(strip("/reseek/api/v0/options?x=1"), "/api/v0/options?x=1");
    assert_eq!(strip("/reseek"), "/");
    assert_eq!(strip("/reseekx/api"), "/reseekx/api");
    assert_eq!(strip("/api/v0/options"), "/api/v0/options");
  }

  #[test]
  fn a_panic_is_a_500_whose_log_line_has_the_panic_message() {
    use tracing_subscriber::layer::SubscriberExt;
    let buffer = std::sync::Arc::new(crate::logging::LogBuffer::default());
    let subscriber =
      tracing_subscriber::registry().with(crate::logging::BufferLayer(buffer.clone()));
    let answer = tracing::subscriber::with_default(subscriber, || {
      panicked(Box::new(String::from("index out of bounds")))
    });
    assert_eq!(answer.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let records = buffer.snapshot();
    assert!(
      records[0].message.contains("index out of bounds"),
      "{records:?}"
    );
  }

  #[test]
  fn credentials_in_a_query_are_not_logged() {
    let uri: Uri = "/api/v0/live?topics=logs&access_token=slsk_secret"
      .parse()
      .unwrap();
    assert_eq!(
      loggable(&uri),
      "/api/v0/live?topics=logs&access_token=*****"
    );
    let link: Uri = "/api/v0/files/downloads/files/YQ/content?t=eyJ.x.y&disposition=inline"
      .parse()
      .unwrap();
    assert_eq!(
      loggable(&link),
      "/api/v0/files/downloads/files/YQ/content?t=*****&disposition=inline"
    );
    let encoded: Uri = "/api/v0/live?access%5Ftoken=slsk_secret&%74=eyJ.x.y&topics=logs"
      .parse()
      .unwrap();
    assert_eq!(
      loggable(&encoded),
      "/api/v0/live?access%5Ftoken=*****&%74=*****&topics=logs"
    );
  }
}

#[cfg(test)]
mod app_tests {
  use crate::testing::{OUTSIDE, app, request};

  #[tokio::test]
  async fn health_unknown_api_paths_and_the_url_base() {
    let app = app(
      "[web]\nurl_base = \"/reseek/\"\n[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
      &[],
    ).await;
    let health = app.send(request("GET", "/health", None)).await;
    assert_eq!(health.status, 200);
    assert_eq!(health.body, "Healthy");
    let unknown = app.send(request("GET", "/api/v0/nothing", None)).await;
    assert_eq!(unknown.status, 404);
    assert!(unknown.body.is_empty());
    let based = app
      .send(request("GET", "/reseek/api/v0/options", None))
      .await;
    assert_eq!(based.status, 200);
    assert_eq!(
      app
        .send(request("GET", "/reseek/health", None))
        .await
        .status,
      200
    );
    assert_eq!(
      app.send(request("GET", "/app/page", None)).await.status,
      404,
      "no UI"
    );
  }

  #[tokio::test]
  async fn old_app_paths_under_the_base_are_sent_into_the_app() {
    let app = app("[web]\nurl_base = \"/reseek\"\n", &[])
      .await
      .with_page();
    let moved = app
      .send(request("GET", "/reseek/searches/x?q=a%20b", None))
      .await;
    assert_eq!(moved.status, 301);
    assert_eq!(moved.headers["location"], "/reseek/#/searches/x?q=a%20b");
    let bare = app.send(request("GET", "/reseek", None)).await;
    assert_eq!(bare.status, 301);
    assert_eq!(bare.headers["location"], "/reseek/");
    let page = app.send(request("GET", "/reseek/", None)).await;
    assert_eq!(page.status, 200);
    assert_eq!(page.body, "<html>app</html>");
    let api = app
      .send(request("GET", "/reseek/api/v0/nothing", None))
      .await;
    assert_eq!(api.status, 404, "a mistyped API path is no app path");
    assert_eq!(
      app
        .send(request("GET", "/reseek/health", None))
        .await
        .status,
      200
    );
    assert_eq!(
      app
        .send(request("GET", "/reseek/_app/missing.js", None))
        .await
        .status,
      404
    );
  }

  #[tokio::test]
  async fn swagger_is_served_only_when_turned_on() {
    let on = app(
      "[feature]\nswagger = true\n[web]\nurl_base = \"/reseek\"\n",
      &[],
    )
    .await;
    let document = on
      .send(request("GET", "/reseek/swagger/v0/swagger.json", None))
      .await;
    assert_eq!(document.status, 200);
    assert!(document.json()["paths"]["/api/v0/application"].is_object());
    let page = on.send(request("GET", "/reseek/swagger/", None)).await;
    assert_eq!(page.status, 200);
    let config = on
      .send(request(
        "GET",
        "/reseek/swagger/swagger-initializer.js",
        None,
      ))
      .await;
    let script = String::from_utf8_lossy(&config.body);
    assert!(
      script.contains("/reseek/swagger/v0/swagger.json"),
      "{script}"
    );
    let off = app("", &[]).await;
    let missing = off
      .send(request("GET", "/swagger/v0/swagger.json", None))
      .await;
    assert_eq!(missing.status, 404);
    assert_eq!(
      off.send(request("GET", "/swagger/", None)).await.status,
      404
    );
  }

  #[tokio::test]
  async fn forced_https_redirects_plain_requests() {
    let app = app("[web.https]\nforce = true\nport = 8443\n", &[]).await;
    let answer = app.send(request("GET", "/health?x=1", None)).await;
    assert_eq!(answer.status, 307);
    assert_eq!(
      answer.headers["location"],
      "https://localhost:8443/health?x=1"
    );
    let mut v6 = request("GET", "/", None);
    v6.headers_mut().insert("host", "[::1]".parse().unwrap());
    assert_eq!(
      app.send(v6).await.headers["location"],
      "https://[::1]:8443/"
    );
  }

  #[tokio::test]
  async fn forced_https_believes_the_scheme_a_trusted_proxy_reports() {
    let app = app(
      "[web]\ntrusted_proxies = [\"127.0.0.1/32\"]\n[web.https]\nforce = true\nport = 8443\n",
      &[],
    )
    .await;
    let proxied = |proto: &'static str| {
      let mut r = request("GET", "/health", None);
      r.headers_mut()
        .insert("x-forwarded-proto", proto.parse().unwrap());
      r.headers_mut()
        .insert("x-forwarded-host", "reseek.example.net".parse().unwrap());
      r
    };
    let tls_at_proxy = app.send(proxied("https")).await;
    assert_eq!(
      tls_at_proxy.status, 200,
      "no redirect loop behind the proxy"
    );
    assert_eq!(
      tls_at_proxy.headers["strict-transport-security"],
      "max-age=2592000"
    );
    let plain = app.send(proxied("http")).await;
    assert_eq!(plain.status, 307);
    assert_eq!(
      plain.headers["location"],
      "https://reseek.example.net:8443/health"
    );
    let untrusted = app.send_from(OUTSIDE, proxied("https")).await;
    assert_eq!(untrusted.status, 307, "only a trusted proxy is believed");
  }
}
