//! The HTTP host: the listeners, serving connections on them, and the request pipeline.

pub(crate) mod idle;
mod listen;
pub mod outbound;
pub mod pipeline;
mod tls;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use futures_util::future::{Either, select};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::{GracefulShutdown, Watcher};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tracing::{Instrument, debug, info, warn};

pub use listen::{Listeners, bind};

/// How long a client has to finish the TLS handshake.
const TLS_HANDSHAKE: Duration = Duration::from_secs(10);

/// How long a client has to send a request's headers, counted from the start of the connection
/// for the first request and from the end of the previous one on a kept-alive connection.
const HEADER_READ: Duration = Duration::from_secs(30);

/// How often an HTTP/2 connection is pinged, so one whose client is gone is closed.
const H2_PING: Duration = Duration::from_secs(60);

/// How a connection reached us.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
  /// plain HTTP over TCP
  Plain,
  /// HTTPS
  Tls,
  /// the unix domain socket
  Unix,
}

/// The other end of a connection, put on every request it carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
  /// the peer's address, `None` over the unix socket
  pub remote: Option<SocketAddr>,
  /// how it connected
  pub transport: Transport,
}

impl Peer {
  /// The peer's address, with an IPv4-mapped IPv6 address as the IPv4 one it carries; a
  /// connection over the unix socket comes from `127.0.0.1`.
  pub fn ip(&self) -> IpAddr {
    self.remote.map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |addr| {
      crate::config::cidr::unmapped(addr.ip())
    })
  }
}

/// Serves `app` on every listener until `cancel` is canceled, then lets requests in flight
/// finish for up to `grace`.
pub async fn serve(listeners: Listeners, app: Router, cancel: CancellationToken, grace: Duration) {
  let mut loops = JoinSet::new();
  for accept in listeners.into_accepts() {
    let app = app.clone();
    let cancel = cancel.clone();
    loops.spawn(accept_loop(accept, app, cancel, grace).in_current_span());
  }
  while let Some(result) = loops.join_next().await {
    if let Err(e) = result {
      tracing::error!(error = %e, "an HTTP listener stopped unexpectedly");
    }
  }
  info!("HTTP host stopped");
}

async fn accept_loop(
  mut accept: listen::Accept,
  app: Router,
  cancel: CancellationToken,
  grace: Duration,
) {
  let graceful = GracefulShutdown::new();
  loop {
    let connection = tokio::select! {
      () = cancel.cancelled() => break,
      connection = accept.next() => connection,
    };
    match connection {
      Ok(listen::Connection::Plain(stream, peer)) => {
        let served = serve_connection(graceful.watcher(), stream, peer, app.clone());
        tokio::spawn(served.in_current_span());
      }
      #[cfg(unix)]
      Ok(listen::Connection::Unix(stream)) => {
        let peer = Peer {
          remote: None,
          transport: Transport::Unix,
        };
        let served = serve_connection(graceful.watcher(), stream, peer, app.clone());
        tokio::spawn(served.in_current_span());
      }
      Ok(listen::Connection::Tls(stream, acceptor, peer)) => {
        let app = app.clone();
        let watcher = graceful.watcher();
        tokio::spawn(
          async move {
            match tokio::time::timeout(TLS_HANDSHAKE, acceptor.accept(stream)).await {
              Ok(Ok(stream)) => serve_connection(watcher, stream, peer, app).await,
              Ok(Err(e)) => debug!(remote = ?peer.remote, error = %e, "TLS handshake failed"),
              Err(_) => debug!(remote = ?peer.remote, "TLS handshake timed out"),
            }
          }
          .in_current_span(),
        );
      }
      Err(e) => {
        // accepting fails for reasons of the moment, such as running out of file descriptors
        warn!(error = %e, "accepting an HTTP connection failed");
        tokio::time::sleep(Duration::from_millis(100)).await;
      }
    }
  }
  accept.close();
  if tokio::time::timeout(grace, graceful.shutdown())
    .await
    .is_err()
  {
    warn!(
      ?grace,
      "HTTP connections still open after the shutdown grace period"
    );
  }
}

