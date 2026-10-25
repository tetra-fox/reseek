//! The session routes: signing in with a local account, who the caller is, the ways to sign
//! in, signing out, and the list of browser sessions with revoking them.

use api_types::events::{Event, LoginFailed, SessionRevokeReason, SessionRevoked};
use api_types::session::{
  CurrentPrincipal, CurrentSession, LocalProvider, LoginRequest, LogoutResponse, OidcProvider,
  Providers, ProxyProvider, Session,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use soulseek_client::LogLimiter;
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error::ApiError;
use crate::app::AppState;
use crate::auth::cookies::{self, Scope};
use crate::auth::oidc::external_base;
use crate::auth::oidc::logout::end_session_url;
use crate::auth::sessions::{self, NewSession, Origin, StoredSession};
use crate::auth::{
  Administrator, AnyRole, Authorized, Client, Presented, RequestSession, networks, session_cookie,
  start_session,
};
use crate::config::Options;

static LOGIN_FAILED: LogLimiter = LogLimiter::new();

/// The session routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(get_session, login))
    .routes(routes!(enabled))
    .routes(routes!(providers))
    .routes(routes!(logout))
    .routes(routes!(list, revoke_others))
    .routes(routes!(revoke))
}

/// Who the caller is, with the browser session it came with.
#[utoipa::path(
  get, path = "/session", tag = "session",
  responses((status = 200, body = CurrentPrincipal), (status = 401, body = api_types::ErrorBody))
)]
async fn get_session(
  caller: Authorized<AnyRole>,
  session: Option<Extension<RequestSession>>,
) -> Json<CurrentPrincipal> {
  let session = session.map(|Extension(RequestSession(session))| session);
  Json(CurrentPrincipal {
    provider: session.as_ref().map(|s| s.origin.provider()),
    session: session.map(|s| CurrentSession {
      id: s.id,
      created_at: s.created_at,
      expires_at: s.expires_at,
    }),
    principal: caller.principal,
  })
}

/// Signs in with a local account's username and password and sets the session cookie.
#[utoipa::path(
  post, path = "/session", tag = "session", request_body = LoginRequest,
  responses(
    (status = 204, description = "signed in; the session cookie is set"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody)
  )
)]
async fn login(
  State(app): State<AppState>,
  Extension(presented): Extension<Presented>,
  body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
  let missing = || ApiError::bad_request("Username and/or Password missing or invalid");
  let Json(login) = body.map_err(|_| missing())?;
  if login.username.trim().is_empty() || login.password.trim().is_empty() {
    return Err(missing());
  }
  let current = app.config.current();
  let options = &current.values;
  if options.headless {
    warn!(
      username = login.username,
      "Login rejected; web UI is DISABLED when running in headless mode"
    );
    return Err(ApiError::forbidden("the web UI is disabled in headless mode").logged());
  }
  let client = &presented.client;
  if app.auth.limits.logins.blocked(client.ip) {
    return Err(ApiError::rate_limited());
  }
  if !options.web.authentication.local.enabled {
    return Err(ApiError::forbidden("password login is off"));
  }
  if !offered_to(options, client) {
    return Err(ApiError::forbidden(
      "password login is not offered to this address",
    ));
  }
  let username = login.username.clone();
  let Some(account) = app
    .auth
    .accounts
    .check(&options.web.authentication, login.username, login.password)
    .await
  else {
    app.auth.limits.logins.failed(client.ip);
    if let Some(suppressed) = LOGIN_FAILED.allow() {
      warn!(username, ip = %client.ip, suppressed, "login failed");
    }
    app.events.raise(Event::LoginFailed(LoginFailed {
      version: 0,
      provider: "local".to_string(),
      ip: client.ip.to_string(),
      reason: "invalid username or password".to_string(),
    }));
    return Err(ApiError::unauthorized("invalid username or password").logged());
  };
  let session = start_session(
    &app,
    &presented,
    NewSession {
      subject: account.subject(),
      name: account.username.clone(),
      email: None,
      groups: None,
      role: account.role,
      origin: Origin::Local,
      ttl: sessions::lifetime(options.web.authentication.session_ttl),
      idp_sid: None,
      id_token: None,
      ip: Some(client.ip.to_string()),
      user_agent: presented.user_agent(),
    },
  )
  .await?;
  let scope = Scope::new(&app.url_base, client.https);
  Ok(
    (
      StatusCode::NO_CONTENT,
      [(header::SET_COOKIE, session_cookie(&app, &session, &scope))],
    )
      .into_response(),
  )
}

