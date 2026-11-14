//! The HTTP server: listeners, TLS, the web UI and what browsers may reach.

use std::path::PathBuf;

use cascade::Cascade;
use serde::{Deserialize, Serialize};

use super::Authentication;

/// HTTP server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Web {
  /// HTTP port. `0` picks a free port and writes it to the log.
  #[cascade(env = "RESEEK_HTTP_PORT", cli = "-l|--http-port", apply = "restart")]
  pub port: u16,
  /// Addresses to listen on, comma-separated. All IPv6 and IPv4 addresses when not set.
  #[cascade(
    env = "RESEEK_HTTP_IP_ADDRESS",
    cli = "--http-ip-address",
    apply = "restart"
  )]
  pub ip_address: Option<String>,
  /// Additional Unix domain socket to listen on. A relative path counts from the file that sets it.
  #[cascade(
    env = "RESEEK_HTTP_SOCKET",
    cli = "--http-socket",
    apply = "restart",
    path
  )]
  pub socket: Option<PathBuf>,
  /// Path under which the daemon is served behind a reverse proxy, such as `/reseek`.
  #[cascade(env = "RESEEK_URL_BASE", cli = "--url-base", apply = "restart")]
  pub url_base: String,
  /// Serves the web UI.
  #[cascade(env = "RESEEK_WEB_UI", cli = "--web-ui", apply = "restart")]
  pub ui: bool,
  /// Directory to serve the UI from, instead of the UI built into the program. Relative to
  /// the program directory.
  #[cascade(env = "RESEEK_CONTENT_PATH", cli = "--content-path", apply = "restart")]
  pub content_path: Option<PathBuf>,
  /// Logs one line per HTTP request.
  #[cascade(env = "RESEEK_HTTP_LOGGING", cli = "--http-logging", apply = "restart")]
  pub logging: bool,
  /// HTTPS listener.
  #[cascade(apply = "restart")]
  pub https: Https,
  /// Origins from which browsers may call the API.
  #[cascade(apply = "restart")]
  pub cors: Cors,
  /// Serving local files to the browser.
  pub files: WebFiles,
  /// Event stream.
  pub live: Live,
  /// Networks of reverse proxies whose `X-Forwarded-For`, `X-Forwarded-Proto` and
  /// `X-Forwarded-Host` headers are trusted.
  #[cascade(env = "RESEEK_TRUSTED_PROXIES", cli = "--trusted-proxies")]
  pub trusted_proxies: Vec<String>,
  /// Host names by which the daemon is reached. A trusted network is trusted only on these
  /// names, on IP addresses and on `localhost`.
  #[cascade(env = "RESEEK_HOSTNAMES", cli = "--hostnames")]
  pub hostnames: Vec<String>,
  /// Public base URL for login redirects, such as `https://reseek.example.net/reseek`.
  /// Derived from each request when not set.
  #[cascade(env = "RESEEK_EXTERNAL_URL", cli = "--external-url")]
  pub external_url: Option<String>,
  /// Access to the API and the web UI.
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

/// HTTPS listener.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Https {
  /// Disables the HTTPS listener.
  #[cascade(env = "RESEEK_NO_HTTPS", cli = "--no-https")]
  pub disabled: bool,
  /// HTTPS port.
  #[cascade(env = "RESEEK_HTTPS_PORT", cli = "-L|--https-port")]
  pub port: u16,
  /// Addresses to listen on, comma-separated. All addresses when not set.
  #[cascade(env = "RESEEK_HTTPS_IP_ADDRESS", cli = "--https-ip-address")]
  pub ip_address: Option<String>,
  /// Redirects HTTP requests to HTTPS and sends HSTS.
  #[cascade(env = "RESEEK_HTTPS_FORCE", cli = "-f|--force-https")]
  pub force: bool,
  /// TLS certificate. A new self-signed certificate is generated at every start when none is
  /// set.
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

/// HTTPS certificate: a PEM certificate chain and key, or a PKCS#12 file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Certificate {
  /// PEM file with the certificate chain, used with `key`. A relative path counts from the file
  /// that sets it.
  #[cascade(env = "RESEEK_HTTPS_CERT", cli = "--https-cert", path)]
  pub cert: Option<PathBuf>,
  /// PEM file with the private key for `cert`. A relative path counts from the file that sets it.
  #[cascade(env = "RESEEK_HTTPS_KEY", cli = "--https-key", path)]
  pub key: Option<PathBuf>,
  /// PKCS#12 file with the certificate and its key, used when `cert` is not set. A relative path
  /// counts from the file that sets it.
  #[cascade(env = "RESEEK_HTTPS_CERT_PFX", cli = "--https-cert-pfx", path)]
  pub pfx: Option<PathBuf>,
  /// Password of the PKCS#12 file.
  #[cascade(
    env = "RESEEK_HTTPS_CERT_PASSWORD",
    cli = "--https-cert-password",
    secret
  )]
  pub password: Option<String>,
}

/// Origins from which browsers may call the API.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Cors {
  /// Origins, such as `http://localhost:5173`, that may call the API with credentials. `*`
  /// allows any origin without credentials. No other origin may call it when empty.
  #[cascade(env = "RESEEK_CORS_ALLOWED_ORIGINS", cli = "--cors-allowed-origins")]
  pub allowed_origins: Vec<String>,
}

/// Serving local files to the browser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct WebFiles {
  /// Allows the browser to open and download files from the download directories.
  #[cascade(env = "RESEEK_WEB_FILES_SERVE")]
  pub serve: bool,
  /// Largest folder that may be downloaded as one zip archive.
  #[cascade(env = "RESEEK_WEB_FILES_ARCHIVE_MAX_BYTES", kind = "size")]
  pub archive_max_bytes: bytesize::ByteSize,
  /// Total speed limit for files and folders served to the browser, in KiB/s. No limit when
  /// not set.
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

/// Event stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Cascade)]
#[serde(default)]
pub struct Live {
  /// Maximum number of live update streams one user or API key may hold open.
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
