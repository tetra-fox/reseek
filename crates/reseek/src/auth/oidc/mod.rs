//! OpenID Connect providers: their discovery documents and signing keys, read over HTTPS and
//! kept for a day, with the keys read again when a token names one not seen yet.
//!
//! The login itself is [`flow`]; the provider ending sessions, and sending the browser to the
//! provider's own logout page, are [`logout`].

pub mod flow;
pub mod logout;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cascade::Watcher;
use openidconnect::core::{
  CoreAuthDisplay, CoreAuthPrompt, CoreErrorResponseType, CoreGenderClaim, CoreJsonWebKey,
  CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreRevocableToken,
  CoreRevocationErrorResponse, CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::{
  AdditionalClaims, AsyncHttpClient, ClientId, ClientSecret, EmptyExtraTokenFields,
  EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpClientError, HttpRequest, HttpResponse,
  IdToken, IdTokenFields, IssuerUrl, JsonWebKeySet, ProviderMetadataWithLogout, RedirectUrl,
  StandardErrorResponse, StandardTokenResponse,
};
use reseek_shares::lock;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::app::AppState;
use crate::auth::Client;
use crate::config::{Oidc, Options};
use crate::http::outbound::error_chain;

/// A provider's discovery document with its signing keys.
pub type Metadata = ProviderMetadataWithLogout;

/// The claims of an ID token or a userinfo answer besides the standard ones, kept whole: the
/// groups, the provider's session id and any claim the configuration names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtraClaims {
  /// every such claim by name
  #[serde(flatten)]
  pub claims: HashMap<String, serde_json::Value>,
}

impl AdditionalClaims for ExtraClaims {}

/// An ID token with every claim it carries.
pub type OidcIdToken =
  IdToken<ExtraClaims, CoreGenderClaim, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm>;

/// The token endpoint's answer, its ID token read with every claim.
type TokenAnswer = StandardTokenResponse<
  IdTokenFields<
    ExtraClaims,
    EmptyExtraTokenFields,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
  >,
  CoreTokenType,
>;

/// A client of one provider, with every endpoint discovery can name.
pub type OidcClient = openidconnect::Client<
  ExtraClaims,
  CoreAuthDisplay,
  CoreGenderClaim,
  CoreJweContentEncryptionAlgorithm,
  CoreJsonWebKey,
  CoreAuthPrompt,
  StandardErrorResponse<CoreErrorResponseType>,
  TokenAnswer,
  CoreTokenIntrospectionResponse,
  CoreRevocableToken,
  CoreRevocationErrorResponse,
  EndpointSet,
  EndpointNotSet,
  EndpointNotSet,
  EndpointNotSet,
  EndpointMaybeSet,
  EndpointMaybeSet,
>;

/// How long a discovery document is used before it is read again.
const DISCOVERY_AGE: Duration = Duration::from_secs(24 * 3600);

/// The least time between two reads of a provider's keys, and between discovery attempts
/// after one failed.
pub(crate) const RETRY: Duration = Duration::from_secs(60);

/// How far a provider's clock may be from ours when its tokens' times are checked.
pub const LEEWAY: chrono::Duration = chrono::Duration::minutes(5);

/// The providers in use, each kept while its configuration stays the same.
#[derive(Default)]
pub struct Providers {
  known: Mutex<HashMap<String, Arc<Provider>>>,
  /// held shared by each login from before it asks for its tokens until its session is stored,
  /// and alone by a back-channel logout while it finds and ends sessions, so a logout waits for
  /// the session a login of the same provider session is making
  sign_ins: tokio::sync::RwLock<()>,
}

/// One provider as configured, with what was read from it.
pub struct Provider {
  /// its name in the configuration
  pub name: String,
  /// its options
  pub options: Oidc,
  http: ProviderHttp,
  discovery: tokio::sync::Mutex<Discovery>,
}

/// The provider's discovery document as read so far, and the last read of its keys.
struct Discovery {
  document: Document,
  keys: Option<KeysRead>,
}

