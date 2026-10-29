//! The HTTP server and who may use it: listen addresses, the socket, the URL base, the UI
//! directory, the HTTPS certificate, trusted proxies and networks, accounts, API keys and
//! OpenID Connect providers.

use std::fs;
use std::net::IpAddr;
use std::path::Path;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use super::fields::{cidr, cidr_list, http_url, ip_addresses, length, range, required, url_path};
use super::paths::{absolute, directory_under, readable_file};
use super::{Context, Report};
use crate::auth::local;
use crate::config::api_key::PrimaryApiKey;
use crate::config::{Authentication, Certificate, Web};

pub(super) fn check(web: &Web, context: &Context, report: &mut Report) {
  let key = "web.https.port";
  report.check(key, range(key, web.https.port, 1, 65535));
  for (key, addresses) in [
    ("web.ip_address", &web.ip_address),
    ("web.https.ip_address", &web.https.ip_address),
  ] {
    if let Some(addresses) = addresses {
      report.check(key, ip_addresses(key, addresses));
    }
  }
  if let Some(socket) = &web.socket {
    report.check("web.socket", absolute("web.socket", socket));
  }
  report.check("web.url_base", url_path("web.url_base", &web.url_base));
  if let Some(content) = &web.content_path {
    let key = "web.content_path";
    report.check(
      key,
      length(key, Some(content.to_string_lossy().as_ref()), 1, 255),
    );
    report.check(key, directory_under(key, content, &context.program_dir));
  }
  super::transfers::check_speed(
    "web.files.serve_speed_limit",
    web.files.serve_speed_limit,
    report,
  );
  check_certificate(&web.https.certificate, report);
  for range in &web.trusted_proxies {
    report.check("web.trusted_proxies", cidr(range));
  }
  if let Some(url) = &web.external_url {
    report.check("web.external_url", http_url("web.external_url", url));
  }
  check_authentication(&web.authentication, report);
}

fn check_certificate(certificate: &Certificate, report: &mut Report) {
  let prefix = "web.https.certificate";
  if certificate.cert.is_some() != certificate.key.is_some() {
    report.add(
      &format!("{prefix}.cert"),
      format!("The fields {prefix}.cert and {prefix}.key must be set together."),
    );
  }
  let mut files_readable = true;
  for (field, path) in [
    ("cert", &certificate.cert),
    ("key", &certificate.key),
    ("pfx", &certificate.pfx),
  ] {
    if let Some(path) = path {
      let key = format!("{prefix}.{field}");
      if let Err(message) = readable_file(&key, path) {
        report.add(&key, message);
        files_readable = false;
      }
    }
  }
  if !files_readable {
    return;
  }
  if certificate.pfx.is_some() && (certificate.cert.is_some() || certificate.key.is_some()) {
    report.add(
      &format!("{prefix}.pfx"),
      format!(
        "The HTTPS certificate is invalid. Set {prefix}.cert and {prefix}.key, or {prefix}.pfx, not both."
      ),
    );
    return;
  }
  if let (Some(cert), Some(key)) = (&certificate.cert, &certificate.key) {
    if let Err(message) = load_pem(cert, key) {
      report.add(
        &format!("{prefix}.cert"),
        format!("The HTTPS certificate is invalid: {message}"),
      );
    }
  } else if let Some(pfx) = &certificate.pfx
    && let Err(message) = load_pkcs12(pfx, certificate.password.as_deref().unwrap_or(""))
  {
    report.add(
      &format!("{prefix}.pfx"),
      format!("The HTTPS certificate is invalid: {message}"),
    );
  }
}

/// Reads a PEM certificate chain and its key, for the message when they do not load.
fn load_pem(cert: &Path, key: &Path) -> Result<(), String> {
  let chain = CertificateDer::pem_file_iter(cert)
    .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
    .map_err(|e| format!("{}: {e}", cert.display()))?;
  if chain.is_empty() {
    return Err(format!("{} contains no certificate", cert.display()));
  }
  PrivateKeyDer::from_pem_file(key)
    .map(drop)
    .map_err(|e| format!("{}: {e}", key.display()))
}

