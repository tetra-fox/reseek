//! Binding the listeners: HTTP on each configured address, HTTPS unless it is off, and the
//! unix domain socket when one is configured.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tracing::info;

use super::{Peer, Transport, tls};
use crate::config::Web;

/// Every bound listener, not yet accepting.
pub struct Listeners {
  http: Vec<TcpListener>,
  https: Option<(Vec<TcpListener>, TlsAcceptor)>,
  #[cfg(unix)]
  unix: Option<(tokio::net::UnixListener, PathBuf)>,
}

/// One listener, accepting.
pub(super) enum Accept {
  Http(TcpListener),
  Https(TcpListener, TlsAcceptor),
  #[cfg(unix)]
  Unix(tokio::net::UnixListener, PathBuf),
}

/// A new connection.
pub(super) enum Connection {
  Plain(TcpStream, Peer),
  Tls(TcpStream, TlsAcceptor, Peer),
  #[cfg(unix)]
  Unix(tokio::net::UnixStream),
}

/// Binds every listener `web` asks for. A listener that can not be bound fails the start.
pub async fn bind(web: &Web) -> Result<Listeners, String> {
  let mut http = Vec::new();
  for ip in addresses(web.ip_address.as_deref())? {
    let listener = tcp(ip, web.port).await?;
    info!(url = %url("http", ip, web.port), "Listening for HTTP requests");
    http.push(listener);
  }
  let https = if web.https.disabled {
    None
  } else {
    let ips = addresses(web.https.ip_address.as_deref())?;
    let acceptor = TlsAcceptor::from(Arc::new(tls::server_config(&web.https, &ips)?));
    let mut listeners = Vec::new();
    for ip in ips {
      let listener = tcp(ip, web.https.port).await?;
      info!(url = %url("https", ip, web.https.port), "Listening for HTTPS requests");
      listeners.push(listener);
    }
    Some((listeners, acceptor))
  };
  #[cfg(unix)]
  let unix = match &web.socket {
    Some(path) => Some((unix_socket(path)?, path.clone())),
    None => None,
  };
  #[cfg(not(unix))]
  if let Some(path) = &web.socket {
    tracing::warn!(path = %path.display(), "unix domain sockets are not supported on this platform; web.socket ignored");
  }
  Ok(Listeners {
    http,
    https,
    #[cfg(unix)]
    unix,
  })
}

impl Listeners {
  /// The address each TCP listener is bound to, HTTP first.
  pub fn local_addrs(&self) -> Vec<SocketAddr> {
    let https = self.https.iter().flat_map(|(listeners, _)| listeners);
    self
      .http
      .iter()
      .chain(https)
      .filter_map(|l| l.local_addr().ok())
      .collect()
  }

  pub(super) fn into_accepts(self) -> Vec<Accept> {
    let mut accepts: Vec<Accept> = self.http.into_iter().map(Accept::Http).collect();
    if let Some((listeners, acceptor)) = self.https {
      accepts.extend(
        listeners
          .into_iter()
          .map(|l| Accept::Https(l, acceptor.clone())),
      );
    }
    #[cfg(unix)]
    if let Some((listener, path)) = self.unix {
      accepts.push(Accept::Unix(listener, path));
    }
    accepts
  }
}

impl Accept {
  pub(super) async fn next(&mut self) -> std::io::Result<Connection> {
    match self {
      Self::Http(listener) => {
        let (stream, remote) = listener.accept().await?;
        Ok(Connection::Plain(
          stream,
          Peer {
            remote: Some(remote),
            transport: Transport::Plain,
          },
        ))
      }
      Self::Https(listener, acceptor) => {
        let (stream, remote) = listener.accept().await?;
        let peer = Peer {
          remote: Some(remote),
          transport: Transport::Tls,
        };
        Ok(Connection::Tls(stream, acceptor.clone(), peer))
      }
      #[cfg(unix)]
      Self::Unix(listener, _) => {
        let (stream, _) = listener.accept().await?;
        Ok(Connection::Unix(stream))
      }
    }
  }

  /// Stops listening; the unix socket's file is removed.
  pub(super) fn close(self) {
    #[cfg(unix)]
    if let Self::Unix(listener, path) = self {
      drop(listener);
      if let Err(e) = std::fs::remove_file(&path) {
        tracing::warn!(path = %path.display(), error = %e, "unix domain socket file not removed");
      }
    }
  }
}

/// The addresses of a comma-separated list, each trimmed and with or without brackets; every
/// address (`[::]`, which takes IPv4 too) when the list is not set or blank.
pub fn addresses(list: Option<&str>) -> Result<Vec<IpAddr>, String> {
  let Some(list) = list.filter(|l| !l.trim().is_empty()) else {
    return Ok(vec![IpAddr::V6(Ipv6Addr::UNSPECIFIED)]);
  };
  list
    .split(',')
    .map(|item| {
      let item = item.trim();
      let bare = item
        .strip_prefix('[')
        .and_then(|i| i.strip_suffix(']'))
        .unwrap_or(item);
      bare
        .parse()
        .map_err(|_| format!("`{item}` is not an IP address"))
    })
    .collect()
}

fn url(scheme: &str, ip: IpAddr, port: u16) -> String {
  format!("{scheme}://{}/", SocketAddr::new(ip, port))
}

async fn tcp(ip: IpAddr, port: u16) -> Result<TcpListener, String> {
  let addr = SocketAddr::new(ip, port);
  TcpListener::bind(addr)
    .await
    .map_err(|e| format!("could not listen on {addr}: {e}"))
}

/// Binds the unix socket at `path` with mode 0660. A socket file nothing listens on any more is
/// removed first; one something still listens on fails the bind.
#[cfg(unix)]
fn unix_socket(path: &std::path::Path) -> Result<tokio::net::UnixListener, String> {
  use std::os::unix::fs::PermissionsExt;
  if path.exists() {
    match std::os::unix::net::UnixStream::connect(path) {
      Ok(_) => {
        return Err(format!(
          "could not listen on {}: something is listening there",
          path.display()
        ));
      }
      Err(_) => {
        info!(path = %path.display(), "removing a stale unix domain socket");
        std::fs::remove_file(path)
          .map_err(|e| format!("could not remove the stale socket {}: {e}", path.display()))?;
      }
    }
  }
  let listener = tokio::net::UnixListener::bind(path)
    .map_err(|e| format!("could not listen on {}: {e}", path.display()))?;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))
    .map_err(|e| format!("could not set the mode of {}: {e}", path.display()))?;
  info!(path = %path.display(), "Listening for HTTP requests on unix domain socket (UDS)");
  Ok(listener)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn address_lists_are_trimmed_and_default_to_every_address() {
    assert_eq!(
      addresses(None).unwrap(),
      [IpAddr::V6(Ipv6Addr::UNSPECIFIED)]
    );
    assert_eq!(addresses(Some("  ")).unwrap().len(), 1);
    assert_eq!(
      addresses(Some("127.0.0.1, [::1]")).unwrap(),
      [
        "127.0.0.1".parse::<IpAddr>().unwrap(),
        "::1".parse().unwrap()
      ]
    );
    assert!(addresses(Some("localhost")).is_err());
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn a_stale_socket_file_is_replaced_and_a_live_one_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reseek.sock");
    let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
    drop(stale);
    let live = unix_socket(&path).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
      0o660
    );
    assert!(unix_socket(&path).unwrap_err().contains("listening"));
    drop(live);
  }
}
