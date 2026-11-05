//! Sample events, raised to try a script without waiting for the real thing. Every text in a
//! sample starts with the caller's disambiguator `d`.
//!
//! The transfer, directory, message and client events have their own sample data. Every other
//! type follows one rule: a text is `d` and the field's name, an id is the zero id, a time is
//! now, a number 0, a flag false, a choice its first value, a list empty, and an object is
//! made by the same rule.

use api_types::analysis::{
  AnalysisMeasures, AnalysisOverride, AnalysisVerdict, AudioAnalysis, ClaimedFormat, EncoderGuess,
  Judgment, SpectrogramLayout, SpectrogramScale,
};
use api_types::auth::{Role, Via};
use api_types::events::{
  DownloadDirectoryComplete, DownloadFailureStage, DownloadFileComplete, DownloadFileFailed, Event,
  EventType, FileAnalysisComplete, LoginFailed, NoFields, PrivateMessageReceived,
  RelayAgentConnected, RelayAgentDisconnected, RelayAgentSharesUpdated, RoomMessageReceivedEvent,
  SessionCreated, SessionRevokeReason, SessionRevoked, SoulseekClientDisconnected, TokenChanged,
  UnblockReason, UploadFileComplete, UserBlocked, UserUnblocked, BuddyPresenceChanged,
};
use api_types::files::FileRoot;
use api_types::messaging::conversations::{MessageDirection, PrivateMessage};
use api_types::messaging::rooms::RoomMessage;
use api_types::transfers::{Transfer, TransferDirection, TransferStates};
use api_types::users::UserPresence;
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

/// A sample of `kind` made with the disambiguator `d` at `now`; `None` for the types no sample
/// is made of.
pub fn sample(kind: EventType, d: &str, now: DateTime<Utc>) -> Option<Event> {
  let text = |field: &str| format!("{d}{field}");
  Some(match kind {
    EventType::DownloadFileComplete => Event::DownloadFileComplete(DownloadFileComplete {
      version: 0,
      local_filename: text("local.file"),
      remote_filename: text("remote.file"),
      transfer: blank_transfer(),
    }),
    EventType::UploadFileComplete => Event::UploadFileComplete(UploadFileComplete {
      version: 0,
      local_filename: text("local.file"),
      remote_filename: text("remote.file"),
      transfer: blank_transfer(),
    }),
    EventType::DownloadDirectoryComplete => {
      Event::DownloadDirectoryComplete(DownloadDirectoryComplete {
        version: 0,
        local_directory_name: text("local.directory"),
        remote_directory_name: text("remote.directory"),
        username: text("username"),
      })
    }
    EventType::DownloadFileFailed => Event::DownloadFileFailed(DownloadFileFailed {
      version: 0,
      stage: DownloadFailureStage::Download,
      reason: text("reason"),
      local_filename: Some(text("local.file")),
      remote_filename: text("remote.file"),
      transfer: blank_transfer(),
    }),
    EventType::PrivateMessageReceived => Event::PrivateMessageReceived(PrivateMessageReceived {
      version: 0,
      message: PrivateMessage {
        local_id: 0,
        id: Some(42),
        timestamp: now,
        username: text("username"),
        direction: MessageDirection::In,
        message: text("message"),
        is_acknowledged: false,
        was_replayed: false,
        auto_reply: false,
      },
    }),
    EventType::RoomMessageReceived => Event::RoomMessageReceived(RoomMessageReceivedEvent {
      version: 0,
      message: RoomMessage {
        local_id: 0,
        timestamp: now,
        username: text("username"),
        message: text("message"),
        room_name: text("room"),
        is_self: false,
        is_mention: false,
        hidden: false,
      },
    }),
    EventType::SoulseekClientConnected => Event::SoulseekClientConnected(NoFields { version: 0 }),
    EventType::SoulseekClientDisconnected => {
      Event::SoulseekClientDisconnected(SoulseekClientDisconnected {
        version: 0,
        message: text("message"),
        exception: None,
      })
    }
    EventType::Noop => Event::Noop(NoFields { version: 0 }),
    EventType::UserBlocked => Event::UserBlocked(UserBlocked {
      version: 0,
      username: text("username"),
      reason: Some(text("reason")),
      scopes: Vec::new(),
      expires_at: Some(now),
      by: text("by"),
    }),
    EventType::UserUnblocked => Event::UserUnblocked(UserUnblocked {
      version: 0,
      username: text("username"),
      reason: UnblockReason::Manual,
      by: Some(text("by")),
    }),
    EventType::BuddyPresenceChanged => Event::BuddyPresenceChanged(BuddyPresenceChanged {
      version: 0,
      username: text("username"),
      presence: UserPresence::Online,
      previous: UserPresence::Offline,
    }),
    EventType::TokenCreated | EventType::TokenRevoked => {
      let changed = TokenChanged {
        version: 0,
        token_id: Uuid::nil(),
        name: text("name"),
        role: Role::ReadOnly,
        by: text("by"),
      };
      if kind == EventType::TokenCreated {
        Event::TokenCreated(changed)
      } else {
        Event::TokenRevoked(changed)
      }
    }
    EventType::SessionCreated => Event::SessionCreated(SessionCreated {
      version: 0,
      subject: text("subject"),
      name: text("name"),
      via: Via::Local,
      role: Role::ReadOnly,
      ip: Some(text("ip")),
    }),
    EventType::SessionRevoked => Event::SessionRevoked(SessionRevoked {
      version: 0,
      session_id: text("sessionId"),
      subject: text("subject"),
      by: Some(text("by")),
      reason: SessionRevokeReason::Logout,
    }),
    EventType::LoginFailed => Event::LoginFailed(LoginFailed {
      version: 0,
      provider: text("provider"),
      ip: text("ip"),
      reason: text("reason"),
    }),
    EventType::FileAnalysisComplete => Event::FileAnalysisComplete(FileAnalysisComplete {
      version: 0,
      report: Box::new(blank_analysis(d, now)),
    }),
    EventType::RelayAgentConnected => Event::RelayAgentConnected(RelayAgentConnected {
      version: 0,
      agent: text("agent"),
      ip_address: text("ipAddress"),
    }),
    EventType::RelayAgentDisconnected => Event::RelayAgentDisconnected(RelayAgentDisconnected {
      version: 0,
      agent: text("agent"),
      reason: text("reason"),
    }),
    EventType::RelayAgentSharesUpdated => Event::RelayAgentSharesUpdated(RelayAgentSharesUpdated {
      version: 0,
      agent: text("agent"),
      files: 0,
      directories: 0,
    }),
    EventType::ScriptRunCompleted => return None,
  })
}

