//! The API token routes: listing tokens with the configuration's API keys, creating a token
//! and renaming it or changing its expiry (only from a login session, never with a token), and
//! revoking it.

use crate::api::extract::Json;
use api_types::auth::Role;
use api_types::auth::tokens::{
  ConfigApiKey, CreateToken, CreatedToken, ListedToken, Token, UpdateToken,
};
use api_types::events::{Event, TokenChanged};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use tracing::info;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::auth::keys::PRIMARY;
use crate::auth::sessions::lifetime;
use crate::auth::tokens::{self, NewToken, StoredToken};
use crate::auth::{AnyRole, Authorized, ReadWrite, owner_role, require_interactive};
use crate::config::cidr::parse_list;
use crate::db::{latest, shifted};

/// The most characters a token's name has.
const NAME_CHARS: usize = 64;

/// The API token routes.
pub fn routes() -> OpenApiRouter<AppState> {
  OpenApiRouter::new()
    .routes(routes!(list, create))
    .routes(routes!(update, revoke))
}

/// The caller's tokens, or everyone's for an administrator, who also sees the configuration's
/// API keys. Revoked tokens are not listed.
#[utoipa::path(
  operation_id = "list_tokens",
  get, path = "/tokens", tag = "tokens",
  responses((status = 200, body = Vec<ListedToken>), (status = 401, body = api_types::ErrorBody))
)]
async fn list(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
) -> Result<Json<Vec<ListedToken>>, ApiError> {
  let admin = caller.principal.role == Role::Administrator;
  let owner = (!admin).then(|| caller.principal.subject.clone());
  let mut listed: Vec<ListedToken> = tokens::list(&app.auth.db, owner)
    .await
    .map_err(ApiError::internal)?
    .iter()
    .map(|token| ListedToken::Api(token.to_api()))
    .collect();
  if admin {
    if let Some(primary) = &app.primary_key {
      listed.push(ListedToken::Config(ConfigApiKey {
        name: PRIMARY.to_string(),
        role: primary.role.into(),
        cidr: primary
          .cidrs
          .iter()
          .map(ToString::to_string)
          .collect::<Vec<_>>()
          .join(","),
      }));
    }
    let current = app.config.current();
    for (name, key) in &current.values.web.authentication.api_keys {
      listed.push(ListedToken::Config(ConfigApiKey {
        name: name.clone(),
        role: key.role.into(),
        cidr: key.cidr.clone(),
      }));
    }
  }
  Ok(Json(listed))
}

/// Creates a token with at most the caller's own role. Its secret is in the answer and is
/// never shown again. Needs a login session: a token cannot make tokens. Refused to a caller
/// whose role cannot be read again later, such as a proxy user with no web UI session, since
/// the token acts with no more than its owner's role on each request.
#[utoipa::path(
  post, path = "/tokens", tag = "tokens", request_body = CreateToken,
  responses(
    (status = 201, body = CreatedToken), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody)
  )
)]
async fn create(
  State(app): State<AppState>,
  caller: Authorized<ReadWrite>,
  Json(request): Json<CreateToken>,
) -> Result<Response, ApiError> {
  require_interactive(&caller.principal)?;
  check_name(&request.name)?;
  let current = app.config.current();
  let owner = owner_role(&app, &caller.principal.subject, &current.values)
    .await
    .map_err(ApiError::internal)?;
  if owner.is_none() {
    return Err(ApiError::forbidden(
      "Your role cannot be read again on later requests, so a token of yours would not work",
    ));
  }
  if request.role > caller.principal.role {
    return Err(ApiError::forbidden(
      "A token cannot have a higher role than its creator",
    ));
  }
  let expires_at = match (&request.expires_in, request.expires_at) {
    (Some(_), Some(_)) => {
      return Err(ApiError::bad_request(
        "Specify expiresIn or expiresAt, not both",
      ));
    }
    (Some(text), None) => {
      let lasts = humantime::parse_duration(text).map_err(|e| {
        ApiError::invalid("The value of expiresIn is not a duration", e.to_string())
      })?;
      Some(shifted(Utc::now(), lifetime(lasts)))
    }
    (None, at) => at,
  };
  check_expiry(expires_at)?;
  if let Some(cidr) = &request.cidr
    && let Err((element, e)) = parse_list(cidr)
  {
    return Err(ApiError::invalid(
      "The value of cidr is not a list of networks",
      format!("{element}: {e}"),
    ));
  }
  let (token, secret) = tokens::create(
    &app.auth.db,
    NewToken {
      name: request.name,
      role: request.role,
      owner: caller.principal.subject.clone(),
      cidr: request.cidr,
      expires_at,
    },
  )
  .await
  .map_err(ApiError::internal)?;
  info!(
    id = %token.id,
    name = token.name,
    by = caller.principal.subject,
    "API token created"
  );
  raise(&app, &token, &caller.principal.subject, true);
  Ok(
    (
      StatusCode::CREATED,
      Json(CreatedToken {
        token: token.to_api(),
        secret,
      }),
    )
      .into_response(),
  )
}