/// A read of the provider's signing keys: when it began, and whether it got them.
#[derive(Clone, Copy)]
struct KeysRead {
  at: Instant,
  got: bool,
}

/// What reading a provider's signing keys again gave.
pub enum Keys {
  /// read now, in this document
  Read(Box<Metadata>),
  /// read less than a minute ago, so the keys in use are the provider's
  Current,
  /// not read, now or at the try less than a minute ago, so a token's key may be one of them
  Unread,
}

/// Where reading the discovery document stands.
enum Document {
  /// never read; after a failed read, `failed_at` holds back the next try
  Unread { failed_at: Option<Instant> },
  /// read at `read_at`; once it is old and reading it again failed, `failed_at` holds back the
  /// next try and the old document stays in use meanwhile
  Read {
    metadata: Box<Metadata>,
    read_at: Instant,
    failed_at: Option<Instant>,
  },
}

/// The provider `name` among `known` when it was made for `options`.
fn made_for(
  known: &HashMap<String, Arc<Provider>>,
  name: &str,
  options: &Oidc,
) -> Option<Arc<Provider>> {
  known
    .get(name)
    .filter(|provider| provider.options == *options)
    .cloned()
}

/// Whether `failed_at` is a failure recent enough to hold back the next try.
fn holds_back(failed_at: Option<Instant>) -> bool {
  failed_at.is_some_and(|at| at.elapsed() < RETRY)
}

impl Providers {
  /// The provider `name` as `config` gives it now, `None` when there is none by that name. A
  /// changed configuration makes a new provider, which discovers again.
  pub async fn get(
    &self,
    name: &str,
    config: &Watcher<Options>,
  ) -> Result<Option<Arc<Provider>>, String> {
    let current = config.current();
    let Some(configured) = current.values.web.authentication.oidc.get(name) else {
      return Ok(None);
    };
    if let Some(provider) = made_for(&lock(&self.known), name, configured) {
      return Ok(Some(provider));
    }
    let made = Arc::new(Provider::new(name, configured.clone()).await?);
    Ok(Some(self.keep(made, config)))
  }

  /// `made`, or one made meanwhile for the same options, so its discovery is shared. `made` is
  /// kept for later calls only while its options are still the configured ones, so one made for
  /// options replaced meanwhile never takes the place of a newer one.
  fn keep(&self, made: Arc<Provider>, config: &Watcher<Options>) -> Arc<Provider> {
    let mut known = lock(&self.known);
    if let Some(provider) = made_for(&known, &made.name, &made.options) {
      return provider;
    }
    // read under the lock, so a provider kept after this one is checked against these
    // options or newer ones
    let current = config.current();
    if current.values.web.authentication.oidc.get(&made.name) == Some(&made.options) {
      known.insert(made.name.clone(), made.clone());
    }
    made
  }

  /// Held by a login while it asks for its tokens and stores its session.
  pub async fn signing_in(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
    self.sign_ins.read().await
  }

  /// Held by a back-channel logout while it finds and ends sessions; waits for the logins under
  /// way.
  pub async fn logging_out(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
    self.sign_ins.write().await
  }
}

impl Provider {
  async fn new(name: &str, options: Oidc) -> Result<Self, String> {
    Ok(Self {
      name: name.to_string(),
      http: ProviderHttp(crate::http::outbound::client(options.ca_certificate.as_deref()).await?),
      options,
      discovery: tokio::sync::Mutex::new(Discovery {
        document: Document::Unread { failed_at: None },
        keys: None,
      }),
    })
  }