/// A transfer with every field at its zero: the zero id, no state, no bytes, requested at the
/// start of year 1.
fn blank_transfer() -> Transfer {
  let year_one = NaiveDate::from_ymd_opt(1, 1, 1)
    .and_then(|day| day.and_hms_opt(0, 0, 0))
    .expect("the start of year 1 is a time")
    .and_utc();
  Transfer {
    id: Uuid::nil(),
    job_id: None,
    username: String::new(),
    direction: TransferDirection::Download,
    filename: String::new(),
    size: 0,
    state: TransferStates::empty(),
    requested_at: year_one,
    enqueued_at: None,
    started_at: None,
    ended_at: None,
    bytes_transferred: 0,
    average_speed: 0.0,
    place_in_queue: None,
    exception: None,
    attempts: 0,
    next_attempt_at: None,
    removed: false,
    bytes_remaining: 0,
    elapsed_time: None,
    percent_complete: 0.0,
    remaining_time: None,
    local_path: None,
    state_history: None,
  }
}

/// An analysis report made by the rule, without its per-window measures as the event carries
/// none.
fn blank_analysis(d: &str, now: DateTime<Utc>) -> AudioAnalysis {
  let text = |field: &str| format!("{d}{field}");
  AudioAnalysis {
    path: text("path"),
    root: FileRoot::Downloads,
    relative_path: text("relativePath"),
    transfer_id: Some(Uuid::nil()),
    size: 0,
    analyzed_at: now,
    duration_seconds: 0.0,
    claimed: ClaimedFormat {
      codec: text("codec"),
      sample_rate: 0,
      bit_depth: Some(0),
      channels: 0,
      bitrate_kbps: Some(0),
    },
    verdict: AnalysisVerdict::Consistent,
    confidence: 0.0,
    flags: Vec::new(),
    encoder_guess: Some(EncoderGuess {
      profile: text("profile"),
      confidence: 0.0,
    }),
    summary: text("summary"),
    measures: AnalysisMeasures {
      cutoff_hz_histogram: Vec::new(),
      cutoff_median_hz: Some(0),
      shelf_sharpness_db_p95: Some(0.0),
      sfb21_holes: Some(0.0),
      frame_artifact_score: Some(0.0),
      upsample_ratio: Some(0.0),
      bit_depth_effective: Some(0),
      noise_floor_dbfs: Some(0.0),
      stereo_correlation: Some(0.0),
      side_energy_db: Some(0.0),
      dc_offset: Some(0.0),
      clipped_samples_pct: Some(0.0),
      peak_dbfs: Some(0.0),
    },
    windows: None,
    spectrogram_url: text("spectrogramUrl"),
    spectrogram: Some(SpectrogramLayout {
      scale: SpectrogramScale::Log,
      max_hz: 0.0,
      duration_seconds: 0.0,
      sampled: false,
      guides: Vec::new(),
      bins: 0,
      columns_per_second: 0,
      peaks_per_second: 0,
      tile_seconds: 0,
      tiles: 0,
    }),
    judgment: Some(AnalysisOverride {
      verdict: Judgment::Legit,
      note: Some(text("note")),
      by: text("by"),
      at: now,
    }),
    limitations: Vec::new(),
    stale: false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_type_but_the_script_record_has_a_sample_that_reads_back() {
    let now: DateTime<Utc> = "2026-10-25T12:00:00Z".parse().unwrap();
    for &kind in EventType::ALL {
      let Some(event) = sample(kind, "x-", now) else {
        assert_eq!(kind, EventType::ScriptRunCompleted);
        continue;
      };
      assert_eq!(event.event_type(), kind);
      let json = serde_json::to_value(&event).unwrap();
      assert_eq!(serde_json::from_value::<Event>(json).unwrap(), event);
    }
  }
}