/// Serves HTTP on `io` until the connection ends, closing it when no request arrives within
/// [`HEADER_READ`]. hyper picks HTTP/1.1 or HTTP/2 from the first bytes, and neither that read
/// nor the HTTP/2 handshake has a timeout of its own; after the first request HTTP/1.1 bounds
/// each request's headers and HTTP/2 pings the client.
async fn serve_connection<IO>(watcher: Watcher, io: IO, peer: Peer, app: Router)
where
  IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
  let requested = Arc::new(Notify::new());
  let connection = builder()
    .serve_connection_with_upgrades(TokioIo::new(io), service(app, peer, requested.clone()))
    .into_owned();
  let connection = std::pin::pin!(watcher.watch(connection));
  let first = std::pin::pin!(tokio::time::timeout(HEADER_READ, requested.notified()));
  let ended = match select(connection, first).await {
    Either::Left((ended, _)) => ended,
    Either::Right((Ok(()), connection)) => connection.await,
    Either::Right((Err(_), _)) => {
      debug!(remote = ?peer.remote, "HTTP connection closed, no request came in time");
      return;
    }
  };
  if let Err(e) = ended {
    debug!(
      remote = ?peer.remote,
      transport = ?peer.transport,
      error = %e,
      "HTTP connection ended with an error"
    );
  }
}

/// The settings of both HTTP versions. `http1_only` and `http2_only` do nothing with
/// `serve_connection_with_upgrades`, which the relay's WebSocket upgrade needs, so the version is
/// always the one the client's first bytes speak.
fn builder() -> auto::Builder<TokioExecutor> {
  let mut builder = auto::Builder::new(TokioExecutor::new());
  // hyper applies its timeouts only with a timer set, and without one has none at all
  builder
    .http1()
    .timer(TokioTimer::new())
    .header_read_timeout(HEADER_READ);
  builder
    .http2()
    .timer(TokioTimer::new())
    .keep_alive_interval(H2_PING);
  builder
}

/// The service for one connection: `app`, with the peer put on each request, telling
/// `requested` of each one.
fn service(
  app: Router,
  peer: Peer,
  requested: Arc<Notify>,
) -> hyper_util::service::TowerToHyperService<
  impl tower::Service<
    hyper::Request<Incoming>,
    Response = axum::response::Response,
    Error = std::convert::Infallible,
    Future = impl Send,
  > + Clone,
> {
  let service = tower::service_fn(move |mut request: hyper::Request<Incoming>| {
    requested.notify_one();
    request.extensions_mut().insert(peer);
    app.clone().oneshot(request.map(axum::body::Body::new))
  });
  hyper_util::service::TowerToHyperService::new(service)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  /// How long the server took to close a connection whose client wrote `sent` and then waited.
  async fn closed_after(sent: &[u8]) -> Duration {
    let (mut client, server) = tokio::io::duplex(4096);
    let peer = Peer {
      remote: None,
      transport: Transport::Plain,
    };
    let graceful = GracefulShutdown::new();
    tokio::spawn(serve_connection(
      graceful.watcher(),
      server,
      peer,
      Router::new(),
    ));
    client.write_all(sent).await.unwrap();
    let started = tokio::time::Instant::now();
    let mut answer = Vec::new();
    client.read_to_end(&mut answer).await.unwrap();
    started.elapsed()
  }

  #[tokio::test(start_paused = true)]
  async fn a_client_that_sends_no_request_is_closed() {
    assert!(closed_after(b"").await >= HEADER_READ, "no bytes at all");
    let preface = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
    assert!(
      closed_after(preface).await >= HEADER_READ,
      "the HTTP/2 preface alone"
    );
    assert!(
      closed_after(b"GET / HT").await >= HEADER_READ,
      "half an HTTP/1.1 request line"
    );
  }
}
