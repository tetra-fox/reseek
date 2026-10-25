//! The socket peers connect to us on, and the loop that accepts their connections. A run of
//! accept failures (running out of file descriptors, say) slows the loop down instead of
//! spinning it.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::Instrument;

use crate::error::ClientError;
use crate::log::LogLimiter;

/// consecutive accept failures after which the loop pauses between attempts
const ERRORS_BEFORE_BACKOFF: u32 = 20;
const BACKOFF: Duration = Duration::from_secs(1);

static ACCEPT_ERRORS: LogLimiter = LogLimiter::new();

/// A bound listening socket with its accept loop running. Dropping it stops the loop and closes
/// the socket.
pub(crate) struct Listener {
  address: SocketAddr,
  _stop: DropGuard,
  task: JoinHandle<()>,
}

impl Listener {
  /// Binds `address` and starts accepting, handing every accepted socket to `on_accept`.
  pub(crate) fn start(
    address: SocketAddr,
    backlog: u32,
    on_accept: impl FnMut(TcpStream, SocketAddr) + Send + 'static,
  ) -> Result<Self, ClientError> {
    let listener = bind(address, backlog).map_err(|error| ClientError::Listen {
      message: format!(
        "Failed to start listening on {address}; the IP and/or port may be in use or are otherwise unavailable"
      ),
      source: Some(std::sync::Arc::new(error)),
    })?;
    let stop = CancellationToken::new();
    let span = tracing::info_span!("listener", %address);
    span.in_scope(|| tracing::info!("listening for peer connections"));
    let task = tokio::spawn(accept_loop(listener, stop.clone(), on_accept).instrument(span));
    Ok(Self {
      address,
      _stop: stop.drop_guard(),
      task,
    })
  }

  /// stops accepting and waits until the socket is closed, so the address can be bound again
  pub(crate) async fn stop(self) {
    let Self {
      address,
      _stop,
      task,
    } = self;
    drop(_stop);
    if let Err(error) = task.await {
      tracing::error!(%address, %error, "listener task failed");
    }
    tracing::info!(%address, "stopped listening for peer connections");
  }
}

fn bind(address: SocketAddr, backlog: u32) -> std::io::Result<TcpListener> {
  let socket = match address {
    SocketAddr::V4(_) => TcpSocket::new_v4()?,
    SocketAddr::V6(_) => TcpSocket::new_v6()?,
  };
  // lets the port be bound again while connections from the previous listener linger in
  // TIME_WAIT; on windows the option would let another process take the port instead
  #[cfg(not(windows))]
  socket.set_reuseaddr(true)?;
  socket.bind(address)?;
  socket.listen(backlog)
}

async fn accept_loop(
  listener: TcpListener,
  stop: CancellationToken,
  mut on_accept: impl FnMut(TcpStream, SocketAddr),
) {
  let mut errors = 0;
  loop {
    let accepted = tokio::select! {
      biased;
      () = stop.cancelled() => return,
      accepted = listener.accept() => accepted,
    };
    match accepted {
      Ok((stream, remote)) => {
        errors = 0;
        tracing::trace!(%remote, "connection accepted");
        on_accept(stream, remote);
      }
      Err(error) => {
        errors += 1;
        if let Some(suppressed) = ACCEPT_ERRORS.allow() {
          tracing::warn!(
            %error,
            consecutive = errors,
            suppressed,
            "Failed to establish an incoming connection"
          );
        }
        if errors >= ERRORS_BEFORE_BACKOFF {
          tokio::select! {
            () = stop.cancelled() => return,
            () = tokio::time::sleep(BACKOFF) => {}
          }
        }
      }
    }
  }
}
