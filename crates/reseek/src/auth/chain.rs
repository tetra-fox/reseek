//! The chain that settles who a request is, in order: an API token, a configuration API key,
//! the session cookie, a trusted reverse proxy's forwarded identity, a trusted network. A
//! token or key that is presented but not valid fails at once; a session cookie that is not
//! valid is cleared and the next ways are tried.

use std::net::{IpAddr, Ipv4Addr};

use api_types::auth::{Principal, Via};
use api_types::events::{Event, SessionCreated};
use axum::http::{HeaderMap, HeaderValue, Uri, header};
use chrono::Utc;
use soulseek_client::LogLimiter;
use tracing::{debug, info, warn};

use super::cookies::{self, Scope};
use super::sessions::{self, NewSession, Origin, StoredSession};
use super::{Client, TOKEN_PREFIX, client, keys, networks, proxy, tokens};
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::config::cidr::unmapped;
use crate::http::Peer;

/// The path of the event stream, the one route that takes a credential in its query.
const LIVE_PATH: &str = "/api/v0/live";

static PROXY_REFUSED: LogLimiter = LogLimiter::new();

/// Everything a request presented that says who it is, kept so the same credential can be
/// checked again later, as the event stream does.
#[derive(Clone)]
pub struct Presented {
  /// the client, after trusted proxies
  pub client: Client,
  /// the connection's own address, before any forwarding
  peer: IpAddr,
  /// the API token or key from `Authorization: Bearer`, `X-API-Key`, or for the event stream
  /// `access_token`
  credential: Option<String>,
  /// the session cookie's value
  cookie: Option<String>,
  /// the request's headers, for a proxy's forwarded identity and the user agent
  headers: HeaderMap,
}

impl Presented {
  /// What a request from `peer` to `uri` with `headers` presents, with the client resolved
  /// through `options.web.trusted_proxies`.
  pub fn read(
    peer: &Peer,
    uri: &Uri,
    headers: &HeaderMap,
    options: &crate::config::Options,
  ) -> Self {
    let proxies = networks::ranges(&options.web.trusted_proxies);
    let client = client::resolve(peer, uri, headers, &proxies);
    let in_query = || {
      (uri.path() == LIVE_PATH)
        .then(|| uri.query())
        .flatten()
        .and_then(|query| {
          url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "access_token")
            .map(|(_, value)| value.into_owned())
        })
    };
    Self {
      peer: peer
        .remote
        .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |addr| unmapped(addr.ip())),
      credential: presented_credential(headers)
        .map(str::to_string)
        .or_else(in_query)
        .filter(|credential| !credential.is_empty()),
      cookie: cookies::find(headers, cookies::SESSION).map(str::to_string),
      headers: headers.clone(),
      client,
    }
  }

  /// The browser's user agent, when it sent one.
  pub fn user_agent(&self) -> Option<String> {
    self
      .headers
      .get(header::USER_AGENT)
      .and_then(|value| value.to_str().ok())
      .map(str::to_string)
  }
}

/// Leaves the credential, the cookie and the headers out, so they are never logged.
impl std::fmt::Debug for Presented {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Presented")
      .field("client", &self.client)
      .field("peer", &self.peer)
      .field("credential", &self.credential.is_some())
      .field("cookie", &self.cookie.is_some())
      .finish_non_exhaustive()
  }
}

/// Who the chain found a request to be, and the cookies its answer must set.
#[derive(Default)]
pub struct Identified {
  /// the principal, `None` for an anonymous request
  pub principal: Option<Principal>,
  /// the session the request came with
  pub session: Option<StoredSession>,
  /// the principal came from the session cookie alone
  pub by_cookie: bool,
  /// `Set-Cookie` values for the answer
  pub cookies: Vec<HeaderValue>,
}

/// Whether the chain runs for a new request, or checks again the credential of one already
/// accepted, which counts no failures and starts no sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
  /// a new request
  Request,
  /// a credential accepted before
  Again,
}

