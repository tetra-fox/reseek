//! The session calls: signing in and out, who the caller is, browser sessions, API tokens and
//! the OpenID Connect routes.

use api_types::auth::session::{
  BackchannelLogout, CurrentPrincipal, LoginRequest, LogoutResponse, OidcCallbackQuery,
  OidcLoginQuery, OidcTestResult, Providers, Session,
};
use api_types::auth::tokens::{CreateToken, CreatedToken, ListedToken, Token, UpdateToken};
use reqwest::header::LOCATION;
use uuid::Uuid;

use super::{Client, Error, Result, Route, done, json, send};

impl Client {
  /// `POST /session`: signs in with a local account; the session cookie is kept for every later
  /// call.
  pub async fn login(&self, username: &str, password: &str) -> Result<()> {
    let body = LoginRequest {
      username: username.to_string(),
      password: password.to_string(),
    };
    done(self.post(Route::new("session")).json(&body)).await
  }

  /// `GET /session`: who the caller is.
  pub async fn session(&self) -> Result<CurrentPrincipal> {
    json(self.get(Route::new("session"))).await
  }

  /// `GET /session/enabled`: whether the caller needs to sign in at all.
  pub async fn authentication_enabled(&self) -> Result<bool> {
    json(self.get(Route::new("session/enabled"))).await
  }

  /// `GET /session/providers`: the ways to sign in offered to the caller.
  pub async fn providers(&self) -> Result<Providers> {
    json(self.get(Route::new("session/providers"))).await
  }

  /// `POST /session/logout`: signs out, with where to go to sign out of the provider too.
  pub async fn logout(&self) -> Result<LogoutResponse> {
    json(self.post(Route::new("session/logout"))).await
  }

  /// `GET /sessions`: the browser sessions the caller may see.
  pub async fn sessions(&self) -> Result<Vec<Session>> {
    json(self.get(Route::new("sessions"))).await
  }

  /// `DELETE /sessions/{id}`: ends one browser session.
  pub async fn revoke_session(&self, id: &str) -> Result<()> {
    done(self.delete(Route::new("sessions").segment(id))).await
  }

  /// `DELETE /sessions`: ends every browser session but the caller's.
  pub async fn revoke_other_sessions(&self) -> Result<()> {
    done(self.delete(Route::new("sessions"))).await
  }

  /// `GET /tokens`: the API tokens the caller may see.
  pub async fn tokens(&self) -> Result<Vec<ListedToken>> {
    json(self.get(Route::new("tokens"))).await
  }

  /// `POST /tokens`: creates an API token; its secret is in the answer only.
  pub async fn create_token(&self, request: &CreateToken) -> Result<CreatedToken> {
    json(self.post(Route::new("tokens")).json(request)).await
  }

  /// `PUT /tokens/{id}`: renames a token or changes its expiry.
  pub async fn update_token(&self, id: Uuid, request: &UpdateToken) -> Result<Token> {
    json(
      self
        .put(Route::new("tokens").segment(&id.to_string()))
        .json(request),
    )
    .await
  }

  /// `DELETE /tokens/{id}`: revokes a token.
  pub async fn revoke_token(&self, id: Uuid) -> Result<()> {
    done(self.delete(Route::new("tokens").segment(&id.to_string()))).await
  }

  /// `GET /auth/oidc/{name}/login`: where a browser is sent to sign in with a provider.
  pub async fn oidc_login(&self, name: &str, query: &OidcLoginQuery) -> Result<Option<String>> {
    let route = Route::new("auth/oidc")
      .segment(name)
      .then("login")
      .query(query)?;
    redirect(self, route).await
  }

  /// `GET /auth/oidc/{name}/callback`: where the provider sends the browser back to, and
  /// where the browser goes next.
  pub async fn oidc_callback(
    &self,
    name: &str,
    query: &OidcCallbackQuery,
  ) -> Result<Option<String>> {
    let route = Route::new("auth/oidc")
      .segment(name)
      .then("callback")
      .query(query)?;
    redirect(self, route).await
  }

  /// `POST /auth/oidc/{name}/backchannel`: a provider ends the sessions a logout token names.
  pub async fn oidc_backchannel(&self, name: &str, logout_token: &str) -> Result<()> {
    let form = BackchannelLogout {
      logout_token: logout_token.to_string(),
    };
    let body = serde_html_form::to_string(&form)?;
    let route = Route::new("auth/oidc").segment(name).then("backchannel");
    let request = self
      .post(route)
      .header("content-type", "application/x-www-form-urlencoded")
      .body(body);
    done(request).await
  }

  /// `POST /auth/oidc/{name}/test`: reads a provider's discovery document now.
  pub async fn test_oidc(&self, name: &str) -> Result<OidcTestResult> {
    json(self.post(Route::new("auth/oidc").segment(name).then("test"))).await
  }
}

/// The `Location` an answer sends the browser to, when it sends it anywhere.
async fn redirect(client: &Client, route: Route) -> Result<Option<String>> {
  let response = send(client.get(route)).await?;
  let Some(location) = response.headers().get(LOCATION) else {
    return Ok(None);
  };
  match location.to_str() {
    Ok(location) => Ok(Some(location.to_string())),
    Err(_) => Err(Error::Unexpected {
      status: response.status(),
      text: "the Location header is not text".to_string(),
    }),
  }
}
