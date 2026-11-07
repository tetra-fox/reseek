//! The figures the daemon keeps current from its metrics: both directions' transfer figures,
//! pushed every second to the streams that follow them, and the health of incoming searches
//! and the process's memory, copied into the state every 30 seconds.

use api_types::live::LiveFrame;
use api_types::state::IncomingSearchHealth;
use tracing::trace;

use super::AppState;
use crate::live::Topic;

/// Pushes the transfer figures while a stream follows them; the hub sends them only when they
/// changed.
pub(super) fn push_transfers(app: &AppState) {
  if app.live.followed(Topic::Metrics) {
    let figures = app.telemetry.metrics().transfers().figures();
    app.live.publish(LiveFrame::MetricsUpdate(figures));
  }
}

/// Copies into the state how incoming searches are answered now: the moving average of the
/// time to answer one, how many wait, and how many were dropped in the minute so far; and the
/// memory the process holds, which stays as it was when the system does not say.
pub(super) async fn sample_health(app: &AppState) {
  let search = &app.telemetry.metrics().search;
  let incoming = IncomingSearchHealth {
    latency: search.response_latency_current.current(),
    queue_depth: u32::try_from(app.network.searches_waiting()).unwrap_or(u32::MAX),
    drop_rate: search.request_drop_rate.current() as f64,
  };
  let memory = app.telemetry.resident_memory().await;
  trace!(?incoming, memory, "health sampled");
  app.state.update(|state| {
    state.health.search.incoming = incoming;
    if let Some(memory) = memory {
      state.process.memory_bytes = memory;
    }
  });
}
