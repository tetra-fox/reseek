//! The HTTPS certificate: a PEM certificate chain and key, a PKCS#12 file, or a self-signed
//! certificate made at startup.

use std::net::IpAddr;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tracing::info;

use crate::config::Https;
use crate::crypto;

/// The TLS configuration of the HTTPS listeners, offering HTTP/2 and HTTP/1.1.
pub fn server_config(https: &Https, addresses: &[IpAddr]) -> Result<ServerConfig, String> {
  let (chain, key) = certificate(https, addresses)?;
  let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
  let mut config = ServerConfig::builder_with_provider(provider)
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_no_client_auth()
    .with_single_cert(chain, key)
    .map_err(|e| format!("Invalid HTTPs certificate: {e}"))?;
  config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
  Ok(config)
}

type Pair = (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>);

fn certificate(https: &Https, addresses: &[IpAddr]) -> Result<Pair, String> {
  let configured = &https.certificate;
  if let (Some(cert), Some(key)) = (&configured.cert, &configured.key) {
    let invalid = |e: rustls::pki_types::pem::Error| format!("Invalid HTTPs certificate: {e}");
    let chain = CertificateDer::pem_file_iter(cert)
      .map_err(invalid)?
      .collect::<Result<Vec<_>, _>>()
      .map_err(invalid)?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(invalid)?;
    info!(path = %cert.display(), "Using certificate from file");
    return Ok((chain, key));
  }
  if let Some(pfx) = &configured.pfx {
    let pair = pkcs12(pfx, configured.password.as_deref().unwrap_or(""))
      .map_err(|e| format!("Invalid HTTPs certificate: {e}"))?;
    info!(path = %pfx.display(), "Using certificate from file");
    return Ok(pair);
  }
  let generated = crypto::self_signed(addresses).map_err(|e| e.to_string())?;
  info!("Using randomly generated self-signed certificate");
  Ok((
    vec![CertificateDer::from(generated.certificate)],
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(generated.key)),
  ))
}

/// The certificate chain and key of a PKCS#12 file.
fn pkcs12(path: &std::path::Path, password: &str) -> Result<Pair, String> {
  let data = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
  let store =
    p12_keystore::KeyStore::from_pkcs12(&data, password, p12_keystore::Pkcs12ImportPolicy::Strict)
      .map_err(|e| e.to_string())?;
  let (_, chain) = store
    .private_key_chain()
    .ok_or("the file holds no private key with its certificate")?;
  let certs = chain
    .certs()
    .iter()
    .map(|c| CertificateDer::from(c.as_der().to_vec()))
    .collect();
  let key = PrivateKeyDer::try_from(chain.key().as_der().to_vec()).map_err(str::to_string)?;
  Ok((certs, key))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_generated_pkcs12_file_loads_back_with_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reseek.pfx");
    let certificate = crypto::self_signed(&["192.168.1.2".parse().unwrap()]).unwrap();
    std::fs::write(&path, crypto::pkcs12(&certificate, "secret").unwrap()).unwrap();
    let (chain, _) = pkcs12(&path, "secret").unwrap();
    assert_eq!(chain[0].as_ref(), certificate.certificate.as_slice());
    assert!(pkcs12(&path, "wrong").is_err());
    let mut https = Https::default();
    https.certificate.pfx = Some(path);
    https.certificate.password = Some("secret".into());
    server_config(&https, &[]).unwrap();
  }
}
