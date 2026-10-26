//! The HTTP server: listeners, TLS, the web UI and what browsers may reach.

use std::path::PathBuf;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use super::Authentication;

/// The HTTP server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Web {
  /// The HTTP port.
  #[cascade(env = "RESEEK_HTTP_PORT", cli = "-l|--http-port", apply = "restart")]
  pub port: u16,
  /// The addresses to listen on, comma-separated; every address, IPv6 and IPv4, when not set.
  #[cascade(
    env = "RESEEK_HTTP_IP_ADDRESS",
    cli = "--http-ip-address",
    apply = "restart"
  )]
  pub ip_address: Option<String>,
  /// Also listens on a unix domain socket at this absolute path.
  #[cascade(env = "RESEEK_HTTP_SOCKET", cli = "--http-socket", apply = "restart")]
  pub socket: Option<PathBuf>,
  /// The path the daemon is served under behind a reverse proxy, such as `/reseek`.
  #[cascade(env = "RESEEK_URL_BASE", cli = "--url-base", apply = "restart")]
  pub url_base: String,
  /// Serves the web UI.
  #[cascade(env = "RESEEK_WEB_UI", cli = "--web-ui", apply = "restart")]
  pub ui: bool,
  /// Serves the UI from this directory, relative to the program's directory, instead of the
  /// one built into the program.
  #[cascade(env = "RESEEK_CONTENT_PATH", cli = "--content-path", apply = "restart")]
  pub content_path: Option<PathBuf>,
  /// Logs one line per HTTP request.
  #[cascade(env = "RESEEK_HTTP_LOGGING", cli = "--http-logging", apply = "restart")]
  pub logging: bool,
  /// The HTTPS listener.
  #[cascade(apply = "restart")]
  pub https: Https,
  /// Which other origins browsers may call the API from.
  #[cascade(apply = "restart")]
  pub cors: Cors,
  /// Serving local files to the browser.
  pub files: WebFiles,
  /// The event stream.
  pub live: Live,
  /// Reverse proxies, as networks, whose `X-Forwarded-For`, `X-Forwarded-Proto` and
  /// `X-Forwarded-Host` headers are believed.
  #[cascade(env = "RESEEK_TRUSTED_PROXIES", cli = "--trusted-proxies")]
  pub trusted_proxies: Vec<String>,
  /// The host names the daemon is reached by; a trusted network is only trusted on these, on
  /// IP addresses and on `localhost`.
  #[cascade(env = "RESEEK_HOSTNAMES", cli = "--hostnames")]
  pub hostnames: Vec<String>,
  /// The public base URL, such as `https://reseek.example.net/reseek`, for login redirects;
  /// worked out from each request when not set.
  #[cascade(env = "RESEEK_EXTERNAL_URL", cli = "--external-url")]
  pub external_url: Option<String>,
  /// Who may use the API and the web UI.
  pub authentication: Authentication,
}

impl Default for Web {
  fn default() -> Self {
    Self {
      port: 5030,
      ip_address: None,
      socket: None,
      url_base: "/".to_string(),
      ui: true,
      content_path: None,
      logging: false,
      https: Https::default(),
      cors: Cors::default(),
      files: WebFiles::default(),
      live: Live::default(),
      trusted_proxies: Vec::new(),
      hostnames: Vec::new(),
      external_url: None,
      authentication: Authentication::default(),
    }
  }
}

/// The HTTPS listener.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Https {
  /// Does not listen for HTTPS.
  #[cascade(env = "RESEEK_NO_HTTPS", cli = "--no-https")]
  pub disabled: bool,
  /// The HTTPS port.
  #[cascade(env = "RESEEK_HTTPS_PORT", cli = "-L|--https-port")]
  pub port: u16,
  /// The addresses to listen on, comma-separated; every address when not set.
  #[cascade(env = "RESEEK_HTTPS_IP_ADDRESS", cli = "--https-ip-address")]
  pub ip_address: Option<String>,
  /// Redirects HTTP requests to HTTPS and sends HSTS.
  #[cascade(env = "RESEEK_HTTPS_FORCE", cli = "-f|--force-https")]
  pub force: bool,
  /// The certificate; a new self-signed one at every start when none is given.
  pub certificate: Certificate,
}

impl Default for Https {
  fn default() -> Self {
    Self {
      disabled: false,
      port: 5031,
      ip_address: None,
      force: false,
      certificate: Certificate::default(),
    }
  }
}

/// The HTTPS certificate: a PEM certificate chain and key, or a PKCS#12 file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Certificate {
  /// A PEM file with the certificate chain, used with `key`.
  #[cascade(env = "RESEEK_HTTPS_CERT", cli = "--https-cert")]
  pub cert: Option<PathBuf>,
  /// A PEM file with the private key of `cert`.
  #[cascade(env = "RESEEK_HTTPS_KEY", cli = "--https-key")]
  pub key: Option<PathBuf>,
  /// A PKCS#12 file with the certificate and its key, used when `cert` is not set.
  #[cascade(env = "RESEEK_HTTPS_CERT_PFX", cli = "--https-cert-pfx")]
  pub pfx: Option<PathBuf>,
  /// The password of the PKCS#12 file.
  #[cascade(
    env = "RESEEK_HTTPS_CERT_PASSWORD",
    cli = "--https-cert-password",
    secret
  )]
  pub password: Option<String>,
}

/// Which other origins browsers may call the API from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Cors {
  /// Origins such as `http://localhost:5173` that may call the API with credentials; `*` lets
  /// any origin call it without credentials. None when empty.
  #[cascade(env = "RESEEK_CORS_ALLOWED_ORIGINS", cli = "--cors-allowed-origins")]
  pub allowed_origins: Vec<String>,
}

/// Serving local files to the browser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct WebFiles {
  /// Lets the browser open and download files from the download directories.
  #[cascade(env = "RESEEK_WEB_FILES_SERVE")]
  pub serve: bool,
  /// The largest folder that may be downloaded as one zip.
  #[cascade(env = "RESEEK_WEB_FILES_ARCHIVE_MAX_BYTES", kind = "size")]
  pub archive_max_bytes: bytesize::ByteSize,
  /// The speed files are served to the browser at, in KiB/s; no limit when not set. Not
  /// applied in this build, which warns at startup when it is set.
  #[cascade(env = "RESEEK_WEB_FILES_SERVE_SPEED_LIMIT")]
  pub serve_speed_limit: Option<u32>,
}

impl Default for WebFiles {
  fn default() -> Self {
    Self {
      serve: true,
      archive_max_bytes: bytesize::ByteSize::gb(20),
      serve_speed_limit: None,
    }
  }
}

/// The event stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Live {
  /// How many event streams one principal may hold open.
  #[cascade(env = "RESEEK_LIVE_MAX_STREAMS_PER_PRINCIPAL")]
  pub max_streams_per_principal: u32,
}

impl Default for Live {
  fn default() -> Self {
    Self {
      max_streams_per_principal: 16,
    }
  }
}