/// Whether there is authentication at all for the caller: false on a trusted network and when
/// authentication is off.
#[utoipa::path(
  get, path = "/session/enabled", tag = "session",
  responses((status = 200, body = bool))
)]
async fn enabled(State(app): State<AppState>, Extension(client): Extension<Client>) -> Json<bool> {
  let current = app.config.current();
  let options = &current.values;
  Json(!options.web.authentication.disabled && networks::trusted(&client, options).is_none())
}

/// The ways to sign in the login page offers the caller.
#[utoipa::path(
  get, path = "/session/providers", tag = "session",
  responses((status = 200, body = Providers))
)]
async fn providers(
  State(app): State<AppState>,
  Extension(client): Extension<Client>,
) -> Json<Providers> {
  let current = app.config.current();
  let options = &current.values;
  let authentication = &options.web.authentication;
  let local = options.web.authentication.local.enabled && offered_to(options, &client);
  let trusted = networks::trusted(&client, options).is_some();
  let proxy = authentication.proxy.enabled;
  let only = authentication.oidc.len() == 1 && !local && !proxy && !trusted;
  Json(Providers {
    local: LocalProvider { enabled: local },
    oidc: authentication
      .oidc
      .iter()
      .map(|(name, provider)| OidcProvider {
        name: name.clone(),
        display_name: provider
          .display_name
          .clone()
          .unwrap_or_else(|| name.clone()),
        login_url: format!("{}/api/v0/auth/oidc/{name}/login", app.url_base),
        auto_login: provider.auto_login && only,
      })
      .collect(),
    proxy: ProxyProvider { enabled: proxy },
    trusted_network: trusted,
  })
}

/// Ends the caller's browser session and clears its cookie. For a session from an OpenID
/// Connect provider that has a logout page, that page is returned for the browser to go to.
#[utoipa::path(
  post, path = "/session/logout", tag = "session",
  responses(
    (status = 200, body = LogoutResponse), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn logout(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Extension(presented): Extension<Presented>,
  session: Option<Extension<RequestSession>>,
) -> Result<Response, ApiError> {
  let scope = Scope::new(&app.url_base, presented.client.https);
  let mut redirect = None;
  if let Some(Extension(RequestSession(session))) = session {
    redirect = provider_logout(&app, &session, &presented.client).await;
    end(
      &app,
      vec![session.id.clone()],
      Some(caller.principal.subject.clone()),
      SessionRevokeReason::Logout,
    )
    .await?;
  }
  Ok(
    (
      [(header::SET_COOKIE, scope.clear(cookies::SESSION))],
      Json(LogoutResponse { redirect }),
    )
      .into_response(),
  )
}

/// The active browser sessions: the caller's own, or everyone's for an administrator.
#[utoipa::path(
  get, path = "/sessions", tag = "session",
  responses((status = 200, body = Vec<Session>), (status = 401, body = api_types::ErrorBody))
)]
async fn list(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  current: Option<Extension<RequestSession>>,
) -> Result<Json<Vec<Session>>, ApiError> {
  let current = current.map(|Extension(RequestSession(session))| session.id);
  let whose = (!is_admin(&caller)).then(|| caller.principal.subject.clone());
  let active = sessions::active(&app.auth.db, whose, crate::auth::idle(&app))
    .await
    .map_err(ApiError::internal)?;
  Ok(Json(
    active
      .iter()
      .map(|session| session.to_api(current.as_deref() == Some(session.id.as_str())))
      .collect(),
  ))
}

