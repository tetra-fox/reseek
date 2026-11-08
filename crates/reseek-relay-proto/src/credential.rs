//! The credential an agent proves it holds its secret with. The secret never travels: both
//! sides derive a key from it, salted with the agent's instance name, and the agent answers
//! each token the controller hands it with a keyed hash of the token and what the token is
//! for, which the controller computes again and compares in constant time.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// How many rounds the key derivation runs.
const ROUNDS: u32 = 1000;

/// The bytes the derivation yields; the credential is keyed with the first 32.
const DERIVED: usize = 48;

/// What a token is for, so a credential made for one purpose is refused for another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
  /// the challenge of a login on the socket
  Login,
  /// uploading the share database
  Shares,
  /// sending a file a peer downloads
  File,
  /// fetching a finished download
  Download,
}

impl Purpose {
  fn label(self) -> &'static str {
    match self {
      Self::Login => "login",
      Self::Shares => "shares",
      Self::File => "file",
      Self::Download => "download",
    }
  }
}

/// The key an agent's credentials are made with, derived from its secret and its instance
/// name.
#[derive(Clone)]
pub struct Key([u8; 32]);

impl std::fmt::Debug for Key {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("Key(..)")
  }
}

impl Key {
  /// The key of the agent `agent` whose secret is `secret`.
  pub fn derive(secret: &str, agent: &str) -> Self {
    let derived = pbkdf2_sha256(secret.as_bytes(), agent.as_bytes(), ROUNDS);
    let mut key = [0; 32];
    key.copy_from_slice(&derived[..32]);
    Self(key)
  }

  /// The credential for `token` used for `purpose`, as lower-case hex.
  pub fn credential(&self, purpose: Purpose, token: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC takes a key of any length");
    mac.update(purpose.label().as_bytes());
    mac.update(b"\n");
    mac.update(token.as_bytes());
    mac
      .finalize()
      .into_bytes()
      .iter()
      .map(|b| format!("{b:02x}"))
      .collect()
  }

  /// Whether `presented` is the credential for `token` used for `purpose`, compared in time
  /// that does not depend on where the two differ.
  pub fn verifies(&self, purpose: Purpose, token: &str, presented: &str) -> bool {
    let expected = self.credential(purpose, token);
    expected.len() == presented.len() && bool::from(expected.as_bytes().ct_eq(presented.as_bytes()))
  }
}

/// PBKDF2 with HMAC-SHA256, yielding [`DERIVED`] bytes.
fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32) -> [u8; DERIVED] {
  let prf = Hmac::<Sha256>::new_from_slice(password).expect("HMAC takes a key of any length");
  let mut out = [0; DERIVED];
  for (index, block) in out.chunks_mut(32).enumerate() {
    let number = u32::try_from(index + 1).expect("two blocks");
    let mut mac = prf.clone();
    mac.update(salt);
    mac.update(&number.to_be_bytes());
    let mut u = mac.finalize().into_bytes();
    let mut t = u;
    for _ in 1..rounds {
      let mut mac = prf.clone();
      mac.update(&u);
      u = mac.finalize().into_bytes();
      for (t, u) in t.iter_mut().zip(u.iter()) {
        *t ^= u;
      }
    }
    block.copy_from_slice(&t[..block.len()]);
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
  }

  #[test]
  fn the_derivation_is_pbkdf2_with_hmac_sha256() {
    // the published PBKDF2-HMAC-SHA256 vectors for "password" and "salt"
    assert_eq!(
      hex(&pbkdf2_sha256(b"password", b"salt", 1)[..32]),
      "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
    );
    assert_eq!(
      hex(&pbkdf2_sha256(b"password", b"salt", 2)[..32]),
      "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
    );
    let derived = pbkdf2_sha256(
      b"passwordPASSWORDpassword",
      b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
      4096,
    );
    assert_eq!(
      hex(&derived[..40]),
      "348c89dbcbd32b2f32d814b8116e84cf2b17347ebc1800181c4e2a1fb8dd53e1c635518c7dac47e9"
    );
  }

  #[test]
  fn a_credential_holds_for_its_token_purpose_and_key_only() {
    let key = Key::derive("0123456789abcdef", "nas");
    let credential = key.credential(Purpose::Login, "token");
    assert_eq!(credential.len(), 64);
    assert!(
      credential
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert!(key.verifies(Purpose::Login, "token", &credential));
    assert!(!key.verifies(Purpose::Shares, "token", &credential));
    assert!(!key.verifies(Purpose::Login, "other", &credential));
    assert!(!key.verifies(Purpose::Login, "token", &credential[..63]));
    let other = Key::derive("0123456789abcdef", "laptop");
    assert!(!other.verifies(Purpose::Login, "token", &credential));
  }
}
