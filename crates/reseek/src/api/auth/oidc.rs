//! The OpenID Connect routes: leaving for a provider's login, coming back from it, the
//! provider's back-channel logout, and testing a provider's discovery.

use api_types::auth::session::{
  BackchannelLogout, OidcCallbackQuery, OidcEndpoints, OidcLoginQuery, OidcTestResult,
};
use api_types::events::{Event, LoginFailed, SessionRevokeReason};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Json};
use openidconnect::RedirectUrl;
use soulseek_client::LogLimiter;
use std::sync::Arc;
use tracing::{debug, info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::auth::cookies::{self, Scope};
use crate::auth::oidc::flow::{self, LOGIN_TIME, is_local_path};
use crate::auth::oidc::logout::{check_token, end_session_url};
use crate::auth::oidc::{Provider, external_base};
use crate::auth::sessions::{self, NewSession, Origin};
use crate::auth::{
  Administrator, Authorized, Presented, end_sessions, session_cookie, start_session,
};

static CALLBACK_FAILED: LogLimiter = LogLimiter::new();
static PROVIDER_DOWN: LogLimiter = LogLimiter::new();
static LOGOUT_UNCHECKED: LogLimiter = LogLimiter::new();
static LOGOUT_REFUSED: LogLimiter = LogLimiter::new();

/// The OpenID Connect routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(login))
    .routes(routes!(callback))
    .routes(routes!(backchannel))
    .routes(routes!(test))
}

/// Sends the browser to the provider's login, remembering the login in a sealed cookie.
#[utoipa::path(
  operation_id = "oidc_login",
  get, path = "/auth/oidc/{name}/login", tag = "auth", params(
    ("name" = String, Path, description = "the provider's name in the configuration"),
    OidcLoginQuery
  ),
  responses(
    (status = 302, description = "to the provider's login"),
    (status = 400, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody),
    (status = 503, body = api_types::ErrorBody)
  )
)]
async fn login(
  State(app): State<AppState>,
  Extension(presented): Extension<Presented>,
  Path(name): Path<String>,
  Query(query): Query<OidcLoginQuery>,
) -> Result<Response, ApiError> {
  let provider = provider(&app, &name)?;
  let redirect = query
    .redirect
    .unwrap_or_else(|| format!("{}/", app.url_base));
  if !is_local_path(&redirect) {
    return Err(ApiError::bad_request(
      "The redirect must be a path on this site",
    ));
  }
  let callback = callback_url(&app, &presented, &name)?;
  let metadata = provider
    .metadata()
    .await
    .map_err(|e| unavailable(&name, &e))?;
  let (location, sealed) = flow::begin(&provider, metadata, callback, redirect, &app.auth.secret);
  let scope = Scope::new(&app.url_base, presented.client.https);
  debug!(provider = name, "login sent to the identity provider");
  let mut response = found(&location);
  response.headers_mut().append(
    header::SET_COOKIE,
    scope.set(&flow::cookie_name(&name), &sealed, LOGIN_TIME),
  );
  Ok(response)
}