/// The chain itself: the first way in that applies decides.
pub async fn identify(
  app: &AppState,
  presented: &Presented,
  check: Check,
) -> Result<Identified, ApiError> {
  let current = app.config.current();
  let options = &current.values;
  let auth = &app.auth;
  let ip = presented.client.ip;
  if let Some(credential) = &presented.credential {
    let counted = check == Check::Request;
    if counted && auth.limits.presentations.blocked(ip) {
      return Err(ApiError::rate_limited());
    }
    let outcome = if credential.starts_with(TOKEN_PREFIX) {
      tokens::check(&auth.db, credential, ip)
        .await
        .map_err(ApiError::internal)?
        .map(|token| token.principal())
        .map_err(|refusal| refusal.to_string())
    } else {
      keys::check(
        credential,
        ip,
        app.primary_key.as_deref(),
        &options.web.authentication.api_keys,
      )
      .map_err(|refusal| refusal.to_string())
    };
    return match outcome {
      Ok(principal) => Ok(Identified {
        principal: Some(principal),
        ..Identified::default()
      }),
      Err(reason) => {
        if counted {
          auth.limits.presentations.failed(ip);
        }
        // the credential itself is never logged, only how long it is
        debug!(%ip, length = credential.len(), reason, "API credential refused");
        Err(ApiError::unauthorized(reason).logged())
      }
    };
  }
  let scope = Scope::new(&app.url_base, presented.client.https);
  let mut identified = Identified::default();
  if let Some(value) = &presented.cookie {
    match session_of(app, value, presented).await? {
      Some(session) => {
        identified.principal = Some(session.principal());
        identified.session = Some(session);
        identified.by_cookie = true;
        return Ok(identified);
      }
      None => identified.cookies.push(scope.clear(cookies::SESSION)),
    }
  }
  let proxy = &options.web.authentication.proxy;
  let proxies = networks::ranges(&options.web.trusted_proxies);
  if let Some(forwarded) = proxy::forwarded(&presented.headers, presented.peer, proxy, &proxies) {
    match forwarded.role(proxy) {
      Some(role) => {
        let principal = Principal {
          subject: forwarded.subject(),
          name: forwarded.username.clone(),
          role,
          via: Via::Proxy,
        };
        if proxy.session && check == Check::Request {
          let session = start_session(
            app,
            presented,
            NewSession {
              subject: principal.subject.clone(),
              name: principal.name.clone(),
              email: forwarded.email.clone(),
              groups: serde_json::to_string(&forwarded.groups).ok(),
              role,
              origin: Origin::Proxy,
              ttl: chrono::Duration::from_std(options.web.authentication.session_ttl)
                .unwrap_or(chrono::Duration::MAX),
              idp_sid: None,
              id_token: None,
              ip: Some(ip.to_string()),
              user_agent: presented.user_agent(),
            },
          )
          .await?;
          identified
            .cookies
            .push(session_cookie(app, &session, &scope));
        }
        identified.principal = Some(principal);
        return Ok(identified);
      }
      None => {
        if let Some(suppressed) = PROXY_REFUSED.allow() {
          warn!(
            username = forwarded.username,
            suppressed, "forwarded identity is in no group that grants a role"
          );
        }
      }
    }
  }
  identified.principal = networks::trusted(&presented.client, options);
  Ok(identified)
}

/// The active session a cookie value names, recording its use when the record is stale.
async fn session_of(
  app: &AppState,
  value: &str,
  presented: &Presented,
) -> Result<Option<StoredSession>, ApiError> {
  let Some(id) = sessions::verified_id(&app.auth.secret, value) else {
    debug!(ip = %presented.client.ip, "session cookie with a wrong signature ignored");
    return Ok(None);
  };
  let Some(session) = sessions::find(&app.auth.db, id)
    .await
    .map_err(ApiError::internal)?
  else {
    debug!(ip = %presented.client.ip, "session cookie for no session ignored");
    return Ok(None);
  };
  let idle = idle(app);
  let now = Utc::now();
  if !session.active(now, idle) {
    debug!(
      subject = session.subject,
      "session cookie for an ended session ignored"
    );
    return Ok(None);
  }
  if now - session.last_seen_at >= sessions::SEEN_WRITES {
    let db = app.auth.db.clone();
    let id = session.id.clone();
    let ip = presented.client.ip.to_string();
    let user_agent = presented.user_agent();
    tokio::spawn(async move {
      if let Err(e) = sessions::seen(&db, &id, ip, user_agent).await {
        warn!(error = %e, "a session's last use was not recorded");
      }
    });
  }
  Ok(Some(session))
}

/// How long a session may go unused.
pub fn idle(app: &AppState) -> chrono::Duration {
  chrono::Duration::from_std(app.config.current().values.web.authentication.session_idle)
    .unwrap_or(chrono::Duration::MAX)
}

/// Makes a session for someone who just signed in and raises `SessionCreated`.
pub async fn start_session(
  app: &AppState,
  presented: &Presented,
  new: NewSession,
) -> Result<StoredSession, ApiError> {
  let session = sessions::create(&app.auth.db, new)
    .await
    .map_err(ApiError::internal)?;
  info!(
    principal = session.subject,
    via = session.origin.stored(),
    "session created"
  );
  app.events.raise(Event::SessionCreated(SessionCreated {
    version: 0,
    subject: session.subject.clone(),
    name: session.name.clone(),
    via: session.origin.via(),
    role: session.role,
    ip: Some(presented.client.ip.to_string()),
  }));
  Ok(session)
}

