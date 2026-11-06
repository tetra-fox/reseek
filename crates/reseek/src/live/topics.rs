//! The topics a stream asks for, who may ask for each, and which topics each frame goes to.

use std::fmt;
use std::str::FromStr;

use api_types::auth::Role;
use api_types::live::LiveFrame;
use uuid::Uuid;

/// A topic of the event stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Topic {
  /// the application state and the options
  Application,
  /// the log, for administrators
  Logs,
  /// transfer figures
  Metrics,
  /// the searches themselves
  Search,
  /// the responses of one search
  SearchResponses(Uuid),
  /// downloads and uploads
  Transfers,
  /// jobs and the schedules
  Jobs,
  /// private messages
  Conversations,
  /// chat rooms
  Rooms,
  /// every message in every public room, which the server sends only while a stream follows it
  PublicRooms,
  /// other users
  Users,
  /// raised events
  Events,
  /// what other users did with us
  Activity,
}

impl Topic {
  /// The least role that may follow the topic.
  pub fn role(self) -> Role {
    match self {
      Topic::Logs => Role::Administrator,
      _ => Role::ReadOnly,
    }
  }
}

impl FromStr for Topic {
  type Err = String;

  fn from_str(text: &str) -> Result<Self, String> {
    Ok(match text {
      "application" => Topic::Application,
      "logs" => Topic::Logs,
      "metrics" => Topic::Metrics,
      "search" => Topic::Search,
      "transfers" => Topic::Transfers,
      "jobs" => Topic::Jobs,
      "conversations" => Topic::Conversations,
      "rooms" => Topic::Rooms,
      "public_rooms" => Topic::PublicRooms,
      "users" => Topic::Users,
      "events" => Topic::Events,
      "activity" => Topic::Activity,
      other => match other.strip_prefix("search:") {
        Some(id) => Topic::SearchResponses(
          id.parse()
            .map_err(|_| format!("topic {other} does not name a search by its id"))?,
        ),
        None => return Err(format!("Unknown topic {other}")),
      },
    })
  }
}

impl fmt::Display for Topic {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Topic::Application => f.write_str("application"),
      Topic::Logs => f.write_str("logs"),
      Topic::Metrics => f.write_str("metrics"),
      Topic::Search => f.write_str("search"),
      Topic::SearchResponses(id) => write!(f, "search:{id}"),
      Topic::Transfers => f.write_str("transfers"),
      Topic::Jobs => f.write_str("jobs"),
      Topic::Conversations => f.write_str("conversations"),
      Topic::Rooms => f.write_str("rooms"),
      Topic::PublicRooms => f.write_str("public_rooms"),
      Topic::Users => f.write_str("users"),
      Topic::Events => f.write_str("events"),
      Topic::Activity => f.write_str("activity"),
    }
  }
}

/// The topics of a `topics` query, in the order given and without repeats; `application` when
/// it names none.
pub fn parse(list: Option<&str>) -> Result<Vec<Topic>, String> {
  let mut topics = Vec::new();
  for text in list.unwrap_or("").split(',').map(str::trim) {
    if text.is_empty() {
      continue;
    }
    let topic: Topic = text.parse()?;
    if !topics.contains(&topic) {
      topics.push(topic);
    }
  }
  if topics.is_empty() {
    topics.push(Topic::Application);
  }
  Ok(topics)
}

/// The topics `frame` goes to; none for the control frames, which streams write themselves.
pub fn routes(frame: &LiveFrame) -> Vec<Topic> {
  use LiveFrame as F;
  let topic = match frame {
    F::Hello(_) | F::Lagged(_) | F::Bye(_) => return Vec::new(),
    F::ChatUnread(_) | F::ChatPins(_) => return vec![Topic::Conversations, Topic::Rooms],
    F::UsersPresence(_) => return vec![Topic::Users, Topic::Application],
    F::SearchWishlistFound(_) => return vec![Topic::Search, Topic::Application],
    F::SearchResponse(received) => Topic::SearchResponses(received.search_id),
    F::ApplicationState(_) | F::ApplicationOptions(_) | F::ApplicationServerMessage(_) => {
      Topic::Application
    }
    F::LogsBuffer(_) | F::LogsRecord(_) => Topic::Logs,
    F::MetricsUpdate(_) => Topic::Metrics,
    F::SearchList(_) | F::SearchCreate(_) | F::SearchUpdate(_) | F::SearchDelete(_) => {
      Topic::Search
    }
    F::TransfersAdded(_)
    | F::TransfersUpdated(_)
    | F::TransfersRemoved(_)
    | F::TransfersJobFinished(_)
    | F::FilesAnalysisQueued(_)
    | F::FilesAnalysis(_) => Topic::Transfers,
    F::JobsUpdated(_) | F::JobsRemoved(_) | F::JobsSchedules(_) => Topic::Jobs,
    F::ConversationsList(_) | F::ConversationsMessage(_) | F::ConversationsUpdate(_) => {
      Topic::Conversations
    }
    F::RoomsList(_)
    | F::RoomsUpdate(_)
    | F::RoomsMessage(_)
    | F::RoomsUserJoined(_)
    | F::RoomsUserLeft(_)
    | F::RoomsUserStatus(_)
    | F::RoomsGap(_) => Topic::Rooms,
    F::RoomsPublicMessage(_) => Topic::PublicRooms,
    F::UsersStatus(_) | F::UsersBrowseProgress(_) | F::UsersUpdated(_) | F::UsersRemoved(_) => {
      Topic::Users
    }
    F::EventsRaised(_) => Topic::Events,
    F::ActivityBuffer(_) | F::ActivityEntry(_) => Topic::Activity,
  };
  vec![topic]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn topics_parse_in_order_without_repeats_and_default_to_application() {
    let id = Uuid::from_u128(7);
    assert_eq!(
      parse(Some(&format!("logs, application,logs,search:{id}"))).unwrap(),
      [Topic::Logs, Topic::Application, Topic::SearchResponses(id)]
    );
    assert_eq!(parse(None).unwrap(), [Topic::Application]);
    assert_eq!(parse(Some(" , ")).unwrap(), [Topic::Application]);
    assert_eq!(parse(Some("hubs")).unwrap_err(), "Unknown topic hubs");
    assert!(parse(Some("search:7")).is_err());
    assert_eq!(
      Topic::SearchResponses(id).to_string(),
      format!("search:{id}")
    );
  }
}