/// Where the provider sends the browser back to: checks the login, signs the user in with a
/// session cookie and returns them to the page they came from.
#[utoipa::path(
  get, path = "/auth/oidc/{name}/callback", tag = "auth", params(
    ("name" = String, Path, description = "the provider's name in the configuration"),
    OidcCallbackQuery
  ),
  responses(
    (status = 302, description = "signed in, or to the login page with the provider's error"),
    (status = 400, body = api_types::ErrorBody, description = "the login expired"),
    (status = 403, description = "the account is not let in", content_type = "text/html"),
    (status = 404, body = api_types::ErrorBody), (status = 429, body = api_types::ErrorBody)
  )
)]
async fn callback(
  State(app): State<AppState>,
  Extension(presented): Extension<Presented>,
  Path(name): Path<String>,
  Query(query): Query<OidcCallbackQuery>,
  headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
  let provider = provider(&app, &name)?;
  let client = &presented.client;
  if app.auth.limits.logins.blocked(client.ip) {
    return Err(ApiError::rate_limited());
  }
  let scope = Scope::new(&app.url_base, client.https);
  let cookie_name = flow::cookie_name(&name);
  let clear = scope.clear(&cookie_name);
  let failed = |reason: String| {
    app.auth.limits.logins.failed(client.ip);
    if let Some(suppressed) = CALLBACK_FAILED.allow() {
      warn!(provider = name, ip = %client.ip, reason, suppressed, "login failed");
    }
    app.events.raise(Event::LoginFailed(LoginFailed {
      version: 0,
      provider: name.clone(),
      ip: client.ip.to_string(),
      reason,
    }));
  };
  if let Some(error) = &query.error {
    let text = query
      .error_description
      .clone()
      .unwrap_or_else(|| error.clone());
    failed(format!("the identity provider refused the login: {error}"));
    return Ok(to_login_page(&app, &text, clear));
  }
  let login = match (
    cookies::find(&headers, &cookie_name),
    query.state.as_deref(),
  ) {
    (Some(sealed), Some(state)) => flow::opened(&app.auth.secret, sealed, state),
    _ => None,
  };
  let Some(login) = login else {
    failed("the login expired or its state did not match".to_string());
    let mut response = ApiError::bad_request("The login expired. Try again.")
      .logged()
      .into_response();
    response.headers_mut().append(header::SET_COOKIE, clear);
    return Ok(response);
  };
  let Some(code) = query.code else {
    failed("the identity provider sent no code".to_string());
    return Ok(to_login_page(
      &app,
      "the sign-in could not be completed",
      clear,
    ));
  };
  let callback = callback_url(&app, &presented, &name)?;
  let metadata = provider
    .metadata()
    .await
    .map_err(|e| unavailable(&name, &e))?;
  let claims = match flow::exchange(&provider, metadata.clone(), callback, code, &login).await {
    Ok(claims) => claims,
    Err(reason) => {
      failed(reason);
      return Ok(to_login_page(
        &app,
        "the sign-in could not be completed",
        clear,
      ));
    }
  };
  let admitted = match flow::admit(&provider, &claims) {
    Ok(admitted) => admitted,
    Err(reason) => {
      failed(format!("not authorized: {reason}"));
      let back = external_base(&app.url_base, &app.config.current().values, client)
        .map(|base| format!("{base}/"))
        .unwrap_or_default();
      let sign_out = end_session_url(&metadata, &claims.id_token, &back);
      let mut response = refused_page(sign_out.as_deref());
      response.headers_mut().append(header::SET_COOKIE, clear);
      return Ok(response);
    }
  };
  let session = start_session(
    &app,
    &presented,
    NewSession {
      subject: format!("oidc:{name}:{}", claims.subject),
      name: admitted.name,
      email: admitted.email,
      groups: serde_json::to_string(&admitted.groups).ok(),
      role: admitted.role,
      origin: Origin::Oidc(name.clone()),
      ttl: sessions::lifetime(provider.options.session_ttl),
      idp_sid: claims.sid,
      id_token: Some(app.auth.secret.seal(claims.id_token.as_bytes())),
      ip: Some(client.ip.to_string()),
      user_agent: presented.user_agent(),
    },
  )
  .await?;
  let mut response = found(&login.redirect);
  let set = response.headers_mut();
  set.append(header::SET_COOKIE, session_cookie(&app, &session, &scope));
  set.append(header::SET_COOKIE, clear);
  Ok(response)
}

