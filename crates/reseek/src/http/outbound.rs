//! The HTTP client for the calls the daemon makes itself: to OpenID Connect providers, to the
//! release feed and to Grafana Loki.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

/// How long an outbound call may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// An HTTP client that trusts the usual certificate authorities and the one in `ca`, does not
/// follow redirects, and gives up after [`TIMEOUT`].
pub fn client(ca: Option<&Path>) -> Result<reqwest::Client, String> {
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
  reqwest::Client::builder()
    .tls_backend_preconfigured(tls)
    .redirect(reqwest::redirect::Policy::none())
    .timeout(TIMEOUT)
    .build()
    .map_err(|e| e.to_string())
}
