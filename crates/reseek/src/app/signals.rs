//! The process signals: SIGINT and SIGTERM stop the daemon, SIGHUP reloads the configuration.

use cascade::Watcher;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, warn};

use crate::config::Options;

/// Shuts down on the first SIGINT or SIGTERM and exits at once on a second; reloads the
/// configuration on SIGHUP once `started` is sent, which a SIGHUP before then waits for.
pub(super) fn spawn_signals(
  watcher: Watcher<Options>,
  started: oneshot::Receiver<()>,
  cancel: CancellationToken,
) {
  #[cfg(unix)]
  {
    use tokio::signal::unix::{SignalKind, signal};
    let reload = watcher;
    tokio::spawn(
      async move {
        let Ok(mut hangup) = signal(SignalKind::hangup()) else {
          warn!("SIGHUP not handled; reload the configuration through the API instead");
          return;
        };
        // dropped unsent when the daemon stops before it starts
        if started.await.is_err() {
          return;
        }
        while hangup.recv().await.is_some() {
          info!("configuration reload asked for by SIGHUP");
          // a failed reload is logged and kept in the configuration's state where it happens
          drop(reload.reload().await);
        }
      }
      .in_current_span(),
    );
    tokio::spawn(
      async move {
        let (Ok(mut interrupt), Ok(mut terminate)) = (
          signal(SignalKind::interrupt()),
          signal(SignalKind::terminate()),
        ) else {
          error!("shutdown signals not handled");
          return;
        };
        let code = |name: &str| if name == "SIGINT" { 130 } else { 143 };
        let first = tokio::select! {
          _ = interrupt.recv() => "SIGINT",
          _ = terminate.recv() => "SIGTERM",
        };
        info!(signal = first, "Received signal, shutting down");
        cancel.cancel();
        let second = tokio::select! {
          _ = interrupt.recv() => "SIGINT",
          _ = terminate.recv() => "SIGTERM",
        };
        warn!(signal = second, "Received a second signal, exiting at once");
        std::process::exit(code(second));
      }
      .in_current_span(),
    );
  }
  #[cfg(not(unix))]
  {
    drop((watcher, started));
    tokio::spawn(
      async move {
        if tokio::signal::ctrl_c().await.is_ok() {
          info!("Received Ctrl-C, shutting down");
          cancel.cancel();
        }
        if tokio::signal::ctrl_c().await.is_ok() {
          std::process::exit(130);
        }
      }
      .in_current_span(),
    );
  }
}
