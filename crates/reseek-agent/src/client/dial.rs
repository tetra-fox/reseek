//! Reaching the controller: the settings of the connection, the socket an agent opens with its
//! API key and the relay protocol, and the HTTP client its uploads and fetches use. Both trust
//! the certificate authorities built in and the system's, or any certificate when the options
//! say so.

use std::sync::Arc;
use std::time::Duration;

use api_types::relay::RELAY_SUBPROTOCOL;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderMap, HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use url::Url;

use reseek_relay_proto::credential::Key;
use reseek_relay_proto::frames::MAX_FRAME;

use crate::options::Options;

/// How long dialing the controller may take, TLS and the upgrade included.
const DIAL: Duration = Duration::from_secs(10);

/// A connection to the controller that is plain or TLS.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// A relay socket to a controller.
pub type Socket = WebSocketStream<Box<dyn Io>>;

/// What the agent connects to its controller with, read from the options when it dials.
pub struct Settings {
  /// the controller's base URL, without a trailing `/`
  pub address: Url,
  /// the API key every call carries
  pub api_key: String,
  /// this agent's instance name
  pub agent: String,
  /// the key the agent's credentials are made with
  pub key: Key,
  /// the TLS settings of the socket and the HTTP calls
  tls: Arc<rustls::ClientConfig>,
  /// the client of the HTTP calls
  pub http: reqwest::Client,
}

impl Settings {
  /// The settings in `options`, which were validated when they were loaded, for the agent
  /// named `agent`.
  pub fn new(options: &Options, agent: &str) -> Result<Self, String> {
    // an agent has these three, which loading the options checked
    let controller = &options.relay.controller;
    let text = controller.address.as_deref().unwrap_or_default();
    let address =
      Url::parse(text.trim_end_matches('/')).map_err(|e| format!("controller address: {e}"))?;
    let tls = Arc::new(tls(controller.ignore_certificate_errors)?);
    let http = reqwest::Client::builder()
      .tls_backend_preconfigured((*tls).clone())
      .redirect(reqwest::redirect::Policy::none())
      .connect_timeout(DIAL)
      .build()
      .map_err(|e| e.to_string())?;
    Ok(Self {
      address,
      api_key: controller.api_key.clone().unwrap_or_default(),
      agent: agent.to_string(),
      key: Key::derive(controller.secret.as_deref().unwrap_or_default(), agent),
      tls,
      http,
    })
  }

  /// The URL of the API route `path` on the controller.
  pub fn url(&self, path: &str) -> String {
    format!(
      "{}/api/v0/{path}",
      self.address.as_str().trim_end_matches('/')
    )
  }

  /// Opens the relay socket.
  pub async fn dial(&self) -> Result<Socket, DialError> {
    tokio::time::timeout(DIAL, self.open())
      .await
      .map_err(|_| "the controller did not answer in time".to_string())?
  }

  async fn open(&self) -> Result<Socket, DialError> {
    let url = Url::parse(&self.url("relay/ws")).map_err(|e| e.to_string())?;
    let mut headers = HeaderMap::new();
    headers.insert(
      "x-api-key",
      HeaderValue::from_str(&self.api_key).map_err(|e| e.to_string())?,
    );
    open_socket(&url, &self.tls, headers).await
  }
}

/// What failed in an HTTP call to the controller, with its causes, and without the call's URL,
/// which holds the token of a file, download or share call.
pub(super) fn call_error(e: reqwest::Error) -> String {
  let e = e.without_url();
  let mut text = e.to_string();
  let mut cause = std::error::Error::source(&e);
  while let Some(inner) = cause {
    text.push_str(": ");
    text.push_str(&inner.to_string());
    cause = inner.source();
  }
  text
}

/// `url` without the user name and password it may hold, to be logged or shown.
pub(super) fn without_userinfo(url: &Url) -> Url {
  let mut shown = url.clone();
  // a URL that cannot take a user name or password has none
  let _ = shown.set_username("");
  let _ = shown.set_password(None);
  shown
}

/// Why the relay socket did not open.
#[derive(Debug, thiserror::Error)]
pub enum DialError {
  /// the controller answered 401 or 403: it does not take the API key, or not with the role
  /// an agent needs, which dialing again does not change
  #[error("{0}")]
  Refused(String),
  /// anything else, which may pass
  #[error("{0}")]
  Failed(String),
}

impl From<String> for DialError {
  fn from(e: String) -> Self {
    Self::Failed(e)
  }
}

