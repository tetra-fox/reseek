//! The HTTP host: the listeners, serving connections on them, and the request pipeline.

mod listen;
pub mod outbound;
pub mod pipeline;
mod tls;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tracing::{Instrument, debug, info, warn};

pub use listen::{Listeners, bind};

/// How long a client has to finish the TLS handshake.
const TLS_HANDSHAKE: Duration = Duration::from_secs(10);

/// How long a client has to send a request's headers, counted from the end of the previous
/// request on a kept-alive connection.
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
        serve_connection(&graceful, stream, peer, app.clone());
      }
      #[cfg(unix)]
      Ok(listen::Connection::Unix(stream)) => {
        let peer = Peer {
          remote: None,
          transport: Transport::Unix,
        };
        serve_connection(&graceful, stream, peer, app.clone());
      }
      Ok(listen::Connection::Tls(stream, acceptor, peer)) => {
        let app = app.clone();
        let watcher = graceful.watcher();
        tokio::spawn(
          async move {
            match tokio::time::timeout(TLS_HANDSHAKE, acceptor.accept(stream)).await {
              Ok(Ok(stream)) => {
                let h2 = stream.get_ref().1.alpn_protocol() == Some(b"h2".as_slice());
                let connection = builder(h2)
                  .serve_connection_with_upgrades(TokioIo::new(stream), service(app, peer))
                  .into_owned();
                if let Err(e) = watcher.watch(connection).await {
                  debug!(error = %e, "HTTPS connection ended with an error");
                }
              }
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

fn serve_connection<IO>(graceful: &GracefulShutdown, io: IO, peer: Peer, app: Router)
where
  IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
  let connection = builder(false)
    .serve_connection_with_upgrades(TokioIo::new(io), service(app, peer))
    .into_owned();
  let connection = graceful.watch(connection);
  tokio::spawn(
    async move {
      if let Err(e) = connection.await {
        debug!(remote = ?peer.remote, error = %e, "HTTP connection ended with an error");
      }
    }
    .in_current_span(),
  );
}

/// HTTP/2 when the TLS handshake chose it, else HTTP/1.1. The version comes from ALPN rather
/// than from the first bytes, since hyper waits for those bytes without a timeout.
fn builder(h2: bool) -> auto::Builder<TokioExecutor> {
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
  if h2 {
    builder.http2_only()
  } else {
    builder.http1_only()
  }
}

/// The service for one connection: `app`, with the peer put on each request.
fn service(
  app: Router,
  peer: Peer,
) -> hyper_util::service::TowerToHyperService<
  impl tower::Service<
    hyper::Request<Incoming>,
    Response = axum::response::Response,
    Error = std::convert::Infallible,
    Future = impl Send,
  > + Clone,
> {
  let service = tower::service_fn(move |mut request: hyper::Request<Incoming>| {
    request.extensions_mut().insert(peer);
    app.clone().oneshot(request.map(axum::body::Body::new))
  });
  hyper_util::service::TowerToHyperService::new(service)
}