/// Revokes one browser session, the caller's own or, for an administrator, anyone's.
#[utoipa::path(
  delete, path = "/sessions/{id}", tag = "session",
  params(("id" = String, Path, description = "the session's id")),
  responses(
    (status = 204, description = "revoked"), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn revoke(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
  let found = sessions::find(&app.auth.db, &id)
    .await
    .map_err(ApiError::internal)?
    .filter(|session| session.active(chrono::Utc::now(), crate::auth::idle(&app)))
    .filter(|session| is_admin(&caller) || session.subject == caller.principal.subject);
  let Some(session) = found else {
    return Err(ApiError::not_found("no such session"));
  };
  end(
    &app,
    vec![session.id],
    Some(caller.principal.subject.clone()),
    SessionRevokeReason::Admin,
  )
  .await?;
  Ok(StatusCode::NO_CONTENT)
}

/// Revokes every active browser session but the caller's own.
#[utoipa::path(
  delete, path = "/sessions", tag = "session",
  responses(
    (status = 204, description = "revoked"), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn revoke_others(
  State(app): State<AppState>,
  admin: Authorized<Administrator>,
  current: Option<Extension<RequestSession>>,
) -> Result<StatusCode, ApiError> {
  let keep = current.map(|Extension(RequestSession(session))| session.id);
  let others: Vec<String> = sessions::active(&app.auth.db, None, crate::auth::idle(&app))
    .await
    .map_err(ApiError::internal)?
    .into_iter()
    .map(|session| session.id)
    .filter(|id| Some(id) != keep.as_ref())
    .collect();
  let count = others.len();
  end(
    &app,
    others,
    Some(admin.principal.subject.clone()),
    SessionRevokeReason::Admin,
  )
  .await?;
  info!(
    by = admin.principal.subject,
    count, "every other session revoked"
  );
  Ok(StatusCode::NO_CONTENT)
}

/// Revokes the sessions `ids`, raising `SessionRevoked` for each one that was still going.
pub async fn end(
  app: &AppState,
  ids: Vec<String>,
  by: Option<String>,
  reason: SessionRevokeReason,
) -> Result<(), ApiError> {
  let ended = sessions::revoke(&app.auth.db, ids)
    .await
    .map_err(ApiError::internal)?;
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
}

/// The OpenID Connect provider's logout page for `session`, when it came from a provider that
/// offers one and logging out there is on.
async fn provider_logout(
  app: &AppState,
  session: &StoredSession,
  client: &Client,
) -> Option<String> {
  let Origin::Oidc(name) = &session.origin else {
    return None;
  };
  let current = app.config.current();
  let provider = match app.auth.oidc.get(name, &current.values) {
    Ok(Some(provider)) if provider.options.logout.rp_initiated => provider,
    Ok(_) => return None,
    Err(e) => {
      warn!(
        provider = name,
        error = e,
        "provider logout page not offered"
      );
      return None;
    }
  };
  let id_token = session
    .id_token
    .as_deref()
    .and_then(|sealed| app.auth.secret.open(sealed))
    .and_then(|bytes| String::from_utf8(bytes).ok())?;
  let metadata = match provider.metadata().await {
    Ok(metadata) => metadata,
    Err(e) => {
      warn!(
        provider = name,
        error = e,
        "provider logout page not offered"
      );
      return None;
    }
  };
  let back = format!(
    "{}/",
    external_base(&app.url_base, &current.values, client)?
  );
  end_session_url(&metadata, &id_token, &back)
}

/// Whether `local.only_from` lets `client`'s address use password login.
fn offered_to(options: &Options, client: &Client) -> bool {
  let only_from = &options.web.authentication.local.only_from;
  only_from.is_empty()
    || networks::ranges(only_from)
      .iter()
      .any(|range| range.contains(client.ip))
}

fn is_admin(caller: &Authorized<AnyRole>) -> bool {
  caller.principal.role == api_types::auth::Role::Administrator
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use api_types::events::{Event, EventType};
  use serde_json::json;

  use crate::testing::{app, request, set_cookie, with};

  const OUTSIDE: &str = "192.0.2.1:1";

  fn credentials(password: &str) -> serde_json::Value {
    json!({ "username": "reseek", "password": password })
  }

  /// Signs in as the built-in account and returns the `reseek_session=...` cookie.
  async fn signed_in(app: &crate::testing::TestApp) -> String {
    let answer = app
      .send_from(
        OUTSIDE,
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(answer.status, 204, "{:?}", answer.body);
    set_cookie(&answer.headers, "reseek_session").unwrap()
  }

  /// Every event raised on `app`'s bus from now on.
  fn recorded(app: &crate::testing::TestApp) -> Arc<Mutex<Vec<Event>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    app.state.events.subscribe("test", None, move |record| {
      kept.lock().unwrap().push(record.event.clone());
      async { Ok(()) }
    });
    seen
  }

  async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
  }

  #[tokio::test]
  async fn a_local_login_gives_a_session_cookie_that_identifies_later_requests() {
    let app = app("", &[]).await;
    let events = recorded(&app);
    let answer = app
      .send_from(
        OUTSIDE,
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(answer.status, 204);
    let header = answer.headers["set-cookie"].to_str().unwrap();
    assert!(
      header.contains("HttpOnly") && header.contains("SameSite=Lax") && header.contains("Path=/")
    );
    assert!(!header.contains("Secure"), "plain HTTP");
    let cookie = set_cookie(&answer.headers, "reseek_session").unwrap();
    let me = app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", &cookie),
      )
      .await;
    assert_eq!(me.status, 200);
    let me = me.json();
    assert_eq!(me["subject"], "local:reseek");
    assert_eq!(me["role"], "Administrator");
    assert_eq!(me["via"], json!({ "kind": "local" }));
    assert_eq!(me["provider"], "local");
    assert!(me["session"]["expiresAt"].is_string());
    settle().await;
    let events = events.lock().unwrap();
    assert!(
      matches!(&events[..], [Event::SessionCreated(created)] if created.subject == "local:reseek")
    );
  }

  #[tokio::test]
  async fn a_forged_or_ended_cookie_is_cleared_and_ignored() {
    let app = app("", &[]).await;
    let cookie = signed_in(&app).await;
    let (id, _) = cookie.split_once('.').unwrap();
    let forged = format!("{id}.AAAA");
    let answer = app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", &forged),
      )
      .await;
    assert_eq!(answer.status, 401);
    assert_eq!(
      set_cookie(&answer.headers, "reseek_session").as_deref(),
      Some("reseek_session="),
      "the bad cookie is cleared"
    );
    let unsigned = id.to_string();
    let answer = app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", &unsigned),
      )
      .await;
    assert_eq!(answer.status, 401);
  }

  #[tokio::test]
  async fn five_failed_logins_from_an_address_are_audited_then_answered_429() {
    let app = app("", &[]).await;
    let events = recorded(&app);
    for _ in 0..5 {
      let answer = app
        .send_from(
          OUTSIDE,
          request("POST", "/api/v0/session", Some(credentials("wrong"))),
        )
        .await;
      assert_eq!(answer.status, 401);
    }
    let blocked = app
      .send_from(
        OUTSIDE,
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(blocked.status, 429);
    assert_eq!(blocked.headers["retry-after"], "60");
    let elsewhere = app
      .send_from(
        "192.0.2.2:1",
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(elsewhere.status, 204);
    settle().await;
    let failed = events
      .lock()
      .unwrap()
      .iter()
      .filter(|e| e.event_type() == EventType::LoginFailed)
      .count();
    assert_eq!(failed, 5, "the refused attempt raises nothing");
    let missing = app
      .send_from(
        "192.0.2.3:1",
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": " ", "password": "x" })),
        ),
      )
      .await;
    assert_eq!(missing.status, 400);
  }

  #[tokio::test]
  async fn cookie_changes_need_a_page_of_this_site() {
    let app = app("", &[]).await;
    let cookie = signed_in(&app).await;
    let logout = || {
      with(
        request("POST", "/api/v0/session/logout", None),
        "cookie",
        &cookie,
      )
    };
    let bare = app.send_from(OUTSIDE, logout()).await;
    assert_eq!(bare.status, 403);
    assert_eq!(bare.json()["error"], "cross-site request refused");
    let elsewhere = app
      .send_from(OUTSIDE, with(logout(), "origin", "http://evil.example"))
      .await;
    assert_eq!(elsewhere.status, 403);
    let own = app
      .send_from(OUTSIDE, with(logout(), "origin", "http://localhost"))
      .await;
    assert_eq!(own.status, 200);
    assert_eq!(own.json(), json!({ "redirect": null }));
    let after = app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", &cookie),
      )
      .await;
    assert_eq!(after.status, 401, "the session is revoked");
  }

  #[tokio::test]
  async fn sessions_are_listed_and_revoked_by_owner_or_administrator() {
    let app = app(
      "[web.authentication.local.users.viewer]\npassword = \"look\"\n",
      &[],
    )
    .await;
    let first = signed_in(&app).await;
    let second = signed_in(&app).await;
    let viewer = app
      .send_from(
        OUTSIDE,
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": "viewer", "password": "look" })),
        ),
      )
      .await;
    let viewer = set_cookie(&viewer.headers, "reseek_session").unwrap();
    let as_cookie = |r, cookie: &str| with(with(r, "cookie", cookie), "origin", "http://localhost");
    let listed = app
      .send_from(
        OUTSIDE,
        as_cookie(request("GET", "/api/v0/sessions", None), &first),
      )
      .await
      .json();
    assert_eq!(
      listed.as_array().unwrap().len(),
      3,
      "an administrator sees all"
    );
    let mine: Vec<&serde_json::Value> = listed
      .as_array()
      .unwrap()
      .iter()
      .filter(|s| s["current"] == true)
      .collect();
    assert_eq!(mine.len(), 1);
    let theirs = app
      .send_from(
        OUTSIDE,
        as_cookie(request("GET", "/api/v0/sessions", None), &viewer),
      )
      .await
      .json();
    assert_eq!(theirs.as_array().unwrap().len(), 1, "others see their own");
    let second_id = second.split_once('=').unwrap().1.split_once('.').unwrap().0;
    let refused = app
      .send_from(
        OUTSIDE,
        as_cookie(
          request("DELETE", &format!("/api/v0/sessions/{second_id}"), None),
          &viewer,
        ),
      )
      .await;
    assert_eq!(refused.status, 404);
    let all = app
      .send_from(
        OUTSIDE,
        as_cookie(request("DELETE", "/api/v0/sessions", None), &first),
      )
      .await;
    assert_eq!(all.status, 204);
    for (cookie, alive) in [(&first, 200), (&second, 401), (&viewer, 401)] {
      let answer = app
        .send_from(
          OUTSIDE,
          with(request("GET", "/api/v0/session", None), "cookie", cookie),
        )
        .await;
      assert_eq!(answer.status, alive);
    }
  }

  #[tokio::test]
  async fn providers_and_enabled_follow_the_callers_network() {
    let app = app(
      "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n[web.authentication.local]\n\
       only_from = [\"192.0.2.0/24\"]\n[web.authentication.oidc.home]\nissuer = \
       \"https://auth.example.net/\"\nclient_id = \"reseek\"\ndisplay_name = \"Home SSO\"\n",
      &[],
    )
    .await;
    let trusted = app
      .send(request("GET", "/api/v0/session/providers", None))
      .await
      .json();
    assert_eq!(trusted["trustedNetwork"], true);
    assert_eq!(trusted["local"]["enabled"], false, "only from 192.0.2.0/24");
    assert_eq!(trusted["oidc"][0]["displayName"], "Home SSO");
    assert_eq!(
      trusted["oidc"][0]["loginUrl"],
      "/api/v0/auth/oidc/home/login"
    );
    let lan = app
      .send_from(OUTSIDE, request("GET", "/api/v0/session/providers", None))
      .await
      .json();
    assert_eq!(lan["trustedNetwork"], false);
    assert_eq!(lan["local"]["enabled"], true);
    let far = app
      .send_from(
        "198.51.100.1:1",
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(far.status, 403);
    assert_eq!(
      far.json()["error"],
      "password login is not offered to this address"
    );
    assert_eq!(
      app
        .send(request("GET", "/api/v0/session/enabled", None))
        .await
        .json(),
      json!(false)
    );
    assert_eq!(
      app
        .send_from(OUTSIDE, request("GET", "/api/v0/session/enabled", None))
        .await
        .json(),
      json!(true)
    );
  }

  #[tokio::test]
  async fn turning_password_login_off_says_so() {
    let app = app("[web.authentication.local]\nenabled = false\n", &[]).await;
    let off = app
      .send_from(
        OUTSIDE,
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(off.status, 403);
    assert_eq!(off.json()["error"], "password login is off");
  }

  #[tokio::test]
  async fn headless_refuses_logins_after_reading_the_body() {
    let app = app("headless = true\n", &[]).await;
    let empty = app
      .send_from(OUTSIDE, request("POST", "/api/v0/session", None))
      .await;
    assert_eq!(empty.status, 400);
    let refused = app
      .send_from(
        OUTSIDE,
        request("POST", "/api/v0/session", Some(credentials("reseek"))),
      )
      .await;
    assert_eq!(refused.status, 403);
  }
}