/// Where the provider ends sessions it started, with a signed logout token naming its session
/// or its user. Answered 404 for a provider that takes no back-channel logout whatever the
/// body, else always 200, a body that is not a logout form included.
#[utoipa::path(
  post, path = "/auth/oidc/{name}/backchannel", tag = "auth",
  params(("name" = String, Path, description = "the provider's name in the configuration")),
  request_body(content = BackchannelLogout, content_type = "application/x-www-form-urlencoded"),
  responses((status = 200, description = "taken"), (status = 404, body = api_types::ErrorBody))
)]
async fn backchannel(
  State(app): State<AppState>,
  Path(name): Path<String>,
  body: Bytes,
) -> Result<StatusCode, ApiError> {
  let provider = provider(&app, &name)?;
  if !provider.options.logout.back_channel {
    return Err(ApiError::not_found(
      "Back-channel logout is disabled for this provider",
    ));
  }
  let form = match serde_html_form::from_bytes::<BackchannelLogout>(&body) {
    Ok(form) => form,
    Err(e) => {
      if let Some(suppressed) = LOGOUT_REFUSED.allow() {
        warn!(
          provider = name,
          reason = %e,
          suppressed,
          "back-channel logout refused; the body is no logout form"
        );
      }
      return Ok(StatusCode::OK);
    }
  };
  let metadata = match provider.metadata().await {
    Ok(metadata) => metadata,
    Err(e) => {
      if let Some(suppressed) = LOGOUT_UNCHECKED.allow() {
        warn!(
          provider = name,
          error = e,
          suppressed,
          "back-channel logout not checked; provider unavailable"
        );
      }
      return Ok(StatusCode::OK);
    }
  };
  let logged_out = match check_token(&provider, metadata, &form.logout_token).await {
    Ok(logged_out) => logged_out,
    Err(reason) => {
      if let Some(suppressed) = LOGOUT_REFUSED.allow() {
        warn!(
          provider = name,
          reason, suppressed, "back-channel logout token refused"
        );
      }
      return Ok(StatusCode::OK);
    }
  };
  let origin = Origin::Oidc(name.clone());
  let subject = logged_out.sub.map(|sub| format!("oidc:{name}:{sub}"));
  let ids = sessions::of_provider_login(&app.auth.db, &origin, logged_out.sid, subject)
    .await
    .map_err(ApiError::internal)?;
  info!(
    provider = name,
    count = ids.len(),
    "identity provider ended sessions"
  );
  end_sessions(&app, ids, None, SessionRevokeReason::Backchannel).await?;
  Ok(StatusCode::OK)
}

/// Reads a provider's discovery document now and reports what it names.
#[utoipa::path(
  post, path = "/auth/oidc/{name}/test", tag = "auth",
  params(("name" = String, Path, description = "the provider's name in the configuration")),
  responses(
    (status = 200, body = OidcTestResult), (status = 401, body = api_types::ErrorBody),
    (status = 403, body = api_types::ErrorBody), (status = 404, body = api_types::ErrorBody)
  )
)]
async fn test(
  State(app): State<AppState>,
  _admin: Authorized<Administrator>,
  Path(name): Path<String>,
) -> Result<Json<OidcTestResult>, ApiError> {
  let provider = provider(&app, &name)?;
  Ok(Json(match provider.discover().await {
    Ok(metadata) => OidcTestResult {
      ok: true,
      issuer: Some(metadata.issuer().to_string()),
      endpoints: Some(OidcEndpoints {
        authorization: metadata.authorization_endpoint().to_string(),
        token: metadata
          .token_endpoint()
          .map(ToString::to_string)
          .unwrap_or_default(),
        userinfo: metadata.userinfo_endpoint().map(ToString::to_string),
        jwks: metadata.jwks_uri().to_string(),
        end_session: metadata
          .additional_metadata()
          .end_session_endpoint
          .as_ref()
          .map(ToString::to_string),
      }),
      error: None,
    },
    Err(e) => {
      debug!(provider = name, error = e, "provider test failed");
      OidcTestResult {
        ok: false,
        issuer: None,
        endpoints: None,
        error: Some(e),
      }
    }
  }))
}

/// The 503 for a provider whose discovery document cannot be read, logged with why.
fn unavailable(name: &str, error: &str) -> ApiError {
  if let Some(suppressed) = PROVIDER_DOWN.allow() {
    warn!(
      provider = name,
      error, suppressed, "OpenID Connect provider unavailable"
    );
  }
  ApiError::service_unavailable("The identity provider is unavailable").logged()
}

