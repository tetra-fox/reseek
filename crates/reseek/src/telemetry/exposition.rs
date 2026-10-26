//! The Prometheus endpoint at `metrics.url`, outside the API and its authentication: HTTP
//! Basic with the configured credentials unless it is turned off.

use axum::Router;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tracing::{debug, info};

use crate::app::AppState;
use crate::config::MetricsAuthentication;

/// The type the Prometheus text format is served as.
pub const TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

/// What a refused request is told to answer with.
const CHALLENGE: &str = "Basic realm=\"metrics\"";

/// The route `metrics.url` names: the url with a leading `/` added when it has none.
pub fn path(url: &str) -> String {
  if url.starts_with('/') {
    url.to_string()
  } else {
    format!("/{url}")
  }
}

/// The endpoint's route at `metrics.url`, with a leading `/` added when it has none.
pub fn routes(app: &AppState) -> Router {
  let path = path(&app.startup.values.metrics.url);
  info!(url = path, "Publishing Prometheus metrics");
  let app = app.clone();
  Router::new().route(
    &path,
    get(move |headers: HeaderMap| {
      let app = app.clone();
      async move { exposition(&app, &headers).await }
    }),
  )
}

/// The metrics as text for a request with the credentials, 401 with a challenge otherwise.
async fn exposition(app: &AppState, headers: &HeaderMap) -> Response {
  let credentials = &app.startup.values.metrics.authentication;
  if !credentials.disabled && !authorized(headers, credentials) {
    debug!("metrics request without valid credentials refused");
    let mut refused = StatusCode::UNAUTHORIZED.into_response();
    refused.headers_mut().insert(
      header::WWW_AUTHENTICATE,
      HeaderValue::from_static(CHALLENGE),
    );
    return refused;
  }
  match app.services.reports.metrics_text().await {
    Ok(text) => ([(header::CONTENT_TYPE, TEXT)], text).into_response(),
    Err(e) => e.into_response(),
  }
}

/// Whether `headers` carry `Authorization: Basic` with exactly the configured username and
/// password. Each is compared in constant time, so the time taken tells nothing about either.
fn authorized(headers: &HeaderMap, credentials: &MetricsAuthentication) -> bool {
  let Some(value) = headers
    .get(header::AUTHORIZATION)
    .and_then(|v| v.to_str().ok())
  else {
    return false;
  };
  let Some((scheme, encoded)) = value.trim().split_once(' ') else {
    return false;
  };
  if !scheme.eq_ignore_ascii_case("basic") {
    return false;
  }
  let Ok(decoded) = STANDARD.decode(encoded.trim()) else {
    return false;
  };
  let Some(colon) = decoded.iter().position(|&b| b == b':') else {
    return false;
  };
  let (username, password) = (&decoded[..colon], &decoded[colon + 1..]);
  let same = |presented: &[u8], expected: &str| -> bool {
    Sha256::digest(presented)
      .ct_eq(&Sha256::digest(expected.as_bytes()))
      .into()
  };
  // both are compared whatever the first gives
  let user = same(username, &credentials.username);
  let pass = same(password, &credentials.password);
  user & pass
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{app, request, with};

  fn basic(user_password: &str) -> String {
    format!("Basic {}", STANDARD.encode(user_password))
  }

  #[tokio::test]
  async fn the_endpoint_takes_exactly_its_credentials() {
    let app = app(
      "[metrics]\nenabled = true\nurl = \"stats\"\n\
       [metrics.authentication]\nusername = \"prom\"\npassword = \"Secret\"\n",
      &[],
    )
    .await;
    let get = || request("GET", "/stats", None);
    let none = app.send(get()).await;
    assert_eq!(none.status, 401);
    assert_eq!(none.headers["www-authenticate"], CHALLENGE);
    assert!(none.body.is_empty());
    for wrong in [
      basic("prom:secret"),
      basic("prom:Secret "),
      basic("Prom:Secret"),
      "Basic not-base64!".to_string(),
      basic("promSecret"),
      format!("Bearer {}", STANDARD.encode("prom:Secret")),
    ] {
      let answer = app.send(with(get(), "authorization", &wrong)).await;
      assert_eq!(answer.status, 401, "{wrong}");
    }
    let good = app
      .send(with(get(), "authorization", &basic("prom:Secret")))
      .await;
    assert_eq!(good.status, 200);
    assert_eq!(good.headers["content-type"], TEXT);
    let text = String::from_utf8(good.body.to_vec()).unwrap();
    assert!(text.contains("# TYPE reseek_browse_requests_received counter\n"));
  }

  #[tokio::test]
  async fn the_endpoint_is_open_without_authentication_and_absent_when_off() {
    let open = app(
      "[metrics]\nenabled = true\n[metrics.authentication]\ndisabled = true\n",
      &[],
    )
    .await;
    assert_eq!(
      open.send(request("GET", "/metrics", None)).await.status,
      200
    );
    let off = app("", &[]).await;
    assert_eq!(off.send(request("GET", "/metrics", None)).await.status, 404);
    let moved = app("[metrics]\nurl = \"stats\"\n", &[]).await.with_page();
    assert_eq!(
      moved.send(request("GET", "/stats", None)).await.status,
      404,
      "the configured path is no UI page while the endpoint is off"
    );
    assert_eq!(moved.send(request("GET", "/other", None)).await.status, 200);
  }
}
