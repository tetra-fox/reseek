//! Events: the record of something that happened, as the event log stores it, scripts receive
//! it and `events.raised` frames carry it, with the data of every event type and the query of
//! the event log.

mod data;

pub use data::*;

use std::borrow::Cow;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// Declares the event catalog once: `EventType` with a variant per type and `Event` with the
/// same variants carrying their data, so a type name and its data cannot drift apart.
macro_rules! event_catalog {
  ($($(#[$doc:meta])* $name:ident($data:ty),)*) => {
    /// What kind of event something is, written as its name and read ignoring case.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, ToSchema)]
    pub enum EventType {
      $($(#[$doc])* $name,)*
    }

    /// An event's type with its data, written as `type` and a `data` object.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
    #[serde(tag = "type", content = "data")]
    pub enum Event {
      $($(#[$doc])* $name($data),)*
    }

    impl EventType {
      /// Every event type, in catalog order.
      pub const ALL: &'static [EventType] = &[$(EventType::$name,)*];

      /// The type's name as it is written, such as `DownloadFileComplete`.
      pub fn name(self) -> &'static str {
        match self {
          $(EventType::$name => stringify!($name),)*
        }
      }
    }

    impl Event {
      /// The event's type.
      pub fn event_type(&self) -> EventType {
        match self {
          $(Event::$name(_) => EventType::$name,)*
        }
      }
    }
  };
}

event_catalog! {
  /// a download succeeded and its file was moved into the downloads directory
  DownloadFileComplete(DownloadFileComplete),
  /// the last pending download from a user in one remote directory succeeded
  DownloadDirectoryComplete(DownloadDirectoryComplete),
  /// an upload succeeded
  UploadFileComplete(UploadFileComplete),
  /// a private message arrived, or the server delivered an unacknowledged one again
  PrivateMessageReceived(PrivateMessageReceived),
  /// a message was said in a room we are in, our own included
  RoomMessageReceived(RoomMessageReceivedEvent),
  /// a user was blocked
  UserBlocked(UserBlocked),
  /// a user's block was removed or expired
  UserUnblocked(UserUnblocked),
  /// an API token was created
  TokenCreated(TokenChanged),
  /// an API token was revoked
  TokenRevoked(TokenChanged),
  /// someone signed in and got a browser session
  SessionCreated(SessionCreated),
  /// a browser session ended before it expired, or expired
  SessionRevoked(SessionRevoked),
  /// a sign-in was refused
  LoginFailed(LoginFailed),
  /// an audio analysis the user asked for finished
  FileAnalysisComplete(FileAnalysisComplete),
  /// a download failed for good, or its file could not be moved into the downloads directory
  DownloadFileFailed(DownloadFileFailed),
  /// a script ran for an event; never delivered to scripts
  ScriptRunCompleted(ScriptRunCompleted),
  /// the connection to the Soulseek server is up, before login
  SoulseekClientConnected(NoFields),
  /// the connection to the Soulseek server was lost or closed
  SoulseekClientDisconnected(SoulseekClientDisconnected),
  /// a relay agent logged in to this controller
  RelayAgentConnected(RelayAgentConnected),
  /// a relay agent's socket closed
  RelayAgentDisconnected(RelayAgentDisconnected),
  /// a relay agent's shares were loaded
  RelayAgentSharesUpdated(RelayAgentSharesUpdated),
  /// a test event with no fields, for trying scripts
  Noop(NoFields),
}

impl std::str::FromStr for EventType {
  type Err = String;

  /// Reads a type name ignoring case.
  fn from_str(text: &str) -> Result<Self, String> {
    Self::ALL
      .iter()
      .copied()
      .find(|kind| kind.name().eq_ignore_ascii_case(text))
      .ok_or_else(|| {
        let names = Self::ALL.iter().map(|kind| kind.name()).collect::<Vec<_>>();
        format!(
          "Unknown event type '{text}'. It must be one of: {}.",
          names.join(", ")
        )
      })
  }
}

impl<'de> Deserialize<'de> for EventType {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let text = <Cow<'de, str>>::deserialize(deserializer)?;
    text.parse().map_err(serde::de::Error::custom)
  }
}

/// One event as the event log keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventRecord {
  /// unique per event; consumers drop repeats by it
  pub id: Uuid,
  /// when it happened, which for a finished transfer is when the transfer ended
  pub timestamp: DateTime<Utc>,
  /// its type and data
  #[serde(flatten)]
  pub event: Event,
}

/// The query of `GET /events`, which lists the event log newest first, or oldest first after
/// `since`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
  /// how many matching events to skip, 0 when left out
  #[serde(default)]
  pub offset: u32,
  /// how many events to return at most, at least 1; 100 when left out
  #[serde(default = "default_limit")]
  #[param(minimum = 1)]
  pub limit: u32,
  /// only events of these types: comma-separated names in any case; every type when left out
  #[serde(
    default,
    rename = "type",
    deserialize_with = "event_types",
    serialize_with = "write_event_types"
  )]
  #[param(value_type = Option<String>, example = "DownloadFileComplete,DownloadFileFailed")]
  pub types: Vec<EventType>,
  /// only events whose data has this `username`
  #[serde(default)]
  pub username: Option<String>,
  /// only events after this time, oldest first
  #[serde(default)]
  pub since: Option<DateTime<Utc>>,
  /// only events before this time, newest first
  #[serde(default)]
  pub before: Option<DateTime<Utc>>,
}

fn default_limit() -> u32 {
  100
}

/// Reads a comma-separated list of event type names, skipping empty entries.
fn event_types<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<EventType>, D::Error> {
  let text = <Cow<'de, str>>::deserialize(deserializer)?;
  text
    .split(',')
    .map(str::trim)
    .filter(|name| !name.is_empty())
    .map(str::parse)
    .collect::<Result<_, _>>()
    .map_err(serde::de::Error::custom)
}

/// Writes event types as the comma-separated list `event_types` reads.
fn write_event_types<S: serde::Serializer>(
  types: &[EventType],
  serializer: S,
) -> Result<S::Ok, S::Error> {
  let names = types.iter().map(|kind| kind.name()).collect::<Vec<_>>();
  serializer.serialize_str(&names.join(","))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn event_type_names_are_read_ignoring_case_and_written_as_named() {
    for &kind in EventType::ALL {
      let json = serde_json::to_string(&kind).unwrap();
      assert_eq!(json, format!("\"{}\"", kind.name()));
      let lower = format!("\"{}\"", kind.name().to_lowercase());
      assert_eq!(serde_json::from_str::<EventType>(&lower).unwrap(), kind);
    }
    let error = serde_json::from_str::<EventType>("\"Any\"").unwrap_err();
    assert!(error.to_string().contains("Unknown event type 'Any'"));
  }

  #[test]
  fn events_query_reads_a_comma_separated_type_list() {
    let query: EventsQuery =
      serde_json::from_str(r#"{"type":"downloadfilecomplete, DownloadFileFailed,"}"#).unwrap();
    assert_eq!(
      query.types,
      [
        EventType::DownloadFileComplete,
        EventType::DownloadFileFailed
      ]
    );
    assert_eq!(query.limit, 100);
    assert_eq!(
      serde_json::to_value(&query).unwrap()["type"],
      "DownloadFileComplete,DownloadFileFailed"
    );
    assert!(serde_json::from_str::<EventsQuery>(r#"{"type":"Bogus"}"#).is_err());
    let all: EventsQuery = serde_json::from_str("{}").unwrap();
    assert!(all.types.is_empty());
  }
}