/// The provider `name`, 404 when none is configured.
fn provider(app: &AppState, name: &str) -> Result<Arc<Provider>, ApiError> {
  match app.auth.oidc.get(name, &app.config.current().values) {
    Ok(Some(provider)) => Ok(provider),
    Ok(None) => Err(ApiError::not_found(format!(
      "No identity provider named {name}"
    ))),
    Err(e) => Err(ApiError::internal(format!("identity provider {name}: {e}"))),
  }
}

/// The callback URL the provider sends the browser back to.
fn callback_url(
  app: &AppState,
  presented: &Presented,
  name: &str,
) -> Result<RedirectUrl, ApiError> {
  let base = external_base(
    &app.url_base,
    &app.config.current().values,
    &presented.client,
  )
  .ok_or_else(|| ApiError::bad_request("A Host header is required to sign in"))?;
  RedirectUrl::new(format!("{base}/api/v0/auth/oidc/{name}/callback"))
    .map_err(|e| ApiError::bad_request(format!("The callback URL is not valid: {e}")))
}

/// The login page showing `error`, with the login cookie cleared.
fn to_login_page(app: &AppState, error: &str, clear: axum::http::HeaderValue) -> Response {
  let error: String = url::form_urlencoded::byte_serialize(error.as_bytes()).collect();
  let mut response = found(&format!("{}/#/login?error={error}", app.url_base));
  response.headers_mut().append(header::SET_COOKIE, clear);
  response
}

/// A `302 Found` to `location`.
fn found(location: &str) -> Response {
  match axum::http::HeaderValue::from_str(location) {
    Ok(location) => (StatusCode::FOUND, [(header::LOCATION, location)]).into_response(),
    Err(e) => ApiError::internal(format!("a redirect is not a header value: {e}")).into_response(),
  }
}