/// The `Set-Cookie` value that hands `session` to the browser until it expires.
pub fn session_cookie(app: &AppState, session: &StoredSession, scope: &Scope) -> HeaderValue {
  let lasts = (session.expires_at - Utc::now())
    .to_std()
    .unwrap_or_default();
  scope.set(
    cookies::SESSION,
    &sessions::cookie_value(&app.auth.secret, &session.id),
    lasts,
  )
}

/// The credential in `Authorization: Bearer` or `X-API-Key`, in that order.
fn presented_credential(headers: &HeaderMap) -> Option<&str> {
  let bearer = headers
    .get(header::AUTHORIZATION)
    .and_then(|h| h.to_str().ok())
    .and_then(|value| {
      let (scheme, credential) = value.split_once(' ')?;
      scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(credential.trim())
    });
  bearer
    .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()))
    .map(str::trim)
    .filter(|credential| !credential.is_empty())
}

#[cfg(test)]
mod tests {
  use crate::testing::{app, request};

  const CONFIG: &str = "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n\
                        [web.authentication.api_keys.viewer]\nkey = \"viewer-key-0123456789\"\n";

  fn with_header(name: &'static str, value: &str) -> axum::http::Request<axum::body::Body> {
    let mut r = request("GET", "/api/v0/options", None);
    r.headers_mut().insert(name, value.parse().unwrap());
    r
  }

  #[tokio::test]
  async fn twenty_failed_credentials_from_an_address_are_answered_429() {
    let app = app(CONFIG, &[]).await;
    for _ in 0..20 {
      let wrong = app
        .send_from(
          "192.0.2.9:1",
          with_header("x-api-key", "wrong-key-0123456789"),
        )
        .await;
      assert_eq!(wrong.status, 401);
    }
    let blocked = app
      .send_from(
        "192.0.2.9:1",
        with_header("x-api-key", "viewer-key-0123456789"),
      )
      .await;
    assert_eq!(blocked.status, 429, "even a good key waits");
    assert_eq!(blocked.headers["retry-after"], "60");
    let other = app
      .send_from(
        "192.0.2.10:1",
        with_header("x-api-key", "viewer-key-0123456789"),
      )
      .await;
    assert_eq!(other.status, 403, "another address is not held back");
  }

  #[tokio::test]
  async fn a_token_beats_a_cookie_beats_a_proxy_beats_a_trusted_network() {
    use crate::testing::{set_cookie, with};
    use serde_json::json;
    let app = app(
      "[web]\ntrusted_proxies = [\"127.0.0.1/32\"]\n[web.authentication]\ntrusted_networks = \
       [\"127.0.0.0/8\"]\n[web.authentication.proxy]\nenabled = true\n[web.authentication.proxy.\
       role_mapping]\nreadwrite = [\"users\"]\n",
      &[],
    )
    .await;
    let token = app
      .send(request(
        "POST",
        "/api/v0/tokens",
        Some(json!({ "name": "script", "role": "ReadOnly" })),
      ))
      .await
      .json()["secret"]
      .as_str()
      .unwrap()
      .to_string();
    let login = app
      .send(request(
        "POST",
        "/api/v0/session",
        Some(json!({ "username": "reseek", "password": "reseek" })),
      ))
      .await;
    let cookie = set_cookie(&login.headers, "reseek_session").unwrap();
    let proxied = |r| {
      with(
        with(r, "x-authentik-username", "alice"),
        "x-authentik-groups",
        "users",
      )
    };
    let who =
      |answer: crate::testing::Answer| answer.json()["subject"].as_str().unwrap().to_string();
    let everything = with(
      with(
        proxied(request("GET", "/api/v0/session", None)),
        "cookie",
        &cookie,
      ),
      "authorization",
      &format!("Bearer {token}"),
    );
    assert!(who(app.send(everything).await).starts_with("token:"));
    let no_token = with(
      proxied(request("GET", "/api/v0/session", None)),
      "cookie",
      &cookie,
    );
    assert_eq!(who(app.send(no_token).await), "local:reseek");
    let proxy = app
      .send(proxied(request("GET", "/api/v0/session", None)))
      .await;
    assert!(
      set_cookie(&proxy.headers, "reseek_session").is_some(),
      "a proxy identity starts a session"
    );
    assert_eq!(who(proxy), "proxy:alice");
    assert_eq!(
      who(app.send(request("GET", "/api/v0/session", None)).await),
      "network:127.0.0.0/8"
    );
    let untrusted = app
      .send_from(
        "192.0.2.1:1",
        proxied(request("GET", "/api/v0/session", None)),
      )
      .await;
    assert_eq!(untrusted.status, 401, "only a trusted proxy is believed");
  }
}
