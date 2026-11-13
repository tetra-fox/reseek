//! The HTTP client for the calls the daemon makes itself: to OpenID Connect providers, to the
//! release feed and to Grafana Loki.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

/// How long an outbound call may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// An HTTP client that trusts the certificate authorities built in, the system's and the one in
/// `ca`, does not follow redirects, and gives up after [`TIMEOUT`].
pub async fn client(ca: Option<&Path>) -> Result<reqwest::Client, String> {
  let ca = ca.map(Path::to_path_buf);
  // the authorities are read from disk
  let roots = tokio::task::spawn_blocking(move || roots(ca))
    .await
    .map_err(|e| e.to_string())??;
  let tls = reseek_tls::client_config(roots).map_err(|e| e.to_string())?;
  reqwest::Client::builder()
    .tls_backend_preconfigured(tls)
    .redirect(reqwest::redirect::Policy::none())
    .timeout(TIMEOUT)
    .build()
    .map_err(|e| e.to_string())
}

/// The certificate authorities built in, the system's and the one in `ca`.
fn roots(ca: Option<PathBuf>) -> Result<rustls::RootCertStore, String> {
  let mut roots = reseek_tls::roots();
  if let Some(path) = ca {
    let certificates = CertificateDer::pem_file_iter(&path)
      .and_then(|certificates| certificates.collect::<Result<Vec<_>, _>>())
      .map_err(|e| format!("{}: {e}", path.display()))?;
    for certificate in certificates {
      roots
        .add(certificate)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    }
  }
  Ok(roots)
}
