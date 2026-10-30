//! Binding the listeners: HTTP on each configured address, HTTPS unless it is off, and the
//! unix domain socket when one is configured.

use std::net::{IpAddr, SocketAddr};
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

/// Binds every listener `web` asks for. A listener that cannot be bound fails the start.
pub async fn bind(web: &Web) -> Result<Listeners, String> {
  let mut http = Vec::new();
  for ip in addresses(web.ip_address.as_deref())? {
    let listener = tcp(ip, web.port).await?;
    info!(url = %url("http", &listener), "Listening for HTTP requests");
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
      info!(url = %url("https", &listener), "Listening for HTTPS requests");
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
  /// Where requests are taken: the URL of each HTTP listener, then of each HTTPS one, then
  /// `unix:` and the socket's path.
  pub fn urls(&self) -> Vec<String> {
    let https = self.https.iter().flat_map(|(listeners, _)| listeners);
    let mut urls: Vec<String> = self.http.iter().map(|l| url("http", l)).collect();
    urls.extend(https.map(|l| url("https", l)));
    #[cfg(unix)]
    if let Some((_, path)) = &self.unix {
      urls.push(format!("unix:{}", path.display()));
    }
    urls
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

/// The addresses of a configured list, as `config::addresses::list` reads it.
fn addresses(list: Option<&str>) -> Result<Vec<IpAddr>, String> {
  crate::config::addresses::list(list).map_err(|item| format!("`{item}` is not an IP address"))
}

/// The URL `listener` is reached at, with the port it is bound to, which the system picked
/// when the configuration asked for port 0.
fn url(scheme: &str, listener: &TcpListener) -> String {
  match listener.local_addr() {
    Ok(address) => format!("{scheme}://{address}/"),
    Err(e) => format!("{scheme}://(address unknown: {e})/"),
  }
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
