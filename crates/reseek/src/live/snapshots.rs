//! What a stream is sent for each topic when it opens without resuming: the one place each
//! topic's snapshot is made.

use api_types::live::LiveFrame;
use api_types::options::OptionsDocument;

use super::Topic;
use crate::app::AppState;
use crate::config::redact;
use tracing::warn;

/// The snapshot frames of `topic`, in the order they are sent.
pub async fn frames(topic: Topic, app: &AppState) -> Vec<LiveFrame> {
  match topic {
    Topic::Application => vec![
      LiveFrame::ApplicationState(Box::new(api_types::state::State::clone(
        &app.state.current(),
      ))),
      LiveFrame::ApplicationOptions(OptionsDocument(redact::values(&app.config.current()))),
    ],
    Topic::Logs => vec![LiveFrame::LogsBuffer(app.logs.snapshot())],
    Topic::Search => match app.searches.snapshot().await {
      Ok(searches) => vec![LiveFrame::SearchList(searches)],
      Err(e) => {
        warn!(error = %e, "searches not read for a stream's snapshot");
        Vec::new()
      }
    },
    // conversations, rooms and transfer figures are kept by services that send their
    // snapshot here once they exist; until then these topics have none
    Topic::Conversations | Topic::Rooms | Topic::Metrics => Vec::new(),
    // clients fetch these lists themselves
    Topic::SearchResponses(_) | Topic::Transfers | Topic::Users | Topic::Events => Vec::new(),
  }
}