/// Reads a PKCS#12 file with its password, for the message when it does not load.
fn load_pkcs12(pfx: &Path, password: &str) -> Result<(), String> {
  let data = fs::read(pfx).map_err(|e| e.to_string())?;
  let store =
    p12_keystore::KeyStore::from_pkcs12(&data, password, p12_keystore::Pkcs12ImportPolicy::Strict)
      .map_err(|e| e.to_string())?;
  match store.private_key_chain() {
    Some((_, chain)) if !chain.certs().is_empty() => Ok(()),
    _ => Err("the file contains no private key for its certificate".to_string()),
  }
}

fn check_authentication(auth: &Authentication, report: &mut Report) {
  let prefix = "web.authentication";
  for (field, value) in [("username", &auth.username), ("password", &auth.password)] {
    let key = format!("{prefix}.{field}");
    report.check(&key, length(&key, Some(value), 1, 255));
  }
  if let Some(text) = &auth.api_key
    && let Err(message) = PrimaryApiKey::parse(text)
  {
    report.add(&format!("{prefix}.api_key"), message);
  }
  if let Some(phc) = &auth.password_hash
    && let Err(e) = local::parses(phc)
  {
    let key = format!("{prefix}.password_hash");
    report.add(
      &key,
      format!("The field {key} is not an argon2 password hash: {e}"),
    );
  }
  let key = format!("{prefix}.local.users");
  for (name, user) in &auth.local.users {
    match (&user.password_hash, &user.password) {
      (Some(phc), _) => {
        if let Err(e) = local::parses(phc) {
          report.add(
            &key,
            format!("The field {key}.{name}.password_hash is not an argon2 password hash: {e}"),
          );
        }
      }
      (None, Some(_)) => {}
      (None, None) => report.add(
        &key,
        format!("The local account {name} requires password_hash or password."),
      ),
    }
  }
  let key = format!("{prefix}.api_keys");
  for (name, api_key) in &auth.api_keys {
    let field = format!("{key}.{name}.key");
    report.check(&key, required(&field, api_key.key.as_deref()));
    report.check(&key, length(&field, api_key.key.as_deref(), 16, 255));
    report.check(&key, cidr_list(&api_key.cidr));
  }
  for range in &auth.trusted_networks {
    report.check(&format!("{prefix}.trusted_networks"), cidr(range));
  }
  for range in &auth.local.only_from {
    report.check(&format!("{prefix}.local.only_from"), cidr(range));
  }
  let key = format!("{prefix}.oidc");
  for (name, provider) in &auth.oidc {
    let field = |f: &str| format!("{key}.{name}.{f}");
    // the name is a path segment of the login URLs and part of a cookie's name
    if name.is_empty()
      || !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
      report.add(
        &key,
        format!(
          "The OpenID Connect provider name {name:?} may contain only letters, digits, - and _."
        ),
      );
    }
    report.check(
      &key,
      required(&field("client_id"), provider.client_id.as_deref()),
    );
    match provider.issuer.as_deref() {
      Some(issuer) if !issuer.trim().is_empty() => {
        report.check(&key, http_url(&field("issuer"), issuer));
        if provider.require_https && !secure_or_local(issuer) {
          report.add(
            &key,
            format!(
              "The field {} must be an https URL unless it is on this machine, or {} must be \
               false.",
              field("issuer"),
              field("require_https")
            ),
          );
        }
      }
      issuer => report.check(&key, required(&field("issuer"), issuer)),
    }
    if let Some(ca) = &provider.ca_certificate {
      report.check(&key, readable_file(&field("ca_certificate"), ca));
    }
  }
}