  /// The discovery document, read again when it is a day old. After a failed read, the
  /// provider is not tried again for a minute: until then the old document is used when there
  /// is one, and the provider is unavailable when there is none.
  pub async fn metadata(&self) -> Result<Metadata, String> {
    let mut discovery = self.discovery.lock().await;
    match &discovery.document {
      Document::Read {
        metadata, read_at, ..
      } if read_at.elapsed() < DISCOVERY_AGE => return Ok(Metadata::clone(metadata)),
      Document::Read {
        metadata,
        failed_at,
        ..
      } if holds_back(*failed_at) => return Ok(Metadata::clone(metadata)),
      Document::Unread { failed_at } if holds_back(*failed_at) => {
        return Err("the identity provider could not be reached a moment ago".to_string());
      }
      Document::Read { .. } | Document::Unread { .. } => {}
    }
    match self.discover().await {
      Ok(metadata) => {
        info!(provider = self.name, "OpenID Connect provider discovered");
        discovery.document = Document::Read {
          metadata: Box::new(metadata.clone()),
          read_at: Instant::now(),
          failed_at: None,
        };
        discovery.keys = Some(KeysRead {
          at: Instant::now(),
          got: true,
        });
        Ok(metadata)
      }
      Err(e) => match &mut discovery.document {
        // a provider read before keeps its old document until the new one can be read
        Document::Read {
          metadata,
          failed_at,
          ..
        } => {
          warn!(
            provider = self.name,
            error = e,
            "OpenID Connect discovery failed; using the last document"
          );
          *failed_at = Some(Instant::now());
          Ok(Metadata::clone(metadata))
        }
        Document::Unread { failed_at } => {
          *failed_at = Some(Instant::now());
          Err(e)
        }
      },
    }
  }

  /// Reads the signing keys again, at most once a minute.
  pub async fn refresh_keys(&self) -> Keys {
    let mut discovery = self.discovery.lock().await;
    if let Some(last) = discovery.keys.filter(|last| last.at.elapsed() < RETRY) {
      return if last.got {
        Keys::Current
      } else {
        Keys::Unread
      };
    }
    let at = Instant::now();
    let Document::Read { metadata, .. } = &mut discovery.document else {
      return Keys::Unread;
    };
    let keys =
      match JsonWebKeySet::<CoreJsonWebKey>::fetch_async(metadata.jwks_uri(), &self.http).await {
        Ok(keys) => {
          debug!(
            provider = self.name,
            count = keys.keys().len(),
            "provider signing keys read again"
          );
          **metadata = Metadata::clone(metadata).set_jwks(keys);
          Keys::Read(metadata.clone())
        }
        Err(e) => {
          warn!(provider = self.name, error = %e, "provider signing keys not read");
          Keys::Unread
        }
      };
    discovery.keys = Some(KeysRead {
      at,
      got: matches!(keys, Keys::Read(_)),
    });
    keys
  }

  /// Reads the discovery document and the signing keys now, without keeping them.
  pub async fn discover(&self) -> Result<Metadata, String> {
    let issuer = self
      .options
      .issuer
      .clone()
      .ok_or_else(|| "the provider has no issuer".to_string())?;
    let issuer = IssuerUrl::new(issuer).map_err(|e| e.to_string())?;
    Metadata::discover_async(issuer, &self.http)
      .await
      .map_err(|e| error_chain(&e))
  }

  /// A client of the provider described by `metadata`, sending the browser back to
  /// `redirect`.
  pub fn client(&self, metadata: Metadata, redirect: RedirectUrl) -> OidcClient {
    let client_id = ClientId::new(self.options.client_id.clone().unwrap_or_default());
    let secret = self.options.client_secret.clone().map(ClientSecret::new);
    let basic = metadata
      .token_endpoint_auth_methods_supported()
      .is_none_or(|methods| {
        methods.contains(&openidconnect::core::CoreClientAuthMethod::ClientSecretBasic)
      });
    let client =
      OidcClient::from_provider_metadata(metadata, client_id, secret).set_redirect_uri(redirect);
    if basic {
      client
    } else {
      client.set_auth_type(openidconnect::AuthType::RequestBody)
    }
  }

  /// The HTTP client calls to the provider go through.
  pub fn http(&self) -> &ProviderHttp {
    &self.http
  }
}

/// The daemon's outbound client in the form the OpenID Connect library sends requests with.
pub struct ProviderHttp(reqwest::Client);

