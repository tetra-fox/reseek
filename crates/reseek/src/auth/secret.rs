//! The daemon's own secret key, kept in `auth.db` so sessions last across restarts, and what is
//! made with it: the MAC on session cookies and on file links, and sealed state the browser
//! carries.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use hmac::{Hmac, Mac};
use rusqlite::params;
use sha2::Sha256;
use tracing::info;

use crate::crypto;
use crate::db::{Database, DbError, timestamp};

type HmacSha256 = Hmac<Sha256>;

/// The name of the key's row.
const KEY: &str = "master";

/// How many random bytes the key has.
const KEY_BYTES: usize = 32;

/// How many bytes an AES-GCM nonce has.
const NONCE_BYTES: usize = 12;

/// The keys derived from the daemon's secret key.
pub struct Secret {
  /// signs session ids
  mac_key: [u8; 32],
  /// signs the tokens that let a link load a file
  link_key: [u8; 32],
  /// seals what the browser carries for us
  sealer: Aes256Gcm,
}

impl Secret {
  /// The secret key from `db`, made and stored on first use.
  pub async fn load(db: &Database) -> Result<Self, DbError> {
    let fresh = crypto::random_bytes(KEY_BYTES);
    let (master, created) = db
      .write(move |conn| {
        let inserted = conn.execute(
          "INSERT OR IGNORE INTO keys (name, secret, created_at) VALUES (?1, ?2, ?3)",
          params![KEY, fresh, timestamp(Utc::now())],
        )?;
        let master: Vec<u8> =
          conn.query_row("SELECT secret FROM keys WHERE name = ?1", [KEY], |row| {
            row.get(0)
          })?;
        Ok((master, inserted == 1))
      })
      .await?;
    if created {
      info!("generated the key session cookies are signed with");
    }
    Ok(Self::from_master(&master))
  }

  /// The keys derived from `master`, for tests that need a secret without a database.
  #[cfg(test)]
  pub fn for_tests(master: &[u8]) -> Self {
    Self::from_master(master)
  }

  fn from_master(master: &[u8]) -> Self {
    let derive = |label: &[u8]| -> [u8; 32] {
      let mut mac =
        <HmacSha256 as Mac>::new_from_slice(master).expect("HMAC takes a key of any length");
      mac.update(label);
      mac.finalize().into_bytes().into()
    };
    let seal_key = derive(b"reseek sealed state");
    Self {
      mac_key: derive(b"reseek session cookie"),
      link_key: derive(b"reseek file link"),
      sealer: Aes256Gcm::new(&seal_key.into()),
    }
  }

  /// The MAC of `data`, as base64url.
  pub fn sign(&self, data: &str) -> String {
    URL_SAFE_NO_PAD.encode(self.mac(data).finalize().into_bytes())
  }

  /// Whether `signature` is the MAC of `data`, compared in constant time.
  pub fn verify(&self, data: &str, signature: &str) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
      return false;
    };
    self.mac(data).verify_slice(&signature).is_ok()
  }

  /// The MAC of `data` with the key of file links, as base64url: an HMAC-SHA256 that a
  /// session cookie's MAC never equals.
  pub fn sign_link(&self, data: &str) -> String {
    URL_SAFE_NO_PAD.encode(keyed(&self.link_key, data).finalize().into_bytes())
  }

  /// Whether `signature` is the file-link MAC of `data`, compared in constant time.
  pub fn verify_link(&self, data: &str, signature: &str) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
      return false;
    };
    keyed(&self.link_key, data).verify_slice(&signature).is_ok()
  }

  fn mac(&self, data: &str) -> HmacSha256 {
    keyed(&self.mac_key, data)
  }

  /// `plain` encrypted and authenticated, as base64url text.
  pub fn seal(&self, plain: &[u8]) -> String {
    let nonce = crypto::random_bytes(NONCE_BYTES);
    let sealed = self
      .sealer
      .encrypt(Nonce::from_slice(&nonce), plain)
      .expect("AES-GCM encrypts any length this daemon seals");
    let mut bytes = nonce;
    bytes.extend(sealed);
    URL_SAFE_NO_PAD.encode(bytes)
  }

  /// What `sealed` held, `None` when it was not sealed by this key or was changed.
  pub fn open(&self, sealed: &str) -> Option<Vec<u8>> {
    let bytes = URL_SAFE_NO_PAD.decode(sealed).ok()?;
    if bytes.len() < NONCE_BYTES {
      return None;
    }
    let (nonce, sealed) = bytes.split_at(NONCE_BYTES);
    self.sealer.decrypt(Nonce::from_slice(nonce), sealed).ok()
  }
}

/// An HMAC-SHA256 of `data` under `key`, ready to finish or verify.
fn keyed(key: &[u8; 32], data: &str) -> HmacSha256 {
  let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes a 32-byte key");
  mac.update(data.as_bytes());
  mac
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn signatures_and_seals_hold_only_for_their_key_and_content() {
    let secret = Secret::from_master(b"one key");
    let other = Secret::from_master(b"another key");
    let signature = secret.sign("session-id");
    assert!(secret.verify("session-id", &signature));
    assert!(!secret.verify("session-ie", &signature));
    assert!(!other.verify("session-id", &signature));
    assert!(!secret.verify("session-id", "not base64!"));
    let link = secret.sign_link("session-id");
    assert!(secret.verify_link("session-id", &link));
    assert!(
      !secret.verify("session-id", &link),
      "a link MAC is no cookie MAC"
    );
    assert!(!other.verify_link("session-id", &link));

    let sealed = secret.seal(b"state and nonce");
    assert_eq!(secret.open(&sealed).unwrap(), b"state and nonce");
    assert!(other.open(&sealed).is_none());
    let mut tampered = URL_SAFE_NO_PAD.decode(&sealed).unwrap();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(secret.open(&URL_SAFE_NO_PAD.encode(tampered)).is_none());
    assert!(secret.open("AA").is_none());
  }
}
