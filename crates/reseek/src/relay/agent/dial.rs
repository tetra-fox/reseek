//! Reaching the controller: the settings of the connection, the socket an agent opens with its
//! API key and the relay protocol, and the HTTP client its uploads and fetches use. Both trust
//! the usual certificate authorities, or any certificate when the options say so.

use std::sync::Arc;
use std::time::Duration;

use api_types::relay::RELAY_SUBPROTOCOL;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use url::Url;

use crate::config::Options;
use crate::relay::credential::Key;
use crate::relay::frames::MAX_FRAME;

/// How long dialling the controller may take, TLS and the upgrade included.
const DIAL: Duration = Duration::from_secs(10);

/// A connection to the controller that is plain or TLS.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// The agent's socket to its controller.
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
  /// The settings in `options`, which were validated when they were loaded.
  pub fn new(options: &Options) -> Result<Self, String> {
    // an agent has these three, which loading the options checked
    let controller = &options.relay.controller;
    let text = controller.address.as_deref().unwrap_or_default();
    let address =
      Url::parse(text.trim_end_matches('/')).map_err(|e| format!("controller address: {e}"))?;
    let tls = Arc::new(tls(controller.ignore_certificate_errors)?);
    let http = reqwest::Client::builder()
      .use_preconfigured_tls((*tls).clone())
      .redirect(reqwest::redirect::Policy::none())
      .connect_timeout(DIAL)
      .build()
      .map_err(|e| e.to_string())?;
    Ok(Self {
      address,
      api_key: controller.api_key.clone().unwrap_or_default(),
      agent: options.instance_name.clone(),
      key: Key::derive(
        controller.secret.as_deref().unwrap_or_default(),
        &options.instance_name,
      ),
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
  pub async fn dial(&self) -> Result<Socket, String> {
    tokio::time::timeout(DIAL, self.open())
      .await
      .map_err(|_| "the controller did not answer in time".to_string())?
  }

  async fn open(&self) -> Result<Socket, String> {
    let host = self
      .address
      .host_str()
      .ok_or("the controller address has no host")?
      .trim_start_matches('[')
      .trim_end_matches(']')
      .to_string();
    let port = self
      .address
      .port_or_known_default()
      .ok_or("the controller address has no port")?;
    let tcp = TcpStream::connect((host.as_str(), port))
      .await
      .map_err(|e| format!("{host}:{port}: {e}"))?;
    let secure = self.address.scheme() == "https";
    let stream: Box<dyn Io> = if secure {
      let name = ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
      let tls = tokio_rustls::TlsConnector::from(self.tls.clone())
        .connect(name, tcp)
        .await
        .map_err(|e| format!("TLS: {e}"))?;
      Box::new(tls)
    } else {
      Box::new(tcp)
    };
    let mut socket = self.url("relay/ws");
    socket.replace_range(
      ..if secure { 5 } else { 4 },
      if secure { "wss" } else { "ws" },
    );
    let mut request = socket.into_client_request().map_err(|e| e.to_string())?;
    let headers = request.headers_mut();
    headers.insert(
      "x-api-key",
      HeaderValue::from_str(&self.api_key).map_err(|e| e.to_string())?,
    );
    headers.insert(
      "sec-websocket-protocol",
      HeaderValue::from_static(RELAY_SUBPROTOCOL),
    );
    let config = WebSocketConfig::default()
      .max_message_size(Some(2 * MAX_FRAME))
      .max_frame_size(Some(2 * MAX_FRAME));
    let (socket, response) =
      tokio_tungstenite::client_async_with_config(request, stream, Some(config))
        .await
        .map_err(|e| e.to_string())?;
    let chosen = response
      .headers()
      .get("sec-websocket-protocol")
      .and_then(|value| value.to_str().ok());
    if chosen != Some(RELAY_SUBPROTOCOL) {
      return Err(format!("the controller chose the protocol {chosen:?}"));
    }
    Ok(socket)
  }
}

/// The TLS settings: the usual authorities, or any certificate with `ignore_certificates`.
fn tls(ignore_certificates: bool) -> Result<rustls::ClientConfig, String> {
  let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
  let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?;
  let config = if ignore_certificates {
    builder
      .dangerous()
      .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
      .with_no_client_auth()
  } else {
    builder
      .with_root_certificates(rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
      })
      .with_no_client_auth()
  };
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
