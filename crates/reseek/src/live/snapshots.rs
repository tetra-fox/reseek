//! What a stream is sent for each topic when it opens without resuming: the one place each
//! topic's snapshot is made.

use api_types::live::LiveFrame;
use api_types::messaging::conversations::ConversationListQuery;
use tracing::warn;

use super::Topic;
use crate::app::AppState;
use crate::config::view;

/// The snapshot frames of `topic`, in the order they are sent.
pub async fn frames(topic: Topic, app: &AppState) -> Vec<LiveFrame> {
  match topic {
    Topic::Application => vec![
      LiveFrame::ApplicationState(Box::new(api_types::state::State::clone(
        &app.state.current(),
      ))),
      LiveFrame::ApplicationOptions(view::document(&app.config.current())),
    ],
    Topic::Logs => vec![LiveFrame::LogsBuffer(app.logs.snapshot())],
    Topic::Activity => vec![LiveFrame::ActivityBuffer(app.activity.snapshot())],
    Topic::Search => match app.searches.snapshot().await {
      Ok(searches) => vec![LiveFrame::SearchList(searches)],
      Err(e) => {
        warn!(error = %e, "searches not read for a stream's snapshot");
        Vec::new()
      }
    },
    Topic::Conversations => {
      let listed = app
        .services
        .messaging
        .conversations(ConversationListQuery::default())
        .await;
      let mut frames = match listed {
        Ok(conversations) => vec![LiveFrame::ConversationsList(conversations)],
        Err(e) => {
          warn!(error = ?e, "conversations not read for a stream's snapshot");
          Vec::new()
        }
      };
      frames.extend(shared(app).await);
      frames
    }
    Topic::Rooms => {
      let mut frames = match app.services.messaging.joined_rooms().await {
        Ok(rooms) => vec![LiveFrame::RoomsList(rooms)],
        Err(e) => {
          warn!(error = ?e, "rooms not read for a stream's snapshot");
          Vec::new()
        }
      };
      frames.extend(shared(app).await);
      frames
    }
    Topic::Jobs => match crate::jobs::schedules::cards(app).await {
      Ok(cards) => vec![LiveFrame::JobsSchedules(cards)],
      Err(e) => {
        warn!(error = ?e, "schedules not read for a stream's snapshot");
        Vec::new()
      }
    },
    Topic::Metrics => vec![LiveFrame::MetricsUpdate(
      app.telemetry.metrics().transfers().figures(),
    )],
    // clients fetch these lists themselves
    Topic::SearchResponses(_)
    | Topic::Transfers
    | Topic::Users
    | Topic::Events
    | Topic::PublicRooms => Vec::new(),
  }
}

/// The pinned chats and the unread counts, the snapshot both chat topics end with.
async fn shared(app: &AppState) -> Vec<LiveFrame> {
  let mut frames = Vec::new();
  match app.services.messaging.pins().await {
    Ok(pins) => frames.push(LiveFrame::ChatPins(pins)),
    Err(e) => warn!(error = ?e, "chat pins not read for a stream's snapshot"),
  }
  match app.messaging.unread_counts().await {
    Ok(counts) => frames.push(LiveFrame::ChatUnread(counts)),
    Err(e) => warn!(error = %e, "unread counts not read for a stream's snapshot"),
  }
  frames
}
