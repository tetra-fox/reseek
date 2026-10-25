//! Refusing cross-site requests: a request that changes something and is authenticated only by
//! the session cookie, which a browser sends on its own, must come from a page of this site,
//! and one let in by a trusted network, which a browser on that network also carries along, may
//! not name another site as its origin.

use axum::http::{HeaderMap, Method, header};
use url::{Origin, Url};

use super::client::Client;
use crate::config::Options;

/// Whether `method` changes something.
pub fn changes_state(method: &Method) -> bool {
  matches!(
    *method,
    Method::POST | Method::PUT | Method::PATCH | Method::DELETE
  )
}

/// Whether the page that sent the request, named by `Origin` or else `Referer`, is this site:
/// the address the client used, `web.external_url`, or one of `web.hostnames`. A request
/// naming neither is not.
pub fn same_origin(headers: &HeaderMap, client: &Client, options: &Options) -> bool {
  let sender = headers
    .get(header::ORIGIN)
    .or_else(|| headers.get(header::REFERER))
    .and_then(|value| value.to_str().ok());
  sender.is_some_and(|text| is_this_site(text, client, options))
}

/// Whether the request carries an `Origin` that is not this site. A request without one, as
/// scripts send, is not refused by this check.
pub fn foreign_origin(headers: &HeaderMap, client: &Client, options: &Options) -> bool {
  match headers.get(header::ORIGIN) {
    Some(value) => !value
      .to_str()
      .is_ok_and(|text| is_this_site(text, client, options)),
    None => false,
  }
}

/// Whether the origin of the URL `text` is the address the client used, `web.external_url`, or
/// one of `web.hostnames`.
fn is_this_site(text: &str, client: &Client, options: &Options) -> bool {
  let Some(sender @ Origin::Tuple(..)) = Url::parse(text).ok().map(|url| url.origin()) else {
    return false;
  };
  let scheme = if client.https { "https" } else { "http" };
  let own = client
    .authority
    .as_deref()
    .and_then(|authority| Url::parse(&format!("{scheme}://{authority}")).ok())
    .map(|url| url.origin());
  if own.as_ref() == Some(&sender) {
    return true;
  }
  let external = options
    .web
    .external_url
    .as_deref()
    .and_then(|url| Url::parse(url).ok())
    .map(|url| url.origin());
  if external.as_ref() == Some(&sender) {
    return true;
  }
  let Origin::Tuple(_, host, _) = &sender else {
    return false;
  };
  let host = host.to_string();
  options
    .web
    .hostnames
    .iter()
    .any(|name| name.eq_ignore_ascii_case(&host))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn client(authority: &str, https: bool) -> Client {
    Client {
      ip: "192.0.2.1".parse().unwrap(),
      host: Some(super::super::client::without_port(authority)),
      authority: Some(authority.to_string()),
      https,
    }
  }

  fn from(name: &'static str, value: &'static str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(name, value.parse().unwrap());
    headers
  }

  #[test]
  fn only_pages_of_this_site_may_send_cookie_requests() {
    let mut options = Options::default();
    let local = client("localhost:5030", false);
    assert!(same_origin(
      &from("origin", "http://localhost:5030"),
      &local,
      &options
    ));
    assert!(same_origin(
      &from("referer", "http://localhost:5030/#/transfers"),
      &local,
      &options
    ));
    assert!(!same_origin(
      &from("origin", "http://localhost:5031"),
      &local,
      &options
    ));
    assert!(!same_origin(
      &from("origin", "https://localhost:5030"),
      &local,
      &options
    ));
    assert!(!same_origin(
      &from("origin", "http://evil.example"),
      &local,
      &options
    ));
    assert!(!same_origin(&from("origin", "null"), &local, &options));
    assert!(!same_origin(&HeaderMap::new(), &local, &options));
    let tls = client("reseek.example.net", true);
    assert!(same_origin(
      &from("origin", "https://reseek.example.net:443"),
      &tls,
      &options
    ));

    options.web.external_url = Some("https://public.example.net/reseek".into());
    options.web.hostnames = vec!["reseek.lan".into()];
    assert!(same_origin(
      &from("origin", "https://public.example.net"),
      &local,
      &options
    ));
    assert!(same_origin(
      &from("origin", "http://Reseek.lan:8080"),
      &local,
      &options
    ));
  }
}
