//! The chain that settles who a request is, in order: an API token, a configuration API key,
//! the session cookie, a trusted reverse proxy's forwarded identity, a trusted network. A
//! token or key that is presented but not valid fails at once, as does a token whose owner the
//! configuration in effect gives no role; a session cookie that is not valid, or whose user the
//! configuration gives no role, is cleared and the next ways are tried. A session the proxy step
//! started ends when the proxy forwards someone else or a different role for them.

use std::net::IpAddr;

use api_types::auth::{Principal, Role, Via};
use api_types::events::{Event, SessionCreated, SessionRevokeReason, SessionRevoked};
use axum::http::{HeaderMap, HeaderValue, Uri, header};
use chrono::Utc;
use soulseek_client::LogLimiter;
use tracing::{Instrument, debug, info, warn};

use super::cookies::{self, Scope};
use super::sessions::{self, NewSession, Origin, StoredSession};
use super::{Client, TOKEN_PREFIX, client, keys, networks, proxy, tokens};
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::db::DbError;
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
  /// the request opens the event stream, which a browser does again by itself after a drop, so
  /// it is no use of a session
  opens_stream: bool,
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
    let opens_stream = uri.path() == LIVE_PATH;
    let in_query = || {
      opens_stream
        .then(|| uri.query())
        .flatten()
        .and_then(|query| {
          url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "access_token")
            .map(|(_, value)| value.into_owned())
        })
    };
    Self {
      peer: peer.ip(),
      credential: presented_credential(headers)
        .map(str::to_string)
        .or_else(in_query)
        .filter(|credential| !credential.is_empty()),
      cookie: cookies::find(headers, cookies::SESSION).map(str::to_string),
      headers: headers.clone(),
      opens_stream,
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
/// accepted, which counts no failures and is no use of a session.
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
    // a failure unless forgiven, so credentials presented at the same time are limited too; a
    // credential checked again counts nothing
    let attempt = match check {
      Check::Request => Some(
        auth
          .limits
          .presentations
          .attempt(ip)
          .await
          .ok_or_else(ApiError::rate_limited)?,
      ),
      Check::Again => None,
    };
    let forgive = || {
      if let Some(attempt) = attempt {
        attempt.forgive();
      }
    };
    let outcome = if credential.starts_with(TOKEN_PREFIX) {
      match tokens::check(&auth.db, credential, ip).await {
        Ok(Ok(token)) => owner_role(app, &token.owner, options)
          .await
          .map(|owner| match owner {
            Some(role) => Ok(token.principal(role)),
            None => Err(tokens::Refusal::Owner.to_string()),
          }),
        Ok(Err(refusal)) => Ok(Err(refusal.to_string())),
        Err(e) => Err(e),
      }
    } else {
      Ok(
        keys::check(
          credential,
          ip,
          app.primary_key.as_deref(),
          &options.web.authentication.api_keys,
        )
        .map_err(|refusal| refusal.to_string()),
      )
    };
    return match outcome {
      Ok(Ok(principal)) => {
        forgive();
        Ok(Identified {
          principal: Some(principal),
          ..Identified::default()
        })
      }
      Ok(Err(reason)) => {
        // the credential itself is never logged, only how long it is
        debug!(%ip, length = credential.len(), reason, "API credential refused");
        Err(ApiError::unauthorized(reason).logged())
      }
      // a check that could not run is not a refused credential
      Err(e) => {
        forgive();
        Err(ApiError::internal(e))
      }
    };
  }
  let scope = Scope::new(&app.url_base, presented.client.https);
  let mut identified = Identified::default();
  if let Some(value) = &presented.cookie {
    let used = check == Check::Request && !presented.opens_stream;
    match cookie_session(app, value, presented, options, used).await? {
      Some((session, role)) => {
        identified.principal = Some(session.principal(role));
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
  identified.principal = networks::trusted(
    &presented.client,
    options,
    app.startup.values.web.authentication.disabled,
  );
  Ok(identified)
}

/// The role of the token owner `owner` now: what `options` give them by their last sign-in, or
/// give the trusted network they are; `None` for anyone else, whose tokens are refused.
pub async fn owner_role(
  app: &AppState,
  owner: &str,
  options: &crate::config::Options,
) -> Result<Option<Role>, DbError> {
  let role = match sessions::last_sign_in(&app.auth.db, owner).await? {
    Some(sign_in) => sign_in.role(owner, &options.web.authentication),
    None => networks::subject_role(
      owner,
      options,
      app.startup.values.web.authentication.disabled,
    ),
  };
  Ok(role)
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
/// proxy's headers; an invalid cookie is cleared, as is one whose proxy session the proxy now
/// forwards someone else or a different role for. API requests never start one.
pub async fn page_session(
  app: &AppState,
  presented: &Presented,
) -> Result<Vec<HeaderValue>, ApiError> {
  let current = app.config.current();
  let options = &current.values;
  let scope = Scope::new(&app.url_base, presented.client.https);
  let mut cookies = Vec::new();
  if let Some(value) = &presented.cookie {
    match cookie_session(app, value, presented, options, true).await? {
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
      groups: Some(forwarded.groups),
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

/// The session a cookie value names with the role `options` give its user, unless the proxy
/// step started it and the proxy now forwards someone else or a different role; that session is
/// revoked. `used` records the request as a use of the session, which keeps it from going idle.
async fn cookie_session(
  app: &AppState,
  value: &str,
  presented: &Presented,
  options: &crate::config::Options,
  used: bool,
) -> Result<Option<(StoredSession, Role)>, ApiError> {
  let Some((session, role)) = session_of(app, value, presented, options, used).await? else {
    return Ok(None);
  };
  if session.sign_in.origin != Origin::Proxy {
    return Ok(Some((session, role)));
  }
  let proxy = &options.web.authentication.proxy;
  let proxies = networks::ranges(&options.web.trusted_proxies);
  let Some(named) = proxy::forwarded(&presented.headers, presented.peer, proxy, &proxies) else {
    // a request with the cookie alone, which is what the session is for
    return Ok(Some((session, role)));
  };
  if named.subject() == session.subject && named.role(proxy) == Some(role) {
    return Ok(Some((session, role)));
  }
  debug!(
    subject = session.subject,
    forwarded = named.subject(),
    "the proxy forwards another user or role than its session's"
  );
  end_sessions(app, vec![session.id], None, SessionRevokeReason::Proxy).await?;
  Ok(None)
}

/// The active session a cookie value names with the role `options` give its user, recording its
/// use when `used` and the record is stale. A session whose user they give no role is refused
/// but not ended here: the options may be a file read half written, so only a configuration
/// that stands ends it, through [`end_sessions_without_role`].
async fn session_of(
  app: &AppState,
  value: &str,
  presented: &Presented,
  options: &crate::config::Options,
  used: bool,
) -> Result<Option<(StoredSession, Role)>, ApiError> {
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
  let Some(role) = session.role(&options.web.authentication) else {
    debug!(
      subject = session.subject,
      "session cookie of a user the configuration gives no role refused"
    );
    return Ok(None);
  };
  if used && session.seen_due(now, idle) {
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
  Ok(Some((session, role)))
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
  let role = new.role;
  let session = sessions::create(&app.auth.db, new)
    .await
    .map_err(ApiError::internal)?;
  info!(
    principal = session.subject,
    via = session.sign_in.origin.stored(),
    "session created"
  );
  app.events.raise(Event::SessionCreated(SessionCreated {
    version: 0,
    subject: session.subject.clone(),
    name: session.name.clone(),
    via: session.sign_in.origin.via(),
    role,
    ip: Some(presented.client.ip.to_string()),
  }));
  Ok(session)
}

/// Ends the active sessions whose users the configuration in effect gives no role.
pub async fn end_sessions_without_role(app: &AppState) -> Result<(), ApiError> {
  let current = app.config.current();
  let authentication = &current.values.web.authentication;
  let ids = sessions::active(&app.auth.db, None, idle(app))
    .await
    .map_err(ApiError::internal)?
    .into_iter()
    .filter(|session| session.role(authentication).is_none())
    .map(|session| session.id)
    .collect();
  end_sessions(app, ids, None, SessionRevokeReason::Configuration).await
}

/// Revokes the sessions `ids`, raising `SessionRevoked` for each one that was still going and
/// closing the event streams they opened. It runs on a task of its own, so a caller that goes
/// away once the revoke is committed cannot leave the streams open and the events unraised.
pub async fn end_sessions(
  app: &AppState,
  ids: Vec<String>,
  by: Option<String>,
  reason: SessionRevokeReason,
) -> Result<(), ApiError> {
  let app = app.clone();
  let ending = async move {
    let ended = sessions::revoke(&app.auth.db, ids)
      .await
      .map_err(ApiError::internal)?;
    if !ended.is_empty() {
      app.live.credentials_changed();
    }
    for session in ended {
      info!(subject = session.subject, by, ?reason, "session revoked");
      app.events.raise(Event::SessionRevoked(SessionRevoked {
        version: 0,
        session_id: session.id,
        subject: session.subject,
        by: by.clone(),
        reason,
      }));
    }
    Ok(())
  };
  tokio::spawn(ending.in_current_span())
    .await
    .map_err(ApiError::internal)?
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
  async fn a_revoke_whose_request_goes_away_still_tells_of_it() {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use api_types::events::{Event, SessionRevokeReason};
    use serde_json::json;

    let app = app("", &[]).await;
    let login = json!({ "username": "reseek", "password": "reseek" });
    let signed = app
      .send_from(OUTSIDE, request("POST", "/api/v0/session", Some(login)))
      .await;
    let cookie = crate::testing::set_cookie(&signed.headers, "reseek_session").unwrap();
    let (id, _) = cookie
      .trim_start_matches("reseek_session=")
      .split_once('.')
      .unwrap();
    let revoked = Arc::new(Mutex::new(Vec::new()));
    let kept = revoked.clone();
    app.state.events.subscribe("test", None, move |record| {
      if let Event::SessionRevoked(event) = &record.event {
        kept.lock().unwrap().push(event.session_id.clone());
      }
      async { Ok(()) }
    });
    let mut ending = Box::pin(super::end_sessions(
      &app.state,
      vec![id.to_string()],
      None,
      SessionRevokeReason::Admin,
    ));
    let polled = futures_util::FutureExt::now_or_never(&mut ending);
    assert!(polled.is_none(), "the revoke is under way: {polled:?}");
    drop(ending);
    for _ in 0..500 {
      if !revoked.lock().unwrap().is_empty() {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(*revoked.lock().unwrap(), [id.to_string()]);
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
  async fn bad_tokens_presented_at_once_are_held_to_the_limit() {
    let app = app(CONFIG, &[]).await;
    // well formed, so the lookup waits on the database and all of them start before any is refused
    let token = format!("Bearer slsk_{}", "a".repeat(40));
    let wrong = || app.send_from("192.0.2.9:1", with_header("authorization", &token));
    let answers = futures_util::future::join_all((0..30).map(|_| wrong())).await;
    let refused = answers.iter().filter(|a| a.status == 401).count();
    let limited = answers.iter().filter(|a| a.status == 429).count();
    assert_eq!((refused, limited), (20, 10));
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

  #[tokio::test]
  async fn a_proxy_session_ends_when_the_proxy_forwards_another_user_or_role() {
    use crate::testing::{set_cookie, with};
    let app = app(
      "[web]\ntrusted_proxies = [\"127.0.0.1/32\"]\n[web.authentication.proxy]\nenabled = \
       true\n[web.authentication.proxy.role_mapping]\nreadwrite = [\"users\"]\nreadonly = \
       [\"guests\"]\n",
      &[],
    )
    .await
    .with_page();
    let proxied = |r, user: &str, groups: &str| {
      with(
        with(r, "x-authentik-username", user),
        "x-authentik-groups",
        groups,
      )
    };
    let started = async || {
      let page = app
        .send(proxied(request("GET", "/", None), "alice", "users"))
        .await;
      set_cookie(&page.headers, "reseek_session").expect("the page starts a session")
    };
    let me = |cookie: &str, forwarded: Option<(&str, &str)>| {
      let r = with(request("GET", "/api/v0/session", None), "cookie", cookie);
      match forwarded {
        Some((user, groups)) => proxied(r, user, groups),
        None => r,
      }
    };

    let cookie = started().await;
    let same = app.send(me(&cookie, Some(("alice", "users")))).await;
    assert_eq!(same.json()["role"], "ReadWrite");
    assert!(set_cookie(&same.headers, "reseek_session").is_none());
    let alone = app.send(me(&cookie, None)).await;
    assert_eq!(
      alone.json()["subject"],
      "proxy:alice",
      "the cookie alone is enough"
    );

    let other = app.send(me(&cookie, Some(("bob", "users")))).await;
    assert_eq!(other.json()["subject"], "proxy:bob");
    assert_eq!(
      set_cookie(&other.headers, "reseek_session").as_deref(),
      Some("reseek_session="),
      "the old session's cookie is cleared"
    );
    assert_eq!(
      app.send(me(&cookie, None)).await.status,
      401,
      "and it is revoked"
    );

    let cookie = started().await;
    let demoted = app.send(me(&cookie, Some(("alice", "guests")))).await;
    assert_eq!(demoted.json()["role"], "ReadOnly");
    assert_eq!(app.send(me(&cookie, None)).await.status, 401);

    let cookie = started().await;
    let refused = app.send(me(&cookie, Some(("alice", "nobody")))).await;
    assert_eq!(
      refused.status, 401,
      "a user in no group with a role keeps no session"
    );
    assert_eq!(app.send(me(&cookie, None)).await.status, 401);
  }

  #[tokio::test]
  async fn a_proxy_session_takes_its_role_from_the_mapping_in_effect() {
    use crate::testing::{set_cookie, with};
    let config = |mapping: &str| {
      format!(
        "[web]\ntrusted_proxies = [\"127.0.0.1/32\"]\n[web.authentication.proxy]\nenabled = \
         true\n[web.authentication.proxy.role_mapping]\n{mapping}"
      )
    };
    let app = app(&config("readwrite = [\"users\"]\n"), &[])
      .await
      .with_page();
    let page = app
      .send(with(
        with(request("GET", "/", None), "x-authentik-username", "alice"),
        "x-authentik-groups",
        "users",
      ))
      .await;
    let cookie = set_cookie(&page.headers, "reseek_session").unwrap();
    let me = || {
      app.send(with(
        request("GET", "/api/v0/session", None),
        "cookie",
        &cookie,
      ))
    };
    assert_eq!(me().await.json()["role"], "ReadWrite");
    let reload = async |text: String| {
      std::fs::write(app.dir.path().join("config.toml"), text).unwrap();
      app.state.config.reload().await.unwrap();
    };
    reload(config("readonly = [\"users\"]\n")).await;
    assert_eq!(me().await.json()["role"], "ReadOnly");
    reload(config("default = \"deny\"\n")).await;
    assert_eq!(me().await.status, 401);
  }

  #[tokio::test]
  async fn authentication_turned_off_in_the_file_stays_on_until_the_restart_it_takes() {
    let app = app("", &[]).await;
    let asked = || app.send_from(OUTSIDE, request("GET", "/api/v0/application", None));
    assert_eq!(asked().await.status, 401);
    app
      .configure_for_restart(
        "[web.authentication]\ndisabled = true\n",
        "web.authentication.disabled",
      )
      .await;
    assert_eq!(asked().await.status, 401);
  }
}