/// Whether a URL is `https`, or `http` to this machine.
fn secure_or_local(url: &str) -> bool {
  let Ok(url) = url::Url::parse(url) else {
    // reported as not an http(s) URL already
    return true;
  };
  match (url.scheme(), url.host()) {
    ("https", _) => true,
    (_, Some(url::Host::Domain(domain))) => domain.eq_ignore_ascii_case("localhost"),
    (_, Some(url::Host::Ipv4(ip))) => IpAddr::V4(ip).is_loopback(),
    (_, Some(url::Host::Ipv6(ip))) => IpAddr::V6(ip).is_loopback(),
    _ => false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::{ApiKey, Oidc};

  fn problems(web: &Web) -> Vec<(String, String)> {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
      default_downloads: dir.path().join("downloads"),
      default_incomplete: dir.path().join("incomplete"),
      program_dir: dir.path().to_path_buf(),
      started_case_sensitive: false,
    };
    let mut report = Report::default();
    check(web, &context, &mut report);
    report.0.into_iter().map(|i| (i.key, i.message)).collect()
  }

  #[test]
  fn api_key_entries_need_a_key() {
    let mut web = Web::default();
    web
      .authentication
      .api_keys
      .insert("script".into(), ApiKey::default());
    web.authentication.api_keys.insert(
      "short".into(),
      ApiKey {
        key: Some("abc".into()),
        ..ApiKey::default()
      },
    );
    let messages: Vec<String> = problems(&web).into_iter().map(|(_, m)| m).collect();
    assert_eq!(
      messages,
      [
        "The field web.authentication.api_keys.script.key must contain a value.",
        "The field web.authentication.api_keys.short.key must be a string with a minimum length \
         of 16 and a maximum length of 255.",
      ]
    );
  }

  #[test]
  fn local_accounts_need_a_password_and_hashes_must_be_argon2() {
    let mut web = Web::default();
    web.authentication.password_hash = Some("plain text".into());
    web
      .authentication
      .local
      .users
      .insert("nobody".into(), crate::config::LocalUser::default());
    web.authentication.local.users.insert(
      "hashed".into(),
      crate::config::LocalUser {
        password_hash: Some(local::hash("x")),
        ..crate::config::LocalUser::default()
      },
    );
    let messages: Vec<String> = problems(&web).into_iter().map(|(_, m)| m).collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].starts_with("The field web.authentication.password_hash is not"));
    assert_eq!(
      messages[1],
      "The local account nobody requires password_hash or password."
    );
  }

  #[test]
  fn issuers_are_https_unless_local_or_allowed() {
    let mut web = Web::default();
    let provider = |issuer: &str| Oidc {
      issuer: Some(issuer.into()),
      client_id: Some("reseek".into()),
      ..Oidc::default()
    };
    web
      .authentication
      .oidc
      .insert("home".into(), provider("https://auth.example.net/app/"));
    web
      .authentication
      .oidc
      .insert("dev".into(), provider("http://127.0.0.1:9000/"));
    assert!(problems(&web).is_empty());
    web
      .authentication
      .oidc
      .insert("plain".into(), provider("http://auth.example.net/"));
    assert_eq!(problems(&web).len(), 1);
    web
      .authentication
      .oidc
      .insert("two words".into(), provider("https://auth.example.net/"));
    assert_eq!(problems(&web).len(), 2, "a name that can not be a cookie's");
    web.authentication.oidc.remove("two words");
    web
      .authentication
      .oidc
      .get_mut("plain")
      .unwrap()
      .require_https = false;
    assert!(problems(&web).is_empty());
  }

  #[test]
  fn a_pem_certificate_needs_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let cert = dir.path().join("cert.pem");
    fs::write(&cert, "not a certificate").unwrap();
    let mut web = Web::default();
    web.https.certificate.cert = Some(cert.clone());
    let found = problems(&web);
    assert_eq!(found.len(), 1);
    assert!(found[0].1.ends_with("must be set together."));
    web.https.certificate.key = Some(cert);
    let found = problems(&web);
    assert!(
      found[0].1.starts_with("The HTTPS certificate is invalid:"),
      "{found:?}"
    );
  }

  #[test]
  fn a_pkcs12_file_must_load() {
    let dir = tempfile::tempdir().unwrap();
    let pfx = dir.path().join("reseek.pfx");
    fs::write(&pfx, b"garbage").unwrap();
    let mut web = Web::default();
    web.https.certificate.pfx = Some(pfx.clone());
    let found = problems(&web);
    assert_eq!(found.len(), 1);
    assert!(found[0].1.starts_with("The HTTPS certificate is invalid:"));
    web.https.certificate.cert = Some(pfx.clone());
    web.https.certificate.key = Some(pfx);
    let found = problems(&web);
    assert_eq!(found.len(), 1);
    assert!(found[0].1.ends_with("not both."), "{found:?}");
  }
}