/// Renames a token or changes its expiry; the owner's or, for an administrator, anyone's.
/// Needs a login session, so a token cannot put off its own expiry.
#[utoipa::path(
  put, path = "/tokens/{id}", tag = "tokens", request_body = UpdateToken,
  params(("id" = Uuid, Path, description = "the token's id")),
  responses(
    (status = 200, body = Token), (status = 400, body = api_types::ErrorBody),
    (status = 401, body = api_types::ErrorBody), (status = 403, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn update(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Path(id): Path<Uuid>,
  Json(request): Json<UpdateToken>,
) -> Result<Json<Token>, ApiError> {
  require_interactive(&caller.principal)?;
  visible(&app, &caller, id).await?;
  if let Some(name) = &request.name {
    check_name(name)?;
  }
  check_expiry(request.expires_at)?;
  let token = tokens::update(&app.auth.db, id, request.name, request.expires_at)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::not_found("No such token"))?;
  info!(id = %id, by = caller.principal.subject, "API token changed");
  Ok(Json(token.to_api()))
}

/// Revokes a token, closing the event streams it opened; the owner's or, for an administrator,
/// anyone's. The record is kept a month for the audit view.
#[utoipa::path(
  operation_id = "revoke_token",
  delete, path = "/tokens/{id}", tag = "tokens",
  params(("id" = Uuid, Path, description = "the token's id")),
  responses(
    (status = 204, description = "revoked"), (status = 401, body = api_types::ErrorBody),
    (status = 404, body = api_types::ErrorBody)
  )
)]
async fn revoke(
  State(app): State<AppState>,
  caller: Authorized<AnyRole>,
  Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
  let token = visible(&app, &caller, id).await?;
  if !tokens::revoke(&app.auth.db, id)
    .await
    .map_err(ApiError::internal)?
  {
    return Err(ApiError::not_found("No such token"));
  }
  info!(id = %id, by = caller.principal.subject, "API token revoked");
  app.live.credentials_changed();
  raise(&app, &token, &caller.principal.subject, false);
  Ok(StatusCode::NO_CONTENT)
}

/// The token `id` when the caller may change it: its owner or an administrator. Anyone else
/// is told it does not exist.
async fn visible(
  app: &AppState,
  caller: &Authorized<AnyRole>,
  id: Uuid,
) -> Result<StoredToken, ApiError> {
  tokens::get(&app.auth.db, id)
    .await
    .map_err(ApiError::internal)?
    .filter(|token| {
      caller.principal.role == Role::Administrator || token.owner == caller.principal.subject
    })
    .ok_or_else(|| ApiError::not_found("No such token"))
}

fn check_name(name: &str) -> Result<(), ApiError> {
  let count = name.chars().count();
  if name.trim().is_empty() || count > NAME_CHARS {
    return Err(ApiError::bad_request(format!(
      "The name must have 1 to {NAME_CHARS} characters"
    )));
  }
  Ok(())
}

fn check_expiry(expires_at: Option<chrono::DateTime<Utc>>) -> Result<(), ApiError> {
  match expires_at {
    Some(at) if at <= Utc::now() => Err(ApiError::bad_request("The expiry is in the past")),
    // a later time is not stored in time order, so pruning would take it for an old one
    Some(at) if at > latest() => Err(ApiError::bad_request("The expiry is after the year 9999")),
    _ => Ok(()),
  }
}

