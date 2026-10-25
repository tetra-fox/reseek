//! A typed client of reseek's HTTP API and its event stream, over the same `api-types` the
//! daemon answers with, so a test or a tool and the daemon cannot disagree about a field.
//!
//! [`Client`] has one method per API call, named after what it does and documented with its
//! route; [`Client::live`] opens the event stream, which reconnects with `Last-Event-ID` on its
//! own. A call that is answered with an error returns [`Error::Api`] with the status and the
//! error body.
//!
//! ```no_run
//! # async fn example() -> api_client::Result<()> {
//! let client = api_client::Client::new("http://127.0.0.1:5030")?;
//! client.login("admin", "hunter2").await?;
//! let state = client.application().await?;
//! println!("connected: {}", state.server.is_connected);
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod application;
mod events;
mod files;
mod live;
mod messaging;
mod options;
mod relay;
mod searches;
mod session;
mod shares;
mod telemetry;
mod transfers;
mod users;

use std::sync::Arc;

use api_types::ErrorBody;
use bytes::Bytes;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use files::LocalFile;
pub use live::{Live, LiveEvent, LiveStream};
pub use relay::AgentCall;

/// What a call that failed failed with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// the request could not be sent, or the answer not read
  #[error(transparent)]
  Http(#[from] reqwest::Error),
  /// the daemon answered with an error
  #[error("{status}: {}", body.error)]
  Api {
    /// the status
    status: StatusCode,
    /// the error body
    body: ErrorBody,
  },
  /// the daemon answered with a status this client did not expect and no error body, such as
  /// the empty 404 of a route that does not exist
  #[error("{status}: {text}")]
  Unexpected {
    /// the status
    status: StatusCode,
    /// the body as text
    text: String,
  },
  /// the answer is not the JSON the call returns
  #[error("the answer is not what the call returns: {0}")]
  Decode(#[from] serde_json::Error),
  /// the query could not be written
  #[error("the query could not be written: {0}")]
  Query(#[from] serde_html_form::ser::Error),
  /// the base URL is not one
  #[error("not a base URL: {0}")]
  Base(String),
}

impl Error {
  /// The status the daemon answered with, when it answered.
  pub fn status(&self) -> Option<StatusCode> {
    match self {
      Error::Api { status, .. } | Error::Unexpected { status, .. } => Some(*status),
      Error::Http(e) => e.status(),
      _ => None,
    }
  }
}

/// The answer of a call.
pub type Result<T> = std::result::Result<T, Error>;

/// How the client says who it is, besides the session cookie [`Client::login`] keeps.
#[derive(Clone, Debug)]
pub enum Credential {
  /// an API token or an API key, sent as `Authorization: Bearer`
  Bearer(String),
  /// an API key, sent as `X-API-Key`
  ApiKey(String),
}

/// A client of one daemon. Cloning gives another handle to the same connections and cookies.
#[derive(Clone)]
pub struct Client {
  http: reqwest::Client,
  /// the daemon's base URL with its URL base, without a trailing `/`
  base: Arc<str>,
  credential: Option<Credential>,
}

impl Client {
  /// A client of the daemon at `base`, such as `http://127.0.0.1:5030` or
  /// `https://example.net/reseek`, keeping the cookies the daemon sets. HTTPS trusts the usual
  /// certificate authorities.
  pub fn new(base: &str) -> Result<Self> {
    let parsed = url::Url::parse(base).map_err(|e| Error::Base(format!("{base}: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
      return Err(Error::Base(format!("{base}: not http or https")));
    }
    let roots = rustls::RootCertStore {
      roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
      rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| Error::Base(e.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    // a request a session cookie authenticates must name this site as its origin, which it
    // is: the client holds the cookie itself
    let origin = parsed.origin().ascii_serialization();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
      reqwest::header::ORIGIN,
      origin
        .parse()
        .map_err(|_| Error::Base(format!("{base}: no origin")))?,
    );
    let http = reqwest::Client::builder()
      .use_preconfigured_tls(tls)
      .default_headers(headers)
      .cookie_store(true)
      .redirect(reqwest::redirect::Policy::none())
      .build()?;
    Ok(Self {
      http,
      base: base.trim_end_matches('/').into(),
      credential: None,
    })
  }

  /// The same client sending `credential` with every request.
  pub fn with_credential(mut self, credential: Credential) -> Self {
    self.credential = Some(credential);
    self
  }

  /// A request for `route`, with the credential.
  fn request(&self, method: Method, route: &Route) -> RequestBuilder {
    let mut url = format!("{}/api/v0{}", self.base, route.path);
    if !route.query.is_empty() {
      url.push('?');
      url.push_str(&route.query.join("&"));
    }
    let request = self.http.request(method, url);
    match &self.credential {
      Some(Credential::Bearer(token)) => request.bearer_auth(token),
      Some(Credential::ApiKey(key)) => request.header("x-api-key", key),
      None => request,
    }
  }

  fn get(&self, route: Route) -> RequestBuilder {
    self.request(Method::GET, &route)
  }

  fn post(&self, route: Route) -> RequestBuilder {
    self.request(Method::POST, &route)
  }

  fn put(&self, route: Route) -> RequestBuilder {
    self.request(Method::PUT, &route)
  }

  fn patch(&self, route: Route) -> RequestBuilder {
    self.request(Method::PATCH, &route)
  }

  fn delete(&self, route: Route) -> RequestBuilder {
    self.request(Method::DELETE, &route)
  }
}

/// The characters a path segment keeps as they are: those `encodeURIComponent` keeps.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'-')
  .remove(b'_')
  .remove(b'.')
  .remove(b'!')
  .remove(b'~')
  .remove(b'*')
  .remove(b'\'')
  .remove(b'(')
  .remove(b')');

/// A route under `/api/v0`, built a segment at a time, each segment percent-encoded once.
#[derive(Clone, Debug, Default)]
struct Route {
  path: String,
  query: Vec<String>,
}

impl Route {
  /// The route of fixed segments `path`, such as `transfers/downloads`.
  fn new(path: &str) -> Self {
    Self {
      path: format!("/{path}"),
      query: Vec::new(),
    }
  }

  /// The route with fixed segments appended.
  fn then(mut self, path: &str) -> Self {
    self.path.push('/');
    self.path.push_str(path);
    self
  }

  /// The route with one segment of any text appended.
  fn segment(mut self, text: &str) -> Self {
    self.path.push('/');
    self.path.extend(utf8_percent_encode(text, SEGMENT));
    self
  }

  /// The route with a name the daemon also takes as `?key=`, which a name a path segment cannot
  /// hold, `.` and `..`, is sent as.
  fn named(self, key: &str, name: &str) -> Self {
    if name == "." || name == ".." {
      let mut route = self.segment("_");
      route
        .query
        .push(format!("{key}={}", utf8_percent_encode(name, SEGMENT)));
      return route;
    }
    self.segment(name)
  }

  /// The route with a username, sent as `?username=` when a path cannot hold it.
  fn user(self, username: &str) -> Self {
    self.named("username", username)
  }

  /// The route with a room name, sent as `?roomName=` when a path cannot hold it.
  fn room(self, room: &str) -> Self {
    self.named("roomName", room)
  }

  /// The route with `query`'s fields as query parameters, a list as its parameter repeated.
  fn query<Q: Serialize>(mut self, query: &Q) -> Result<Self> {
    let text = serde_html_form::to_string(query)?;
    if !text.is_empty() {
      self.query.push(text);
    }
    Ok(self)
  }
}

/// Sends `request` and returns the answer when it is a success or a redirection: a 304 answers
/// a call that had nothing to do, a 302 sends a browser on.
async fn send(request: RequestBuilder) -> Result<Response> {
  let response = request.send().await?;
  let status = response.status();
  if status.is_success() || status.is_redirection() {
    return Ok(response);
  }
  Err(refused(response).await)
}

/// The error an answer that is no success carries: its error body, or its text when it has
/// none.
async fn refused(response: Response) -> Error {
  let status = response.status();
  let text = match response.text().await {
    Ok(text) => text,
    Err(e) => return Error::Http(e),
  };
  match serde_json::from_str::<ErrorBody>(&text) {
    Ok(body) => Error::Api { status, body },
    Err(_) => Error::Unexpected { status, text },
  }
}

/// The answer read as JSON.
async fn json<T: DeserializeOwned>(request: RequestBuilder) -> Result<T> {
  let bytes = send(request).await?.bytes().await?;
  Ok(serde_json::from_slice(&bytes)?)
}

/// The answer's status, its body ignored.
async fn status(request: RequestBuilder) -> Result<StatusCode> {
  Ok(send(request).await?.status())
}

/// The call succeeded.
async fn done(request: RequestBuilder) -> Result<()> {
  send(request).await.map(drop)
}

/// The answer's body as it is.
async fn bytes(request: RequestBuilder) -> Result<Bytes> {
  Ok(send(request).await?.bytes().await?)
}

/// The answer's body as text.
async fn text(request: RequestBuilder) -> Result<String> {
  Ok(send(request).await?.text().await?)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn segments_are_encoded_once_and_dot_names_go_in_the_query() {
    let route = Route::new("users").user("a/b c+%").then("browse");
    assert_eq!(route.path, "/users/a%2Fb%20c%2B%25/browse");
    let dots = Route::new("users").user("..").then("browse");
    assert_eq!(dots.path, "/users/_/browse");
    assert_eq!(dots.query, ["username=.."]);
    let room = Route::new("rooms/joined").room("Caf\u{e9}").then("users");
    assert_eq!(room.path, "/rooms/joined/Caf%C3%A9/users");
  }
}
