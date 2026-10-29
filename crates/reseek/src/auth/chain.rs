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
/// accepted, which counts no failures.
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
  if let Some((principal, _)) = forwarded(presented, options) {
    identified.principal = Some(principal);
    return Ok(identified);
  }
  identified.principal = networks::trusted(&presented.client, options);
  Ok(identified)
}

/// The proxy step: the identity a trusted reverse proxy forwarded, when its groups grant a
/// role, with what the proxy said about the user.
fn forwarded(
  presented: &Presented,
  options: &crate::config::Options,
) -> Option<(Principal, proxy::Forwarded)> {
  let proxy = &options.web.authentication.proxy;
  let proxies = networks::ranges(&options.web.trusted_proxies);
  let forwarded = proxy::forwarded(&presented.headers, presented.peer, proxy, &proxies)?;
  let Some(role) = forwarded.role(proxy) else {
    if let Some(suppressed) = PROXY_REFUSED.allow() {
      warn!(
        username = forwarded.username,
        suppressed, "forwarded identity is in no group that grants a role"
      );
    }
    return None;
  };
  let principal = Principal {
    subject: forwarded.subject(),
    name: forwarded.username.clone(),
    role,
    via: Via::Proxy,
  };
  Some((principal, forwarded))
}

/// The `Set-Cookie` values for the web UI's page: the cookie and proxy steps of the chain.
/// With `proxy.session` on, a page load that a trusted proxy names a user for and that has no
/// valid session cookie starts a session, so the page's later requests work without the
/// proxy's headers; an invalid cookie is cleared. API requests never start one.
pub async fn page_session(
  app: &AppState,
  presented: &Presented,
) -> Result<Vec<HeaderValue>, ApiError> {
  let current = app.config.current();
  let options = &current.values;
  let scope = Scope::new(&app.url_base, presented.client.https);
  let mut cookies = Vec::new();
  if let Some(value) = &presented.cookie {
    match session_of(app, value, presented).await? {
      Some(_) => return Ok(cookies),
      None => cookies.push(scope.clear(cookies::SESSION)),
    }
  }
  if !options.web.authentication.proxy.session {
    return Ok(cookies);
  }
  let Some((principal, forwarded)) = forwarded(presented, options) else {
    return Ok(cookies);
  };
  let session = start_session(
    app,
    presented,
    NewSession {
      subject: principal.subject,
      name: principal.name,
      email: forwarded.email,
      groups: serde_json::to_string(&forwarded.groups).ok(),
      role: principal.role,
      origin: Origin::Proxy,
      ttl: sessions::lifetime(options.web.authentication.session_ttl),
      idp_sid: None,
      id_token: None,
      ip: Some(presented.client.ip.to_string()),
      user_agent: presented.user_agent(),
    },
  )
  .await?;
  cookies.push(session_cookie(app, &session, &scope));
  Ok(cookies)
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
  sessions::lifetime(app.config.current().values.web.authentication.session_idle)
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
  use crate::auth::tests::{CONFIG, with_header};
  use crate::testing::{OUTSIDE, app, request};

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
    let api_call = app
      .send(with(
        proxied(request("GET", "/api/v0/session", None)),
        "accept",
        "text/html",
      ))
      .await;
    assert!(
      set_cookie(&api_call.headers, "reseek_session").is_none(),
      "an API request acts on the headers alone"
    );
    assert_eq!(who(api_call), "proxy:alice");
    assert_eq!(
      who(app.send(request("GET", "/api/v0/session", None)).await),
      "network:127.0.0.0/8"
    );
    let untrusted = app
      .send_from(OUTSIDE, proxied(request("GET", "/api/v0/session", None)))
      .await;
    assert_eq!(untrusted.status, 401, "only a trusted proxy is believed");
  }

  #[tokio::test]
  async fn the_ui_page_starts_a_proxy_users_session() {
    use crate::testing::{set_cookie, with};
    let app = app(
      "[web]\ntrusted_proxies = [\"127.0.0.1/32\"]\n[web.authentication.proxy]\nenabled = \
       true\n[web.authentication.proxy.role_mapping]\nreadwrite = [\"users\"]\n",
      &[],
    )
    .await
    .with_page();
    let created = std::sync::Arc::new(std::sync::Mutex::new(0));
    let counted = created.clone();
    app.state.events.subscribe(
      "test",
      Some(api_types::events::EventType::SessionCreated),
      move |_| {
        *counted.lock().unwrap() += 1;
        async { Ok(()) }
      },
    );
    let rows = || async {
      app
        .state
        .auth
        .db
        .read(|conn| {
          conn.query_row("SELECT count(*) FROM sessions", [], |row| {
            row.get::<_, i64>(0)
          })
        })
        .await
        .unwrap()
    };
    let proxied = |r| with(r, "x-authentik-username", "alice");
    let groups = |r| with(r, "x-authentik-groups", "users");

    let api = app
      .send(groups(proxied(request("GET", "/api/v0/session", None))))
      .await;
    assert_eq!(api.status, 200);
    assert!(set_cookie(&api.headers, "reseek_session").is_none());
    let bare = app.send(request("GET", "/", None)).await;
    assert_eq!(bare.status, 200);
    assert!(
      set_cookie(&bare.headers, "reseek_session").is_none(),
      "no forwarded user"
    );
    let stranger = app
      .send_from(OUTSIDE, groups(proxied(request("GET", "/", None))))
      .await;
    assert!(
      set_cookie(&stranger.headers, "reseek_session").is_none(),
      "not a trusted proxy"
    );
    assert_eq!(rows().await, 0);

    let page = app.send(groups(proxied(request("GET", "/", None)))).await;
    assert_eq!(page.status, 200);
    assert_eq!(page.body, "<html>app</html>");
    let cookie = set_cookie(&page.headers, "reseek_session").expect("the page starts a session");
    assert_eq!(rows().await, 1);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(*created.lock().unwrap(), 1, "SessionCreated is raised");

    let me = app
      .send(with(
        request("GET", "/api/v0/session", None),
        "cookie",
        &cookie,
      ))
      .await;
    assert_eq!(me.status, 200);
    assert_eq!(me.json()["subject"], "proxy:alice");
    let mut stream = app
      .stream_from(
        "127.0.0.1:1",
        with(request("GET", "/api/v0/live", None), "cookie", &cookie),
      )
      .await;
    assert_eq!(stream.status, 200, "the stream works on the cookie alone");
    assert_eq!(stream.next().await.unwrap().event, "hello");

    let again = app
      .send(with(
        groups(proxied(request("GET", "/", None))),
        "cookie",
        &cookie,
      ))
      .await;
    assert!(
      set_cookie(&again.headers, "reseek_session").is_none(),
      "the cookie is kept"
    );
    assert_eq!(rows().await, 1);
  }
}
