//! The HTTP client for the calls the daemon makes itself: to OpenID Connect providers, to the
//! release feed and to Grafana Loki.

use std::path::Path;
use std::time::Duration;

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

/// How long an outbound call may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// An HTTP client that trusts the certificate authorities built in, the system's and the one in
/// `ca`, does not follow redirects, and gives up after [`TIMEOUT`].
pub fn client(ca: Option<&Path>) -> Result<reqwest::Client, String> {
  let mut roots = reseek_tls::roots();
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
  let tls = reseek_tls::client_config(roots).map_err(|e| e.to_string())?;
  reqwest::Client::builder()
    .tls_backend_preconfigured(tls)
    .redirect(reqwest::redirect::Policy::none())
    .timeout(TIMEOUT)
    .build()
    .map_err(|e| e.to_string())
}
