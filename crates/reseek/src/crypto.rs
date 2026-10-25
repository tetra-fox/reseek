//! Random secrets, their Base62 text form, and self-signed TLS certificates.

use std::net::IpAddr;

use rand::RngCore;
use rcgen::{
  CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose,
  PKCS_RSA_SHA256, SanType,
};

/// The digits of Base62, in value order.
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// How long a generated certificate is valid, the longest browsers accept.
const CERTIFICATE_DAYS: i64 = 825;

/// `n` bytes from a cryptographically secure generator seeded by the operating system.
pub fn random_bytes(n: usize) -> Vec<u8> {
  let mut bytes = vec![0; n];
  rand::rng().fill_bytes(&mut bytes);
  bytes
}

/// `bytes` read as one big-endian number and written in Base62, most significant digit
/// first. Each leading zero byte before the last byte is written as a leading `0`, so the
/// length of the input shows.
pub fn base62(bytes: &[u8]) -> String {
  let zeros = bytes
    .iter()
    .take_while(|&&b| b == 0)
    .count()
    .min(bytes.len().saturating_sub(1));
  let mut number = bytes[zeros..].to_vec();
  let mut digits = Vec::new();
  while number.iter().any(|&b| b != 0) {
    let mut remainder = 0u32;
    for byte in &mut number {
      let value = (remainder << 8) | u32::from(*byte);
      *byte = u8::try_from(value / 62).expect("a byte's quotient by 62 fits a byte");
      remainder = value % 62;
    }
    digits.push(BASE62[remainder as usize]);
  }
  if digits.is_empty() {
    digits.push(b'0');
  }
  let mut text = "0".repeat(zeros);
  text.extend(digits.iter().rev().map(|&d| char::from(d)));
  text
}

/// A new self-signed certificate and its private key, both DER.
pub struct SelfSigned {
  /// the certificate
  pub certificate: Vec<u8>,
  /// its PKCS#8 private key
  pub key: Vec<u8>,
}

/// Makes a self-signed certificate for `CN=reseek`, valid for `localhost`, the loopback
/// addresses and `addresses`, with an RSA 2048 key.
pub fn self_signed(addresses: &[IpAddr]) -> Result<SelfSigned, rcgen::Error> {
  let key = KeyPair::generate_for(&PKCS_RSA_SHA256)?;
  let mut params = CertificateParams::default();
  let mut name = DistinguishedName::new();
  name.push(DnType::CommonName, "reseek");
  params.distinguished_name = name;
  let mut names = vec![SanType::DnsName("localhost".try_into()?)];
  let loopback: [IpAddr; 2] = [
    std::net::Ipv4Addr::LOCALHOST.into(),
    std::net::Ipv6Addr::LOCALHOST.into(),
  ];
  for ip in loopback.iter().chain(addresses) {
    if !ip.is_unspecified() && !names.contains(&SanType::IpAddress(*ip)) {
      names.push(SanType::IpAddress(*ip));
    }
  }
  params.subject_alt_names = names;
  params.key_usages = vec![
    KeyUsagePurpose::DigitalSignature,
    KeyUsagePurpose::KeyEncipherment,
  ];
  params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
  let now = time::OffsetDateTime::now_utc();
  params.not_before = now - time::Duration::days(1);
  params.not_after = now + time::Duration::days(CERTIFICATE_DAYS);
  let certificate = params.self_signed(&key)?;
  Ok(SelfSigned {
    certificate: certificate.der().to_vec(),
    key: key.serialize_der(),
  })
}

/// `certificate` with its key as a PKCS#12 file protected by `password`.
pub fn pkcs12(
  certificate: &SelfSigned,
  password: &str,
) -> Result<Vec<u8>, p12_keystore::error::Error> {
  let key = p12_keystore::PrivateKey::from_der(&certificate.key)?;
  let cert = p12_keystore::Certificate::from_der(&certificate.certificate)?;
  let chain = p12_keystore::PrivateKeyChain::new(random_bytes(20), key, [cert]);
  let mut store = p12_keystore::KeyStore::new();
  store.add_entry(
    "reseek",
    p12_keystore::KeyStoreEntry::PrivateKeyChain(chain),
  );
  store.writer(password).write()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn base62_is_a_big_number_conversion_that_keeps_leading_zeros() {
    assert_eq!(base62(&[0, 0, 1]), "001");
    assert_eq!(base62(&[255]), "47");
    assert_eq!(base62(&[0]), "0");
    assert_eq!(base62(&[0, 0]), "00");
    assert_eq!(base62(&[1, 0]), "48");
    assert_eq!(base62(&[]), "0");
  }
}