/// The page for an account the provider signed in that the configuration does not let in.
fn refused_page(sign_out: Option<&str>) -> Response {
  let link = sign_out
    .map(|url| {
      let url = url
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
      format!("<p><a href=\"{url}\">Sign out of SSO</a></p>")
    })
    .unwrap_or_default();
  (
    StatusCode::FORBIDDEN,
    Html(format!(
      "<!doctype html><html><head><meta charset=\"utf-8\"><title>Not authorized</title></head>\
       <body><p>Your account is not authorized for this application.</p>{link}</body></html>"
    )),
  )
    .into_response()
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};

  use crate::testing::issuer::{CLIENT_ID, CLIENT_SECRET, Grant, Issuer};
  use crate::testing::{Answer, OUTSIDE, TestApp, app, request, set_cookie, with};

  async fn setup(default: &str) -> (Issuer, TestApp) {
    let issuer = Issuer::start().await;
    let app = app(
      &format!(
        "[web.authentication.oidc.home]\nissuer = \"{}\"\nclient_id = \"{CLIENT_ID}\"\n\
         client_secret = \"{CLIENT_SECRET}\"\n[web.authentication.oidc.home.role_mapping]\n\
         administrator = [\"admins\"]\nreadwrite = [\"users\"]\ndefault = \"{default}\"\n",
        issuer.url
      ),
      &[],
    )
    .await;
    (issuer, app)
  }

  /// What a login sent to the provider: its parameters and the login cookie.
  struct Started {
    params: std::collections::HashMap<String, String>,
    cookie: String,
  }

  async fn start(app: &TestApp, redirect: &str) -> Started {
    let answer = app
      .send_from(
        OUTSIDE,
        request(
          "GET",
          &format!("/api/v0/auth/oidc/home/login?redirect={redirect}"),
          None,
        ),
      )
      .await;
    assert_eq!(answer.status, 302, "{:?}", answer.body);
    let location = url::Url::parse(answer.headers["location"].to_str().unwrap()).unwrap();
    assert!(location.path().ends_with("/authorize"));
    let params: std::collections::HashMap<String, String> =
      location.query_pairs().into_owned().collect();
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["response_type"], "code");
    assert!(params["scope"].contains("groups"));
    assert_eq!(
      params["redirect_uri"],
      "http://localhost/api/v0/auth/oidc/home/callback"
    );
    Started {
      params,
      cookie: set_cookie(&answer.headers, "oidc_home").unwrap(),
    }
  }

  fn grant(issuer: &Issuer, started: &Started, code: &str, claims: Value) {
    issuer.grant(
      code,
      Grant {
        challenge: started.params["code_challenge"].clone(),
        redirect_uri: started.params["redirect_uri"].clone(),
        claims,
      },
    );
  }

  async fn back(app: &TestApp, started: &Started, code: &str) -> Answer {
    let state = &started.params["state"];
    app
      .send_from(
        OUTSIDE,
        with(
          request(
            "GET",
            &format!("/api/v0/auth/oidc/home/callback?code={code}&state={state}"),
            None,
          ),
          "cookie",
          &started.cookie,
        ),
      )
      .await
  }

  async fn me(app: &TestApp, cookie: &str) -> Answer {
    app
      .send_from(
        OUTSIDE,
        with(request("GET", "/api/v0/session", None), "cookie", cookie),
      )
      .await
  }

  #[tokio::test]
  async fn a_provider_login_maps_groups_to_a_role_and_back_channel_logout_ends_it() {
    let (issuer, app) = setup("deny").await;
    let started = start(&app, "/%23/transfers").await;
    let nonce = started.params["nonce"].clone();
    grant(
      &issuer,
      &started,
      "c1",
      json!({ "sub": "u1", "nonce": nonce, "sid": "s1", "preferred_username": "alice", "groups": ["users"] }),
    );
    let signed = back(&app, &started, "c1").await;
    assert_eq!(signed.status, 302, "{:?}", signed.body);
    assert_eq!(signed.headers["location"], "/#/transfers");
    assert_eq!(
      set_cookie(&signed.headers, "oidc_home").as_deref(),
      Some("oidc_home=")
    );
    let cookie = set_cookie(&signed.headers, "reseek_session").unwrap();
    let who = me(&app, &cookie).await.json();
    assert_eq!(who["subject"], "oidc:home:u1");
    assert_eq!(who["name"], "alice");
    assert_eq!(who["role"], "ReadWrite");
    assert_eq!(who["provider"], "home");

    let logout = app
      .send_from(
        OUTSIDE,
        with(
          with(
            request("POST", "/api/v0/session/logout", None),
            "cookie",
            &cookie,
          ),
          "origin",
          "http://localhost",
        ),
      )
      .await
      .json();
    let redirect = logout["redirect"].as_str().unwrap();
    assert!(redirect.starts_with(&format!("{}/logout?id_token_hint=", issuer.url)));
    assert!(redirect.contains("post_logout_redirect_uri=http%3A%2F%2Flocalhost%2F"));

    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    grant(
      &issuer,
      &started,
      "c2",
      json!({ "sub": "u1", "nonce": nonce, "sid": "s2", "groups": ["admins", "users"] }),
    );
    let cookie = set_cookie(&back(&app, &started, "c2").await.headers, "reseek_session").unwrap();
    assert_eq!(me(&app, &cookie).await.json()["role"], "Administrator");
    let now = chrono::Utc::now().timestamp();
    let logout = |claims: Value| {
      let token = issuer.sign(&claims);
      axum::http::Request::builder()
        .method("POST")
        .uri("/api/v0/auth/oidc/home/backchannel")
        .header("host", "localhost")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(axum::body::Body::from(format!("logout_token={token}")))
        .unwrap()
    };
    let events = json!({ "http://schemas.openid.net/event/backchannel-logout": {} });
    let lasting = app
      .send_from(
        OUTSIDE,
        logout(json!({ "iss": issuer.url, "aud": CLIENT_ID, "iat": now, "jti": "j0", "sid": "s2", "events": events })),
      )
      .await;
    assert_eq!(lasting.status, 200, "always answered 200");
    assert_eq!(
      me(&app, &cookie).await.status,
      200,
      "a logout token without an expiry ends nothing"
    );
    let unnamed = app
      .send_from(
        OUTSIDE,
        logout(json!({
          "iss": issuer.url, "aud": CLIENT_ID, "iat": now, "exp": now + 120, "sid": "s2",
          "events": events,
        })),
      )
      .await;
    assert_eq!(unnamed.status, 200);
    assert_eq!(
      me(&app, &cookie).await.status,
      200,
      "a logout token without a jti ends nothing"
    );
    let posted = app
      .send_from(
        OUTSIDE,
        logout(json!({
          "iss": issuer.url, "aud": CLIENT_ID, "iat": now, "exp": now + 120, "jti": "j1",
          "sid": "s2", "events": events,
        })),
      )
      .await;
    assert_eq!(posted.status, 200);
    assert_eq!(
      me(&app, &cookie).await.status,
      401,
      "the provider ended the session"
    );
  }

  #[tokio::test]
  async fn back_channel_status_depends_on_the_provider_not_the_body() {
    let issuer = Issuer::start().await;
    let app = app(
      &format!(
        "[web.authentication.oidc.home]\nissuer = \"{0}\"\nclient_id = \"{CLIENT_ID}\"\n\
         [web.authentication.oidc.quiet]\nissuer = \"{0}\"\nclient_id = \"{CLIENT_ID}\"\n\
         [web.authentication.oidc.quiet.logout]\nback_channel = false\n",
        issuer.url
      ),
      &[],
    )
    .await;
    let post = |provider: &str, content_type: &str, body: &'static str| {
      axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/v0/auth/oidc/{provider}/backchannel"))
        .header("host", "localhost")
        .header("content-type", content_type)
        .body(axum::body::Body::from(body))
        .unwrap()
    };
    let form = "application/x-www-form-urlencoded";
    for (provider, content_type, body, status) in [
      ("nobody", "text/plain", "\u{0}", 404),
      ("quiet", form, "logout_token=x", 404),
      ("quiet", "application/json", "{", 404),
      ("home", "text/plain", "\u{0}", 200),
      ("home", form, "other=1", 200),
      ("home", form, "logout_token=not.a.token", 200),
    ] {
      let answer = app
        .send_from(OUTSIDE, post(provider, content_type, body))
        .await;
      assert_eq!(answer.status, status, "{provider} {content_type} {body:?}");
    }
  }

  #[tokio::test]
  async fn state_nonce_and_pkce_must_match_the_login() {
    let (issuer, app) = setup("readonly").await;
    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    let other = Started {
      params: started.params.clone(),
      cookie: start(&app, "/").await.cookie,
    };
    grant(
      &issuer,
      &started,
      "c1",
      json!({ "sub": "u1", "nonce": nonce }),
    );
    let crossed = back(&app, &other, "c1").await;
    assert_eq!(crossed.status, 400, "the state belongs to another login");
    assert_eq!(crossed.json()["error"], "The login expired. Try again.");

    let started = start(&app, "/").await;
    grant(
      &issuer,
      &started,
      "c2",
      json!({ "sub": "u1", "nonce": "not the one sent" }),
    );
    let replayed = back(&app, &started, "c2").await;
    assert_eq!(replayed.status, 302);
    assert!(
      replayed.headers["location"]
        .to_str()
        .unwrap()
        .starts_with("/#/login?error=")
    );
    assert!(set_cookie(&replayed.headers, "reseek_session").is_none());

    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    issuer.grant(
      "c3",
      Grant {
        challenge: "a challenge for another verifier".to_string(),
        redirect_uri: started.params["redirect_uri"].clone(),
        claims: json!({ "sub": "u1", "nonce": nonce }),
      },
    );
    let injected = back(&app, &started, "c3").await;
    assert_eq!(
      injected.status, 302,
      "a code for another login is refused at the token endpoint"
    );
    assert!(set_cookie(&injected.headers, "reseek_session").is_none());

    let refused = app
      .send_from(
        OUTSIDE,
        request(
          "GET",
          "/api/v0/auth/oidc/home/callback?error=access_denied&error_description=No",
          None,
        ),
      )
      .await;
    assert_eq!(refused.headers["location"], "/#/login?error=No");
    let away = app
      .send_from(
        OUTSIDE,
        request(
          "GET",
          "/api/v0/auth/oidc/home/login?redirect=//evil.example",
          None,
        ),
      )
      .await;
    assert_eq!(away.status, 400);
    assert_eq!(
      app
        .send_from(
          OUTSIDE,
          request("GET", "/api/v0/auth/oidc/nope/login", None)
        )
        .await
        .status,
      404
    );
  }

  #[tokio::test]
  async fn the_deny_default_refuses_users_in_no_group_and_userinfo_fills_in_groups() {
    let (issuer, app) = setup("deny").await;
    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    grant(
      &issuer,
      &started,
      "c1",
      json!({ "sub": "u2", "nonce": nonce, "groups": ["guests"] }),
    );
    let denied = back(&app, &started, "c1").await;
    assert_eq!(denied.status, 403);
    let page = String::from_utf8_lossy(&denied.body).into_owned();
    assert!(page.contains("Your account is not authorized for this application"));
    assert!(page.contains("Sign out of SSO"));

    issuer.set_userinfo(json!({ "sub": "u3", "groups": ["admins"] }));
    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    grant(
      &issuer,
      &started,
      "c2",
      json!({ "sub": "u3", "nonce": nonce }),
    );
    let cookie = set_cookie(&back(&app, &started, "c2").await.headers, "reseek_session").unwrap();
    assert_eq!(me(&app, &cookie).await.json()["role"], "Administrator");

    issuer.set_signed_userinfo(json!({ "sub": "u4", "groups": ["users"] }));
    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    grant(
      &issuer,
      &started,
      "c3",
      json!({ "sub": "u4", "nonce": nonce }),
    );
    let signed = set_cookie(&back(&app, &started, "c3").await.headers, "reseek_session")
      .expect("a signed userinfo answer is read");
    assert_eq!(me(&app, &signed).await.json()["role"], "ReadWrite");

    issuer.set_userinfo(json!({ "sub": "someone else", "groups": ["admins"] }));
    let started = start(&app, "/").await;
    let nonce = started.params["nonce"].clone();
    grant(
      &issuer,
      &started,
      "c4",
      json!({ "sub": "u5", "nonce": nonce }),
    );
    assert_eq!(
      back(&app, &started, "c4").await.status,
      403,
      "userinfo about another subject is not used, so the deny default applies"
    );
  }

  #[tokio::test]
  async fn the_provider_test_reads_discovery() {
    let issuer = Issuer::start().await;
    let app = app(
      &format!(
        "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n[web.authentication.oidc.home]\n\
         issuer = \"{}\"\nclient_id = \"{CLIENT_ID}\"\n[web.authentication.oidc.gone]\nissuer = \
         \"http://127.0.0.1:9/\"\nclient_id = \"x\"\n",
        issuer.url
      ),
      &[],
    )
    .await;
    let ok = app
      .send(request("POST", "/api/v0/auth/oidc/home/test", None))
      .await
      .json();
    assert_eq!(ok["ok"], true);
    assert_eq!(ok["endpoints"]["token"], format!("{}/token", issuer.url));
    assert_eq!(
      ok["endpoints"]["endSession"],
      format!("{}/logout", issuer.url)
    );
    let gone = app
      .send(request("POST", "/api/v0/auth/oidc/gone/test", None))
      .await
      .json();
    assert_eq!(gone["ok"], false);
    assert!(gone["error"].is_string());
    let stranger = app
      .send_from(
        OUTSIDE,
        request("POST", "/api/v0/auth/oidc/home/test", None),
      )
      .await;
    assert_eq!(stranger.status, 401);
  }
}
