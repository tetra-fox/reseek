//! The session routes: signing in with a local account, who the caller is, the ways to sign
//! in, signing out, and the list of browser sessions with revoking them.

use api_types::auth::session::{
  CurrentPrincipal, CurrentSession, LocalProvider, LoginRequest, LogoutResponse, OidcProvider,
  Providers, ProxyProvider, Session,
};
use api_types::events::{Event, LoginFailed, SessionRevokeReason};
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use soulseek_client::LogLimiter;
use tracing::{info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::api::extract::Json;
use crate::app::AppState;
use crate::auth::cookies::{self, Scope};
use crate::auth::local::Busy;
use crate::auth::oidc::external_base;
use crate::auth::oidc::logout::end_session_url;
use crate::auth::sessions::{self, NewSession, Origin, StoredSession};
use crate::auth::{
  Administrator, AnyRole, Authentication, Authorized, Client, Presented, RequestSession,
  end_sessions, networks, session_cookie, start_session,
};
use crate::config::Options;

static LOGIN_FAILED: LogLimiter = LogLimiter::new();
static HEADLESS_LOGIN: LogLimiter = LogLimiter::new();
static LOGIN_BUSY: LogLimiter = LogLimiter::new();

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
    provider: session.as_ref().map(|s| s.sign_in.origin.provider()),
    session: session.map(|s| CurrentSession {
      id: s.id,
      created_at: s.created_at,
      expires_at: s.expires_at,
    }),
    principal: caller.principal,
  })
}

