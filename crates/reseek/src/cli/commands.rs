//! The commands that do one thing and exit.

use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};

use crate::auth::local;
use crate::crypto;

/// The name `--generate-cert` writes when given no path.
const CERT_FILE: &str = "reseek.pfx";

/// Writes a new self-signed certificate with a random password as PKCS#12 to `path`, or to
/// `reseek.pfx` in the working directory, and returns what to print.
pub fn generate_cert(path: Option<&Path>) -> Result<String, String> {
  let path = path.map_or_else(|| PathBuf::from(CERT_FILE), Path::to_path_buf);
  let password = crypto::base62(&crypto::random_bytes(16));
  let certificate = crypto::self_signed(&[]).map_err(|e| e.to_string())?;
  let file = crypto::pkcs12(&certificate, &password).map_err(|e| e.to_string())?;
  std::fs::write(&path, file).map_err(|e| format!("writing {}: {e}", path.display()))?;
  Ok(format!(
    "Certificate exported to {}\nPassword: {password}",
    path.display()
  ))
}

/// A secret of `length` random bytes in Base62.
pub fn generate_secret(length: usize) -> String {
  crypto::base62(&crypto::random_bytes(length))
}

/// Reads a password, without echo from a terminal or as the first line of stdin otherwise, and
/// returns its argon2id hash for `password_hash`.
pub fn hash_password() -> Result<String, String> {
  let password = if std::io::stdin().is_terminal() {
    rpassword::prompt_password("Password: ").map_err(|e| format!("reading the password: {e}"))?
  } else {
    let mut line = String::new();
    std::io::stdin()
      .lock()
      .read_line(&mut line)
      .map_err(|e| format!("reading the password: {e}"))?;
    line.trim_end_matches(['\n', '\r']).to_string()
  };
  if password.is_empty() {
    return Err("the password is empty".to_string());
  }
  Ok(local::hash(&password))
}
