//! OpenID Connect providers: their discovery documents and signing keys, read over HTTPS and
//! kept for a day, with the keys read again when a token names one not seen yet.
//!
//! The login itself is [`flow`]; the provider ending sessions, and sending the browser to the
//! provider's own logout page, are [`logout`].

pub mod flow;
pub mod logout;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use openidconnect::core::{
  CoreAuthDisplay, CoreAuthPrompt, CoreErrorResponseType, CoreGenderClaim, CoreJsonWebKey,
  CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreRevocableToken,
  CoreRevocationErrorResponse, CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::{
  AdditionalClaims, ClientId, ClientSecret, EmptyExtraTokenFields, EndpointMaybeSet,
  EndpointNotSet, EndpointSet, IdToken, IdTokenFields, IssuerUrl, JsonWebKeySet,
  ProviderMetadataWithLogout, RedirectUrl, StandardErrorResponse, StandardTokenResponse,
};
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use tracing::{Instrument, debug, error, info, info_span, warn};

use crate::app::AppState;
use crate::auth::Client;
use crate::config::{Oidc, Options};

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
const RETRY: Duration = Duration::from_secs(60);

/// How far a provider's clock may be from ours when its tokens' times are checked.
pub const LEEWAY: chrono::Duration = chrono::Duration::minutes(5);

/// How long a call to a provider may take.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The providers in use, each kept while its configuration stays the same.
#[derive(Default)]
pub struct Providers {
  known: Mutex<HashMap<String, Arc<Provider>>>,
}

/// One provider as configured, with what was read from it.
pub struct Provider {
  /// its name in the configuration
  pub name: String,
  /// its options
  pub options: Oidc,
  http: openidconnect::reqwest::Client,
  discovery: tokio::sync::Mutex<Discovery>,
}

#[derive(Default)]
struct Discovery {
  metadata: Option<(Metadata, Instant)>,
  failed_at: Option<Instant>,
  keys_at: Option<Instant>,
}

impl Providers {
  /// The provider `name` as `options` configure it now, `None` when there is none by that
  /// name. A changed configuration makes a new provider, which discovers again.
  pub fn get(&self, name: &str, options: &Options) -> Result<Option<Arc<Provider>>, String> {
    let Some(configured) = options.web.authentication.oidc.get(name) else {
      return Ok(None);
    };
    let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(provider) = known.get(name)
      && provider.options == *configured
    {
      return Ok(Some(provider.clone()));
    }
    let provider = Arc::new(Provider::new(name, configured.clone())?);
    known.insert(name.to_string(), provider.clone());
    Ok(Some(provider))
  }
}

impl Provider {
  fn new(name: &str, options: Oidc) -> Result<Self, String> {
    Ok(Self {
      name: name.to_string(),
      http: http_client(options.ca_certificate.as_deref())?,
      options,
      discovery: tokio::sync::Mutex::new(Discovery::default()),
    })
  }

  /// The discovery document, read again when it is a day old. After a failed read the
  /// provider is unavailable for a minute before the next try.
  pub async fn metadata(&self) -> Result<Metadata, String> {
    let mut discovery = self.discovery.lock().await;
    if let Some((metadata, at)) = &discovery.metadata
      && at.elapsed() < DISCOVERY_AGE
    {
      return Ok(metadata.clone());
    }
    if discovery.failed_at.is_some_and(|at| at.elapsed() < RETRY) {
      return Err("the identity provider could not be reached a moment ago".to_string());
    }
    match self.discover().await {
      Ok(metadata) => {
        info!(provider = self.name, "OpenID Connect provider discovered");
        discovery.metadata = Some((metadata.clone(), Instant::now()));
        discovery.keys_at = Some(Instant::now());
        discovery.failed_at = None;
        Ok(metadata)
      }
      Err(e) => {
        discovery.failed_at = Some(Instant::now());
        // a provider read before keeps its old document until the new one can be read
        match &discovery.metadata {
          Some((metadata, _)) => {
            warn!(
              provider = self.name,
              error = e,
              "OpenID Connect discovery failed; using the last document"
            );
            Ok(metadata.clone())
          }
          None => Err(e),
        }
      }
    }
  }

  /// Reads the signing keys again, at most once a minute; the new document when it did.
  pub async fn refresh_keys(&self) -> Option<Metadata> {
    let mut discovery = self.discovery.lock().await;
    if discovery.keys_at.is_some_and(|at| at.elapsed() < RETRY) {
      return None;
    }
    discovery.keys_at = Some(Instant::now());
    let (metadata, at) = discovery.metadata.clone()?;
    match JsonWebKeySet::<CoreJsonWebKey>::fetch_async(metadata.jwks_uri(), &self.http).await {
      Ok(keys) => {
        debug!(
          provider = self.name,
          count = keys.keys().len(),
          "provider signing keys read again"
        );
        let metadata = metadata.set_jwks(keys);
        discovery.metadata = Some((metadata.clone(), at));
        Some(metadata)
      }
      Err(e) => {
        warn!(provider = self.name, error = %e, "provider signing keys not read");
        None
      }
    }
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
  pub fn http(&self) -> &openidconnect::reqwest::Client {
    &self.http
  }
}

/// An HTTP client that trusts the usual certificate authorities and the one in `ca`, does not
/// follow redirects, and gives up after [`TIMEOUT`].
fn http_client(ca: Option<&Path>) -> Result<openidconnect::reqwest::Client, String> {
  let mut roots = rustls::RootCertStore {
    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
  };
  if let Some(path) = ca {
    let certificates = CertificateDer::pem_file_iter(path)
      .and_then(|certificates| certificates.collect::<Result<Vec<_>, _>>())
      .map_err(|e| format!("{}: {e}", path.display()))?;
    for certificate in certificates {
      roots
        .add(certificate)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    }
  }
  let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
    rustls::crypto::aws_lc_rs::default_provider(),
  ))
  .with_safe_default_protocol_versions()
  .map_err(|e| e.to_string())?
  .with_root_certificates(roots)
  .with_no_client_auth();
  openidconnect::reqwest::Client::builder()
    .use_preconfigured_tls(tls)
    .redirect(openidconnect::reqwest::redirect::Policy::none())
    .timeout(TIMEOUT)
    .build()
    .map_err(|e| e.to_string())
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

/// An error with the errors that caused it, joined by `: `.
pub fn error_chain(error: &dyn std::error::Error) -> String {
  let mut text = error.to_string();
  let mut source = error.source();
  while let Some(cause) = source {
    text.push_str(": ");
    text.push_str(&cause.to_string());
    source = cause.source();
  }
  text
}

/// Discovers every configured provider once at startup. A provider that can not be reached
/// stays unavailable until a login tries it again; the daemon runs on.
pub fn discover_at_start(app: &AppState) {
  let current = app.config.current();
  for name in current.values.web.authentication.oidc.keys() {
    let provider = match app.auth.oidc.get(name, &current.values) {
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
