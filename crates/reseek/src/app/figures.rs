//! The figures the daemon keeps current from its metrics: both directions' transfer figures,
//! pushed every second to the streams that follow them, and the health of incoming searches
//! and the process's memory, copied into the state every 30 seconds.

use api_types::live::LiveFrame;
use api_types::state::IncomingSearchHealth;
use tracing::trace;

use super::AppState;
use crate::live::Topic;
use crate::state::StateOwner;
use crate::telemetry::Telemetry;

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
  copy_health(&app.telemetry, app.network.searches_waiting(), &app.state).await;
}

/// [`sample_health`] from `telemetry`, with `waiting` searches waiting, into `state`.
async fn copy_health(telemetry: &Telemetry, waiting: usize, state: &StateOwner) {
  let search = &telemetry.metrics().search;
  let incoming = IncomingSearchHealth {
    latency: search.response_latency_current.current(),
    queue_depth: u32::try_from(waiting).unwrap_or(u32::MAX),
    drop_rate: search.request_drop_rate.current() as f64,
  };
  let memory = telemetry.resident_memory().await;
  trace!(?incoming, memory, "health sampled");
  state.update(|state| {
    state.health.search.incoming = incoming;
    if let Some(memory) = memory {
      state.process.memory_bytes = memory;
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn the_health_sample_copies_the_search_figures_and_the_memory() {
    let telemetry = Telemetry::new(
      crate::transfers::fixtures::transfers().await,
      tokio_util::sync::CancellationToken::new(),
    );
    let owner = crate::testing::state();
    let search = &telemetry.metrics().search;
    search.response_latency_current.update(12.5);
    search.request_drop_rate.count_up(3);
    copy_health(&telemetry, 2, &owner).await;
    let state = owner.current();
    assert_eq!(
      state.health.search.incoming,
      IncomingSearchHealth {
        latency: 12.5,
        queue_depth: 2,
        drop_rate: 3.0,
      }
    );
    if cfg!(target_os = "linux") {
      assert!(state.process.memory_bytes > 0);
    }
    search.request_drop_rate.elapse();
    copy_health(&telemetry, 0, &owner).await;
    assert_eq!(
      owner.current().health.search.incoming.drop_rate,
      0.0,
      "the minute's count, not the last whole minute's"
    );
  }
}