/// Signs in with a local account's username and password and sets the session cookie. A session
/// the browser signed in with before is ended, as its cookie is replaced.
#[utoipa::path(
  operation_id = "session_login",
  post, path = "/session", tag = "session", request_body = LoginRequest,
  responses(
    (status = 204, description = "signed in; the session cookie is set"),
    (status = 400, body = api_types::ErrorBody), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn login(
  State(app): State<AppState>,
  Extension(presented): Extension<Presented>,
  replaced: Option<Extension<RequestSession>>,
  Json(login): Json<LoginRequest>,
) -> Result<Response, ApiError> {
  if login.username.trim().is_empty() || login.password.trim().is_empty() {
    return Err(ApiError::bad_request("Username or password is missing"));
  }
  // headless takes effect at a restart, like the UI it turns off
  if app.startup.values.headless {
    if let Some(suppressed) = HEADLESS_LOGIN.allow() {
      warn!(
        username = login.username,
        suppressed, "Login rejected; web UI is DISABLED when running in headless mode"
      );
    }
    return Err(ApiError::forbidden("The web UI is disabled in headless mode").logged());
  }
  let mut current = app.config.current();
  let client = &presented.client;
  password_login_offered(&current.values, client)?;
  // a failure unless forgiven, so attempts made at the same time are limited too
  let Some(attempt) = app.auth.limits.logins.attempt(client.ip).await else {
    return Err(ApiError::rate_limited());
  };
  let username = login.username.clone();
  // the session is made from the options the password was checked against, which must still be
  // in effect after the check, so after a reload during it the login is checked again
  let checked = loop {
    let checked = app
      .auth
      .accounts
      .check(
        &current.values.web.authentication,
        login.username.clone(),
        login.password.clone(),
      )
      .await;
    let now = app.config.current();
    if now.revision == current.revision {
      break checked;
    }
    current = now;
    if let Err(e) = password_login_offered(&current.values, client) {
      attempt.forgive();
      return Err(e);
    }
  };
  let options = &current.values;
  let account = match checked {
    Ok(Some(account)) => {
      attempt.forgive();
      account
    }
    Ok(None) => return Err(failed_login(&app, &username, client)),
    Err(Busy) => {
      // a check that never ran is not a failed one
      attempt.forgive();
      if let Some(suppressed) = LOGIN_BUSY.allow() {
        warn!(
          suppressed,
          "password login refused, every password check stayed busy"
        );
      }
      return Err(ApiError::busy("Too many sign-ins at once").logged());
    }
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
  if let Some(Extension(RequestSession(replaced))) = replaced {
    let by = Some(session.subject.clone());
    end_sessions(&app, vec![replaced.id], by, SessionRevokeReason::Logout).await?;
  }
  let scope = Scope::new(&app.url_base, client.https);
  Ok(
    (
      StatusCode::NO_CONTENT,
      [(header::SET_COOKIE, session_cookie(&app, &session, &scope))],
    )
      .into_response(),
  )
}

/// Logs and raises a failed password login of `username` from `client`, and the answer to it.
fn failed_login(app: &AppState, username: &str, client: &Client) -> ApiError {
  if let Some(suppressed) = LOGIN_FAILED.allow() {
    warn!(username, ip = %client.ip, suppressed, "login failed");
  }
  app.events.raise(Event::LoginFailed(LoginFailed {
    version: 0,
    provider: "local".to_string(),
    ip: client.ip.to_string(),
    reason: "Invalid username or password".to_string(),
  }));
  ApiError::unauthorized("Invalid username or password").logged()
}

/// Whether there is authentication at all for the caller: false on a trusted network and when
/// authentication is off.
#[utoipa::path(
  get, path = "/session/enabled", tag = "session",
  responses((status = 200, body = bool, content_type = "application/json"))
)]
async fn enabled(State(app): State<AppState>, Extension(client): Extension<Client>) -> Json<bool> {
  let off = app.startup.values.web.authentication.disabled;
  Json(!off && networks::trusted(&client, &app.config.current().values, off).is_none())
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
  let trusted = networks::trusted(
    &client,
    options,
    app.startup.values.web.authentication.disabled,
  )
  .is_some();
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

/// Ends the caller's browser session and clears its cookie, also a session whose user has no role
/// right now. For a session from an OpenID Connect provider that has a logout page, that page is
/// returned for the browser to go to.
#[utoipa::path(
  post, path = "/session/logout", tag = "session",
  responses(
    (status = 200, body = LogoutResponse), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody)
  )
)]
async fn logout(
  State(app): State<AppState>,
  Extension(Authentication(principal)): Extension<Authentication>,
  Extension(presented): Extension<Presented>,
  session: Option<Extension<RequestSession>>,
) -> Result<Response, ApiError> {
  let session = session.map(|Extension(RequestSession(session))| session);
  // a session whose user has no role right now comes without a principal
  let by = match (&principal, &session) {
    (Some(principal), _) => principal.subject.clone(),
    (None, Some(session)) => session.subject.clone(),
    (None, None) => return Err(ApiError::unauthorized("Authentication required")),
  };
  let scope = Scope::new(&app.url_base, presented.client.https);
  let mut redirect = None;
  if let Some(session) = session {
    // revoked before the provider is asked for its logout page, which can take seconds, so a
    // caller that leaves meanwhile is still logged out
    end_sessions(
      &app,
      vec![session.id.clone()],
      Some(by),
      SessionRevokeReason::Logout,
    )
    .await?;
    redirect = provider_logout(&app, &session, &presented.client).await;
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
  operation_id = "list_sessions",
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
  let options = app.config.current();
  let authentication = &options.values.web.authentication;
  // a session whose user the configuration gives no role is ended once the configuration
  // stands, so it is not listed
  Ok(Json(
    active
      .iter()
      .filter_map(|session| {
        let role = session.role(authentication)?;
        Some(session.to_api(current.as_deref() == Some(session.id.as_str()), role))
      })
      .collect(),
  ))
}

/// Revokes one browser session, the caller's own or, for an administrator, anyone's.
#[utoipa::path(
  operation_id = "revoke_session",
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
    return Err(ApiError::not_found("No such session"));
  };
  end_sessions(
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
  end_sessions(
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

/// The OpenID Connect provider's logout page for `session`, when it came from a provider that
/// offers one and logging out there is on.
async fn provider_logout(
  app: &AppState,
  session: &StoredSession,
  client: &Client,
) -> Option<String> {
  let Origin::Oidc(name) = &session.sign_in.origin else {
    return None;
  };
  let current = app.config.current();
  let provider = match app.auth.oidc.get(name, &app.config).await {
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

/// Refuses password login when it is off or `client`'s address may not use it.
fn password_login_offered(options: &Options, client: &Client) -> Result<(), ApiError> {
  if !options.web.authentication.local.enabled {
    return Err(ApiError::forbidden("Password login is disabled"));
  }
  if !offered_to(options, client) {
    return Err(ApiError::forbidden(
      "Password login is not offered to this address",
    ));
  }
  Ok(())
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

  use crate::testing::{OUTSIDE, app, request, set_cookie, with};

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
  async fn a_lowered_or_removed_user_loses_the_role_on_the_next_request() {
    let config = |role: Option<&str>| match role {
      Some(role) => format!(
        "[web.authentication.local.users.viewer]\npassword = \"look\"\nrole = \"{role}\"\n\
         [web.authentication.api_keys.robot]\nkey = \"robot-key-0123456789\"\nrole = \"{role}\"\n"
      ),
      None => String::new(),
    };
    let app = app(&config(Some("administrator")), &[]).await;
    let reload = async |text: String| {
      std::fs::write(app.dir.path().join("config.toml"), text).unwrap();
      app.state.config.reload().await.unwrap();
    };
    let admin = signed_in(&app).await;
    let viewer_login = async || {
      let signed = app
        .send_from(
          OUTSIDE,
          request(
            "POST",
            "/api/v0/session",
            Some(json!({ "username": "viewer", "password": "look" })),
          ),
        )
        .await;
      set_cookie(&signed.headers, "reseek_session").unwrap()
    };
    let cookie = viewer_login().await;
    let created = app
      .send_from(
        OUTSIDE,
        with(
          with(
            request(
              "POST",
              "/api/v0/tokens",
              Some(json!({ "name": "script", "role": "Administrator" })),
            ),
            "cookie",
            &cookie,
          ),
          "origin",
          "http://localhost",
        ),
      )
      .await;
    assert_eq!(created.status, 201, "{:?}", created.body);
    let token = format!("Bearer {}", created.json()["secret"].as_str().unwrap());
    let credentials = [
      ("cookie", cookie.as_str()),
      ("x-api-key", "robot-key-0123456789"),
      ("authorization", token.as_str()),
    ];
    let send = |uri: &str, (name, value): (&'static str, &str)| {
      app.send_from(OUTSIDE, with(request("GET", uri, None), name, value))
    };
    for credential in credentials {
      let answer = send("/api/v0/options/view", credential).await;
      assert_eq!(answer.status, 200, "{credential:?}");
    }

    reload(config(Some("readonly"))).await;
    for credential in credentials {
      let answer = send("/api/v0/options/view", credential).await;
      assert_eq!(answer.status, 403, "{credential:?} is no administrator now");
    }
    for credential in [credentials[0], credentials[2]] {
      let me = send("/api/v0/session", credential).await.json();
      assert_eq!(me["role"], "ReadOnly", "{credential:?}");
    }
    let listed = send("/api/v0/sessions", ("cookie", admin.as_str()))
      .await
      .json();
    let roles: Vec<&str> = listed
      .as_array()
      .unwrap()
      .iter()
      .map(|session| session["role"].as_str().unwrap())
      .collect();
    assert_eq!(roles, ["ReadOnly", "Administrator"]);

    reload(config(None)).await;
    for credential in &credentials[1..] {
      let answer = send("/api/v0/session", *credential).await;
      assert_eq!(answer.status, 401, "{credential:?} is gone");
    }
    // the session goes on until the removal stands, its cookie kept
    let withheld = send("/api/v0/session", credentials[0]).await;
    assert_eq!(withheld.status, 403);
    assert_eq!(withheld.json()["error"], "access_withheld");
    assert!(set_cookie(&withheld.headers, "reseek_session").is_none());
    let listed = send("/api/v0/sessions", ("cookie", admin.as_str()))
      .await
      .json();
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");

    reload(config(Some("readonly"))).await;
    let me = send("/api/v0/session", credentials[2]).await.json();
    assert_eq!(me["role"], "ReadOnly", "the token follows its owner again");
  }

  #[tokio::test]
  async fn a_session_whose_user_has_no_role_now_may_sign_out_or_in_and_nothing_else() {
    use crate::auth::sessions;

    let viewer = "[web.authentication.local.users.viewer]\npassword = \"look\"\n";
    let app = app(viewer, &[]).await;
    let reload = async |text: &str| {
      std::fs::write(app.dir.path().join("config.toml"), text).unwrap();
      app.state.config.reload().await.unwrap();
    };
    let as_cookie = |r, cookie: &str| with(with(r, "cookie", cookie), "origin", "http://localhost");
    let withheld = async || {
      reload(viewer).await;
      let login = json!({ "username": "viewer", "password": "look" });
      let signed = app
        .send_from(OUTSIDE, request("POST", "/api/v0/session", Some(login)))
        .await;
      let cookie = set_cookie(&signed.headers, "reseek_session").unwrap();
      reload("").await;
      cookie
    };
    let ended = async |cookie: &str| {
      let id = cookie.split_once('=').unwrap().1.split_once('.').unwrap().0;
      let session = sessions::find(&app.state.auth.db, id).await.unwrap();
      session.unwrap().revoked_at.is_some()
    };

    let cookie = withheld().await;
    let refused = app
      .send_from(
        OUTSIDE,
        as_cookie(request("GET", "/api/v0/sessions", None), &cookie),
      )
      .await;
    assert_eq!(refused.status, 403);
    let out = app
      .send_from(
        OUTSIDE,
        as_cookie(request("POST", "/api/v0/session/logout", None), &cookie),
      )
      .await;
    assert_eq!(out.status, 200, "{:?}", out.body);
    assert_eq!(
      set_cookie(&out.headers, "reseek_session").as_deref(),
      Some("reseek_session=")
    );
    assert!(ended(&cookie).await);

    let cookie = withheld().await;
    let other = as_cookie(
      request("POST", "/api/v0/session", Some(credentials("reseek"))),
      &cookie,
    );
    let signed = app.send_from(OUTSIDE, other).await;
    assert_eq!(signed.status, 204, "{:?}", signed.body);
    assert!(set_cookie(&signed.headers, "reseek_session").is_some());
    assert!(ended(&cookie).await, "the new session replaces it");
  }

  #[tokio::test]
  async fn correct_logins_at_once_wait_their_turn_and_wrong_ones_still_count() {
    use std::time::Duration;

    let app = app("", &[]).await;
    let login = |password: &str| {
      app.send_from(
        OUTSIDE,
        request("POST", "/api/v0/session", Some(credentials(password))),
      )
    };
    // the password checks wait until the hold drops, so every login is under way at once
    let at_once = async |password: &str, count: usize| {
      let held = app.state.auth.accounts.hold_checks().await;
      let logins = futures_util::future::join_all((0..count).map(|_| login(password)));
      tokio::pin!(logins);
      let waited = tokio::time::timeout(Duration::from_millis(200), &mut logins).await;
      assert!(waited.is_err(), "the logins wait for the held checks");
      drop(held);
      let mut statuses: Vec<u16> = logins.await.iter().map(|a| a.status.as_u16()).collect();
      statuses.sort_unstable();
      statuses
    };
    assert_eq!(at_once("reseek", 10).await, [204; 10]);
    assert_eq!(
      at_once("wrong", 8).await,
      [401, 401, 401, 401, 401, 429, 429, 429]
    );
    assert_eq!(login("reseek").await.status, 429, "the failures count");
  }

  #[tokio::test]
  async fn a_sign_in_body_that_does_not_read_is_answered_with_why() {
    let app = app("", &[]).await;
    let mut cut = request("POST", "/api/v0/session", Some(json!({})));
    *cut.body_mut() = axum::body::Body::from("{\"username\": \"reseek\", ");
    let answer = app.send_from(OUTSIDE, cut).await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.json()["error"], "Invalid request body");

    let mut broken = request("POST", "/api/v0/session", Some(json!({})));
    let pieces: [Result<&str, std::io::Error>; 2] = [
      Ok("{\"username\": "),
      Err(std::io::Error::other("connection reset")),
    ];
    *broken.body_mut() = axum::body::Body::from_stream(futures_util::stream::iter(pieces));
    let answer = app.send_from(OUTSIDE, broken).await;
    assert_eq!(answer.status, 400);
    assert_eq!(answer.json()["error"], "Could not read the request body");

    let (logs, _capturing) = crate::testing::captured_logs();
    let typed = json!({ "username": "reseek", "password": 4815162342_u64 });
    let answer = app
      .send_from(OUTSIDE, request("POST", "/api/v0/session", Some(typed)))
      .await;
    assert_eq!(answer.status, 400);
    let detail = answer.json()["detail"].as_str().unwrap().to_string();
    assert!(
      detail.contains("4815162342"),
      "the client is told: {detail}"
    );
    let logged = logs.snapshot();
    assert!(
      logged.iter().any(|line| line
        .message
        .contains("detail=password does not fit what the route takes")),
      "{logged:?}"
    );
    assert!(
      !logged
        .iter()
        .any(|line| line.message.contains("4815162342")),
      "the password is not logged: {logged:?}"
    );

    let (logs, _capturing) = crate::testing::captured_logs();
    let missing = json!({ "username": "reseek" });
    let answer = app
      .send_from(OUTSIDE, request("POST", "/api/v0/session", Some(missing)))
      .await;
    assert_eq!(answer.status, 400);
    let detail = answer.json()["detail"].as_str().unwrap().to_string();
    assert!(detail.contains("missing field `password`"), "{detail}");
    let logged = logs.snapshot();
    assert!(
      logged.iter().any(|line| line
        .message
        .contains("detail=the body does not fit what the route takes")),
      "{logged:?}"
    );
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
    assert_eq!(bare.json()["error"], "Cross-site request refused");
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
      "Password login is not offered to this address"
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
    assert_eq!(off.json()["error"], "Password login is disabled");
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

  #[tokio::test]
  async fn headless_set_in_the_file_refuses_no_login_until_the_restart_it_takes() {
    let app = app("", &[]).await;
    app
      .configure_for_restart("headless = true\n", "headless")
      .await;
    signed_in(&app).await;
  }

  /// A logout whose caller leaves while the provider is asked for its logout page has revoked
  /// the session already.
  #[tokio::test]
  async fn a_logout_dropped_while_the_provider_is_asked_ends_the_session() {
    use crate::auth::sessions::{self, NewSession, Origin};

    // a provider that takes connections and never answers, so discovery waits
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", silent.local_addr().unwrap());
    tokio::spawn(async move {
      let mut open = Vec::new();
      while let Ok((stream, _)) = silent.accept().await {
        open.push(stream);
      }
    });
    let app = app(
      &format!(
        "[web.authentication.oidc.home]\nissuer = \"{issuer}\"\nclient_id = \"reseek\"\n\
         [web.authentication.oidc.home.role_mapping]\nreadwrite = [\"users\"]\n"
      ),
      &[],
    )
    .await;
    let auth = &app.state.auth;
    let session = sessions::create(
      &auth.db,
      NewSession {
        subject: "oidc:home:u1".to_string(),
        name: "alice".to_string(),
        email: None,
        groups: Some(vec!["users".to_string()]),
        role: api_types::auth::Role::ReadWrite,
        origin: Origin::Oidc("home".to_string()),
        ttl: chrono::Duration::hours(1),
        idp_sid: None,
        // without one there is no logout page to ask the provider for
        id_token: Some(auth.secret.seal(b"header.claims.signature")),
        ip: None,
        user_agent: None,
      },
    )
    .await
    .unwrap();
    let cookie = format!(
      "reseek_session={}",
      sessions::cookie_value(&auth.secret, &session.id)
    );
    let logout = with(
      with(
        request("POST", "/api/v0/session/logout", None),
        "cookie",
        &cookie,
      ),
      "origin",
      "http://localhost",
    );
    let left = tokio::time::timeout(
      std::time::Duration::from_millis(200),
      app.send_from(OUTSIDE, logout),
    )
    .await;
    assert!(left.is_err(), "the logout did not wait for the provider");
    let me = app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", &cookie),
      )
      .await;
    assert_eq!(me.status, 401);
  }

  #[tokio::test]
  async fn a_login_takes_the_role_of_a_reload_made_during_its_password_check() {
    let account = |role: &str| {
      format!("[web.authentication.local.users.viewer]\npassword = \"look\"\nrole = \"{role}\"\n")
    };
    let app = app(&account("administrator"), &[]).await;
    let held = app.state.auth.accounts.hold_checks().await;
    let login = app.send_from(
      OUTSIDE,
      request(
        "POST",
        "/api/v0/session",
        Some(json!({ "username": "viewer", "password": "look" })),
      ),
    );
    tokio::pin!(login);
    let waited = tokio::time::timeout(std::time::Duration::from_millis(200), &mut login).await;
    assert!(waited.is_err(), "the login waits for its password check");
    std::fs::write(app.dir.path().join("config.toml"), account("readonly")).unwrap();
    app.state.config.reload().await.unwrap();
    drop(held);
    let signed = login.await;
    assert_eq!(signed.status, 204, "{:?}", signed.body);
    let cookie = set_cookie(&signed.headers, "reseek_session").unwrap();
    let me = app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", &cookie),
      )
      .await;
    assert_eq!(me.json()["role"], "ReadOnly");
  }
}
