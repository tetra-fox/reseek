//! The certificate authorities the TLS clients of reseek and its relay agent trust: the roots
//! built into the program, from webpki-roots, and the system's. On Linux the system's are found
//! where OpenSSL looks, `SSL_CERT_FILE` and `SSL_CERT_DIR` first, then the usual bundle files
//! and directories; on macOS and Windows they come from the platform's store. A machine without
//! a system store trusts the built-in roots alone, and one whose store cannot be read is told
//! once and trusts what could be read.

use std::sync::{Arc, Once};

use rustls::RootCertStore;
use rustls_native_certs::CertificateResult;
use tracing::{debug, info, warn};

/// The system store was described in the log.
static TOLD: Once = Once::new();

/// The built-in roots and the system's, read now, so a certificate authority added to the system
/// is trusted by the clients built after it.
pub fn roots() -> RootCertStore {
  let (roots, system) = with_system(rustls_native_certs::load_native_certs());
  TOLD.call_once(|| system.log());
  roots
}

/// TLS client settings that trust `roots`.
pub fn client_config(roots: RootCertStore) -> Result<rustls::ClientConfig, rustls::Error> {
  let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
  Ok(
    rustls::ClientConfig::builder_with_provider(provider)
      .with_safe_default_protocol_versions()?
      .with_root_certificates(roots)
      .with_no_client_auth(),
  )
}

/// What the system's certificate store gave.
#[derive(Debug, PartialEq)]
enum System {
  /// there is none
  Missing,
  /// `added` authorities, and what could not be read or parsed
  Read { added: usize, problems: Vec<String> },
}

impl System {
  fn log(&self) {
    match self {
      System::Missing => {
        info!("no system certificate store found; TLS trusts the built-in authorities only");
      }
      System::Read { added, problems } if problems.is_empty() => {
        debug!(added, "system certificate authorities trusted");
      }
      System::Read { added, problems } => {
        warn!(
          added,
          ?problems,
          "the system certificate store was not read in full; TLS trusts the built-in authorities and what was read"
        );
      }
    }
  }
}

/// The built-in roots with the authorities of `system` added.
fn with_system(system: CertificateResult) -> (RootCertStore, System) {
  let mut roots = RootCertStore {
    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
  };
  let mut problems: Vec<String> = system.errors.iter().map(ToString::to_string).collect();
  if system.certs.is_empty() && problems.is_empty() {
    return (roots, System::Missing);
  }
  let (added, ignored) = roots.add_parsable_certificates(system.certs);
  if ignored > 0 {
    problems.push(format!("{ignored} certificates could not be parsed"));
  }
  (roots, System::Read { added, problems })
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
  use rustls::client::WebPkiServerVerifier;
  use rustls::client::danger::ServerCertVerifier;
  use rustls::pki_types::{ServerName, UnixTime};
  use rustls_native_certs::load_certs_from_paths;

  use super::*;

  /// A certificate authority of our own, written as a PEM bundle into `dir`, and a certificate
  /// it issued for `controller.lan`.
  fn private_ca(
    dir: &Path,
  ) -> (
    std::path::PathBuf,
    rustls::pki_types::CertificateDer<'static>,
  ) {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
    let bundle = dir.join("bundle.pem");
    std::fs::write(&bundle, ca.pem()).unwrap();
    let leaf = CertificateParams::new(vec!["controller.lan".to_string()])
      .unwrap()
      .signed_by(&KeyPair::generate().unwrap(), &ca)
      .unwrap();
    (bundle, leaf.der().clone())
  }

  /// Whether `roots` trust `leaf` as the certificate of `controller.lan`.
  fn trusted(roots: RootCertStore, leaf: &rustls::pki_types::CertificateDer<'_>) -> bool {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
      .build()
      .unwrap();
    let name = ServerName::try_from("controller.lan").unwrap();
    verifier
      .verify_server_cert(leaf, &[], &name, &[], UnixTime::now())
      .is_ok()
  }

  #[test]
  fn a_system_authority_is_trusted_beside_the_built_in_ones() {
    let dir = tempfile::tempdir().unwrap();
    let (bundle, leaf) = private_ca(dir.path());
    let (roots, system) = with_system(load_certs_from_paths(Some(&bundle), None));
    assert_eq!(
      system,
      System::Read {
        added: 1,
        problems: Vec::new()
      }
    );
    assert_eq!(roots.len(), webpki_roots::TLS_SERVER_ROOTS.len() + 1);
    assert!(trusted(roots, &leaf));
  }

  #[test]
  fn without_a_system_store_the_built_in_roots_are_trusted_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (_, leaf) = private_ca(dir.path());
    let (roots, system) = with_system(load_certs_from_paths(None, None));
    assert_eq!(system, System::Missing);
    assert_eq!(roots.len(), webpki_roots::TLS_SERVER_ROOTS.len());
    assert!(!trusted(roots, &leaf));
  }

  #[test]
  fn a_store_that_cannot_be_read_leaves_the_built_in_roots() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.pem");
    let (roots, system) = with_system(load_certs_from_paths(Some(&missing), None));
    let System::Read { added, problems } = system else {
      panic!("{system:?}");
    };
    assert_eq!(added, 0);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(roots.len(), webpki_roots::TLS_SERVER_ROOTS.len());
  }
}