/// Raises `TokenCreated` or `TokenRevoked` for `token`, done by `by`.
fn raise(app: &AppState, token: &StoredToken, by: &str, created: bool) {
  let changed = TokenChanged {
    version: 0,
    token_id: token.id,
    name: token.name.clone(),
    role: token.role,
    by: by.to_string(),
  };
  app.events.raise(if created {
    Event::TokenCreated(changed)
  } else {
    Event::TokenRevoked(changed)
  });
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use crate::testing::{OUTSIDE, TRUSTED, TestApp, app, request, with};

  async fn create(app: &TestApp, body: serde_json::Value) -> serde_json::Value {
    let answer = app
      .send(request("POST", "/api/v0/tokens", Some(body)))
      .await;
    assert_eq!(answer.status, 201, "{:?}", answer.body);
    answer.json()
  }

  fn bearer(
    r: axum::http::Request<axum::body::Body>,
    secret: &str,
  ) -> axum::http::Request<axum::body::Body> {
    with(r, "authorization", &format!("Bearer {secret}"))
  }

  #[tokio::test]
  async fn a_token_is_created_used_listed_and_revoked() {
    let app = app(TRUSTED, &[]).await;
    let created = create(&app, json!({ "name": "backup", "role": "ReadOnly" })).await;
    let secret = created["secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with("slsk_") && secret.len() == 45);
    assert_eq!(created["token"]["prefix"], &secret[5..13]);
    assert_eq!(created["token"]["owner"], "network:127.0.0.0/8");
    let me = app
      .send_from(
        OUTSIDE,
        bearer(request("GET", "/api/v0/session", None), &secret),
      )
      .await;
    assert_eq!(me.status, 200);
    let me = me.json();
    let id = created["token"]["id"].as_str().unwrap();
    assert_eq!(me["subject"], format!("token:{id}"));
    assert_eq!(me["via"], json!({ "kind": "apiToken", "id": id }));
    assert_eq!(me["session"], json!(null));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let listed = app
      .send(request("GET", "/api/v0/tokens", None))
      .await
      .json();
    assert_eq!(listed[0]["source"], "api");
    assert_eq!(listed[0]["lastUsedIp"], "192.0.2.1");
    let renamed = app
      .send(request(
        "PUT",
        &format!("/api/v0/tokens/{id}"),
        Some(json!({ "name": "nightly" })),
      ))
      .await;
    assert_eq!(renamed.json()["name"], "nightly");
    let beyond = app
      .send(request(
        "PUT",
        &format!("/api/v0/tokens/{id}"),
        Some(json!({ "expiresAt": "+10000-01-01T00:00:00Z" })),
      ))
      .await;
    assert_eq!(beyond.status, 400, "an expiry past what is stored in order");
    let revoked = app
      .send(request("DELETE", &format!("/api/v0/tokens/{id}"), None))
      .await;
    assert_eq!(revoked.status, 204);
    let refused = app
      .send_from(
        OUTSIDE,
        bearer(request("GET", "/api/v0/session", None), &secret),
      )
      .await;
    assert_eq!(refused.status, 401);
    assert_eq!(refused.json()["error"], "token_revoked");
    let listed = app
      .send(request("GET", "/api/v0/tokens", None))
      .await
      .json();
    assert!(
      listed
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["source"] == "config")
    );
  }

  #[tokio::test]
  async fn a_token_can_not_mint_tokens_nor_exceed_its_creators_role() {
    let app = app(TRUSTED, &[]).await;
    let admin = create(&app, json!({ "name": "admin", "role": "Administrator" })).await;
    let secret = admin["secret"].as_str().unwrap();
    let minted = app
      .send_from(
        OUTSIDE,
        bearer(
          request(
            "POST",
            "/api/v0/tokens",
            Some(json!({ "name": "x", "role": "ReadOnly" })),
          ),
          secret,
        ),
      )
      .await;
    assert_eq!(minted.status, 403, "a token is no login session");
    let id = admin["token"]["id"].as_str().unwrap();
    let extended = app
      .send_from(
        OUTSIDE,
        bearer(
          request(
            "PUT",
            &format!("/api/v0/tokens/{id}"),
            Some(json!({ "expiresAt": "2099-01-01T00:00:00Z" })),
          ),
          secret,
        ),
      )
      .await;
    assert_eq!(extended.status, 403, "nor can it put off its own expiry");
    for (body, status) in [
      (json!({ "name": "", "role": "ReadOnly" }), 400),
      (json!({ "name": "x".repeat(65), "role": "ReadOnly" }), 400),
      (
        json!({ "name": "x", "role": "ReadOnly", "expiresIn": "1d", "expiresAt": "2099-01-01T00:00:00Z" }),
        400,
      ),
      (
        json!({ "name": "x", "role": "ReadOnly", "expiresIn": "soon" }),
        400,
      ),
      (
        json!({ "name": "x", "role": "ReadOnly", "expiresAt": "2000-01-01T00:00:00Z" }),
        400,
      ),
      (
        json!({ "name": "x", "role": "ReadOnly", "expiresAt": "+10000-01-01T00:00:00Z" }),
        400,
      ),
      (
        json!({ "name": "x", "role": "ReadOnly", "cidr": "10.0.0.0/33" }),
        400,
      ),
    ] {
      let answer = app
        .send(request("POST", "/api/v0/tokens", Some(body.clone())))
        .await;
      assert_eq!(answer.status, status, "{body}");
    }
  }

  #[tokio::test]
  async fn a_token_is_held_to_its_role_networks_and_expiry() {
    let app = app(
      &format!("{TRUSTED}[web.authentication.local.users.viewer]\npassword = \"look\"\nrole = \"readwrite\"\n"),
      &[],
    )
    .await;
    let login = app
      .send_from(
        OUTSIDE,
        request(
          "POST",
          "/api/v0/session",
          Some(json!({ "username": "viewer", "password": "look" })),
        ),
      )
      .await;
    let cookie = crate::testing::set_cookie(&login.headers, "reseek_session").unwrap();
    let above = app
      .send_from(
        OUTSIDE,
        with(
          with(
            request(
              "POST",
              "/api/v0/tokens",
              Some(json!({ "name": "x", "role": "Administrator" })),
            ),
            "cookie",
            &cookie,
          ),
          "origin",
          "http://localhost",
        ),
      )
      .await;
    assert_eq!(above.status, 403, "no role above the creator's own");
    let narrow = create(
      &app,
      json!({ "name": "lan", "role": "ReadOnly", "cidr": "10.0.0.0/8", "expiresIn": "1h" }),
    )
    .await;
    let secret = narrow["secret"].as_str().unwrap().to_string();
    let away = app
      .send_from(
        OUTSIDE,
        bearer(request("GET", "/api/v0/session", None), &secret),
      )
      .await;
    assert_eq!(away.status, 401);
    let home = app
      .send_from(
        "10.1.2.3:1",
        bearer(request("GET", "/api/v0/session", None), &secret),
      )
      .await;
    assert_eq!(home.status, 200);
    let options = app
      .send_from(
        "10.1.2.3:1",
        bearer(request("GET", "/api/v0/options/view", None), &secret),
      )
      .await;
    assert_eq!(options.status, 403, "a read-only token is no administrator");
    let id = narrow["token"]["id"].as_str().unwrap().to_string();
    app
      .state
      .auth
      .db
      .write(move |conn| {
        conn.execute(
          "UPDATE api_tokens SET expires_at = '2000-01-01T00:00:00.000000Z' WHERE id = ?1",
          [id],
        )
      })
      .await
      .unwrap();
    let expired = app
      .send_from(
        "10.1.2.3:1",
        bearer(request("GET", "/api/v0/session", None), &secret),
      )
      .await;
    assert_eq!(expired.status, 401);
    assert_eq!(expired.json()["error"], "token_expired");
  }

  #[tokio::test]
  async fn a_networks_token_follows_the_configuration_and_a_header_only_proxy_user_gets_none() {
    let config = |networks: &str, trusted_role: &str| {
      format!(
        "[web]\ntrusted_proxies = [\"192.0.2.7/32\"]\n[web.authentication]\n\
         trusted_networks = [{networks}]\ntrusted_role = \"{trusted_role}\"\n\
         [web.authentication.proxy]\nenabled = true\n\
         [web.authentication.proxy.role_mapping]\nadministrator = [\"admins\"]\n"
      )
    };
    let app = app(&config("\"127.0.0.0/8\"", "administrator"), &[]).await;
    let reload = async |text: String| {
      std::fs::write(app.dir.path().join("config.toml"), text).unwrap();
      app.state.config.reload().await.unwrap();
    };
    let by_network = create(&app, json!({ "name": "lan", "role": "Administrator" })).await;
    let by_network = by_network["secret"].as_str().unwrap().to_string();
    // a proxy user who never had a page session, so no groups of theirs are stored to read a
    // token's role from later
    let by_proxy = app
      .send_from(
        "192.0.2.7:1",
        with(
          with(
            request(
              "POST",
              "/api/v0/tokens",
              Some(json!({ "name": "proxied", "role": "ReadOnly" })),
            ),
            "x-authentik-username",
            "alice",
          ),
          "x-authentik-groups",
          "admins",
        ),
      )
      .await;
    assert_eq!(by_proxy.status, 403, "{:?}", by_proxy.body);
    assert_eq!(
      by_proxy.json()["error"],
      "Your role cannot be read again on later requests, so a token of yours would not work"
    );
    let me = async |secret: &str| {
      app
        .send_from(
          OUTSIDE,
          bearer(request("GET", "/api/v0/session", None), secret),
        )
        .await
    };
    assert_eq!(me(&by_network).await.json()["role"], "Administrator");

    reload(config("\"127.0.0.0/8\"", "readonly")).await;
    assert_eq!(me(&by_network).await.json()["role"], "ReadOnly");
    reload(config("\"10.0.0.0/8\"", "administrator")).await;
    let refused = me(&by_network).await;
    assert_eq!(refused.status, 401, "the network is trusted no more");
    assert_eq!(refused.json()["error"], "token_owner_without_role");
  }
}