impl<'c> AsyncHttpClient<'c> for ProviderHttp {
  type Error = HttpClientError<reqwest::Error>;
  type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + 'c>>;

  fn call(&'c self, request: HttpRequest) -> Self::Future {
    Box::pin(async move {
      let response = self
        .0
        .execute(request.try_into().map_err(Box::new)?)
        .await
        .map_err(Box::new)?;
      let mut answer = openidconnect::http::Response::builder()
        .status(response.status())
        .version(response.version());
      for (name, value) in response.headers() {
        answer = answer.header(name, value);
      }
      let body = response.bytes().await.map_err(Box::new)?;
      answer.body(body.to_vec()).map_err(HttpClientError::Http)
    })
  }
}

/// The public base URL of the daemon: `web.external_url` when set, else the scheme and host
/// the client used with the URL base; `None` when the request names no host.
pub fn external_base(url_base: &str, options: &Options, client: &Client) -> Option<String> {
  if let Some(url) = &options.web.external_url {
    return Some(url.trim_end_matches('/').to_string());
  }
  let scheme = if client.https { "https" } else { "http" };
  let authority = client.authority.as_deref()?;
  Some(format!("{scheme}://{authority}{url_base}"))
}

/// Discovers every configured provider once at startup. A provider that cannot be reached
/// stays unavailable until a login tries it again; the daemon runs on.
pub async fn discover_at_start(app: AppState) {
  let current = app.config.current();
  for name in current.values.web.authentication.oidc.keys() {
    let provider = match app.auth.oidc.get(name, &app.config).await {
      Ok(Some(provider)) => provider,
      Ok(None) => continue,
      Err(e) => {
        error!(
          provider = name,
          error = e,
          "OpenID Connect provider not set up; it is unavailable"
        );
        continue;
      }
    };
    let span = info_span!("oidc", provider = name.as_str());
    tokio::spawn(
      async move {
        if let Err(e) = provider.metadata().await {
          error!(
            error = e,
            "OpenID Connect discovery failed; the provider is unavailable"
          );
        }
      }
      .instrument(span),
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::issuer::Issuer;

  #[tokio::test]
  async fn metadata_stale_document_is_used_while_the_provider_is_down() {
    let issuer = Issuer::start().await;
    let options = Oidc {
      issuer: Some(issuer.url.clone()),
      ..Oidc::default()
    };
    let provider = Provider::new("test", options).await.unwrap();
    provider.metadata().await.unwrap();
    drop(issuer);
    tokio::time::pause();
    tokio::time::advance(DISCOVERY_AGE).await;
    // the read fails, and the day-old document is used
    provider.metadata().await.unwrap();
    // and goes on being used while the provider is not tried again
    provider.metadata().await.unwrap();
    tokio::time::advance(RETRY).await;
    provider.metadata().await.unwrap();
  }

  #[tokio::test]
  async fn a_provider_made_for_replaced_options_does_not_replace_a_newer_one() {
    let home = |issuer: &str| {
      format!("[web.authentication.oidc.home]\nissuer = \"{issuer}\"\nclient_id = \"reseek\"\n")
    };
    let app = crate::testing::app(&home("http://127.0.0.1:9/old"), &[]).await;
    let config = &app.state.config;
    let providers = Providers::default();
    // made for the first options, and finishing once newer ones are in use
    let first = config.current().values.web.authentication.oidc["home"].clone();
    let stale = Arc::new(Provider::new("home", first).await.unwrap());
    let path = app.dir.path().join("config.toml");
    std::fs::write(path, home("http://127.0.0.1:9/new")).unwrap();
    config.reload().await.unwrap();
    let newer = providers.get("home", config).await.unwrap().unwrap();
    let kept = providers.keep(stale.clone(), config);
    assert!(
      Arc::ptr_eq(&kept, &stale),
      "its caller gets the one it made"
    );
    let again = providers.get("home", config).await.unwrap().unwrap();
    assert!(Arc::ptr_eq(&again, &newer));
  }
}