/// Opens a relay socket to `url`, the `http` or `https` URL of the controller's relay route,
/// over TLS with `tls` for `https`, sending `headers` with the upgrade. The controller must
/// choose the relay protocol.
pub async fn open_socket(
  url: &Url,
  tls: &Arc<rustls::ClientConfig>,
  headers: HeaderMap,
) -> Result<Socket, DialError> {
  let host = url
    .host_str()
    .ok_or_else(|| "the controller address has no host".to_string())?
    .trim_start_matches('[')
    .trim_end_matches(']')
    .to_string();
  let port = url
    .port_or_known_default()
    .ok_or_else(|| "the controller address has no port".to_string())?;
  let tcp = TcpStream::connect((host.as_str(), port))
    .await
    .map_err(|e| format!("{host}:{port}: {e}"))?;
  let secure = url.scheme() == "https";
  let stream: Box<dyn Io> = if secure {
    let name = ServerName::try_from(host).map_err(|e| e.to_string())?;
    let tls = tokio_rustls::TlsConnector::from(tls.clone())
      .connect(name, tcp)
      .await
      .map_err(|e| format!("TLS: {e}"))?;
    Box::new(tls)
  } else {
    Box::new(tcp)
  };
  let mut socket = url.clone();
  socket
    .set_scheme(if secure { "wss" } else { "ws" })
    .map_err(|()| format!("{} is not an http or https address", without_userinfo(url)))?;
  let mut request = socket
    .as_str()
    .into_client_request()
    .map_err(|e| e.to_string())?;
  let sent = request.headers_mut();
  sent.extend(headers);
  sent.insert(
    "sec-websocket-protocol",
    HeaderValue::from_static(RELAY_SUBPROTOCOL),
  );
  let config = WebSocketConfig::default()
    .max_message_size(Some(2 * MAX_FRAME))
    .max_frame_size(Some(2 * MAX_FRAME));
  let (socket, response) =
    match tokio_tungstenite::client_async_with_config(request, stream, Some(config)).await {
      Ok(opened) => opened,
      Err(tungstenite::Error::Http(answer))
        if matches!(
          answer.status(),
          StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) =>
      {
        let (parts, body) = answer.into_parts();
        let body = body.unwrap_or_default();
        return Err(DialError::Refused(format!(
          "the controller answered {} {}",
          parts.status,
          String::from_utf8_lossy(&body)
        )));
      }
      Err(e) => return Err(e.to_string().into()),
    };
  let chosen = response
    .headers()
    .get("sec-websocket-protocol")
    .and_then(|value| value.to_str().ok());
  if chosen != Some(RELAY_SUBPROTOCOL) {
    return Err(format!("the controller chose the protocol {chosen:?}").into());
  }
  Ok(socket)
}

/// The TLS settings: the built-in and the system's certificate authorities, or any certificate
/// with `ignore_certificates`.
fn tls(ignore_certificates: bool) -> Result<rustls::ClientConfig, String> {
  if !ignore_certificates {
    return reseek_tls::client_config(reseek_tls::roots()).map_err(|e| e.to_string());
  }
  let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
  let config = rustls::ClientConfig::builder_with_provider(provider.clone())
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
    .with_no_client_auth();
  Ok(config)
}

/// Takes the controller's certificate whatever it is, for a controller with a self-signed one
/// when the operator asked for that. The handshake's signatures are still checked, so the
/// connection is encrypted to whoever holds the certificate's key.
#[derive(Debug)]
struct AnyCertificate(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
  fn verify_server_cert(
    &self,
    _: &CertificateDer<'_>,
    _: &[CertificateDer<'_>],
    _: &ServerName<'_>,
    _: &[u8],
    _: UnixTime,
  ) -> Result<ServerCertVerified, rustls::Error> {
    Ok(ServerCertVerified::assertion())
  }

  fn verify_tls12_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    rustls::crypto::verify_tls12_signature(
      message,
      cert,
      dss,
      &self.0.signature_verification_algorithms,
    )
  }

  fn verify_tls13_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    rustls::crypto::verify_tls13_signature(
      message,
      cert,
      dss,
      &self.0.signature_verification_algorithms,
    )
  }

  fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
    self.0.signature_verification_algorithms.supported_schemes()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn a_socket_that_does_not_open_names_its_address_without_the_password() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // a scheme that is not http or https connects and then fails
    let url = Url::parse(&format!(
      "relay://nas:hunter2@127.0.0.1:{port}/api/v0/relay/ws"
    ))
    .unwrap();
    let tls = Arc::new(tls(false).unwrap());
    let Err(DialError::Failed(e)) = open_socket(&url, &tls, HeaderMap::new()).await else {
      panic!("the socket does not open");
    };
    assert!(e.contains(&format!("relay://127.0.0.1:{port}/")), "{e}");
    assert!(!e.contains("hunter2"), "{e}");
  }
}
