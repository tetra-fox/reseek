//! The event stream: `GET /live` read as [`LiveFrame`]s, opened again after a lost connection
//! with `Last-Event-ID`, so the daemon sends what was missed when it still has it.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use api_types::live::{Bye, ByeReason, LiveFrame};
use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use tracing::debug;

use super::{Client, Error, Result, Route, send};

/// How long to wait before opening the stream again, until the daemon says otherwise.
const RETRY: Duration = Duration::from_secs(1);

/// One frame of the event stream with its sequence number.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveEvent {
  /// the frame's sequence number, which a reopened stream resumes after
  pub id: Option<u64>,
  /// the frame
  pub frame: LiveFrame,
}

/// An event stream about to be opened.
#[derive(Clone)]
pub struct Live {
  client: Client,
  topics: String,
  last_event_id: Option<u64>,
}

impl Client {
  /// The event stream of `topics`, such as `["application", "transfers"]`; open it with
  /// [`Live::open`].
  pub fn live(&self, topics: &[&str]) -> Live {
    Live {
      client: self.clone(),
      topics: topics.join(","),
      last_event_id: None,
    }
  }
}

type Events =
  Pin<Box<dyn Stream<Item = std::result::Result<Event, EventStreamError<reqwest::Error>>> + Send>>;

impl Live {
  /// The same stream resuming after the frame with sequence number `id`.
  pub fn resume_from(mut self, id: u64) -> Self {
    self.last_event_id = Some(id);
    self
  }

  /// Opens the stream. A refusal, such as a 401, is the error; once open, a lost connection is
  /// opened again on its own.
  pub async fn open(self) -> Result<LiveStream> {
    let events = self.connect().await?;
    let following = Following {
      live: self,
      events: Some(events),
      retry: RETRY,
      waiting: false,
      closed: false,
    };
    Ok(LiveStream(Box::pin(futures_util::stream::unfold(
      following, next,
    ))))
  }

  async fn connect(&self) -> Result<Events> {
    let route = Route::new("live").query(&[("topics", &self.topics)])?;
    let mut request = self.client.get(route).header("accept", "text/event-stream");
    if let Some(id) = self.last_event_id {
      request = request.header("last-event-id", id.to_string());
    }
    let response = send(request).await?;
    Ok(Box::pin(response.bytes_stream().eventsource()))
  }
}

/// An open event stream: every frame in order, and an error each time opening it again fails.
/// It ends when the daemon says the credential stopped being valid.
pub struct LiveStream(Pin<Box<dyn Stream<Item = Result<LiveEvent>> + Send>>);

impl Stream for LiveStream {
  type Item = Result<LiveEvent>;

  fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.0.as_mut().poll_next(context)
  }
}

/// Where a stream is between frames.
struct Following {
  live: Live,
  /// the open connection's events, `None` while it is to be opened again
  events: Option<Events>,
  /// how long to wait before opening again
  retry: Duration,
  /// the last attempt to open failed, so the next one waits first
  waiting: bool,
  /// the stream has ended
  closed: bool,
}

/// The next frame, opening the stream again as often as it takes.
async fn next(mut state: Following) -> Option<(Result<LiveEvent>, Following)> {
  loop {
    if state.closed {
      return None;
    }
    let Some(events) = state.events.as_mut() else {
      if state.waiting {
        tokio::time::sleep(state.retry).await;
      }
      match state.live.connect().await {
        Ok(events) => {
          state.events = Some(events);
          state.waiting = false;
          continue;
        }
        Err(e) => {
          // a refusal does not change by trying again; a connection that failed may
          state.closed = matches!(e, Error::Api { .. } | Error::Unexpected { .. });
          state.waiting = true;
          return Some((Err(e), state));
        }
      }
    };
    match events.next().await {
      Some(Ok(event)) => {
        if let Some(retry) = event.retry {
          state.retry = retry;
        }
        if event.event.is_empty() {
          continue;
        }
        let id = event.id.parse::<u64>().ok();
        if id.is_some() {
          state.live.last_event_id = id;
        }
        let frame = match frame(&event) {
          Ok(frame) => frame,
          Err(e) => return Some((Err(e), state)),
        };
        if matches!(
          frame,
          LiveFrame::Bye(Bye {
            reason: ByeReason::Unauthorized
          })
        ) {
          state.closed = true;
        }
        return Some((Ok(LiveEvent { id, frame }), state));
      }
      Some(Err(e)) => {
        debug!(error = %e, "event stream lost; opening it again");
        state.events = None;
        state.waiting = true;
      }
      None => {
        debug!("event stream ended; opening it again");
        state.events = None;
        state.waiting = true;
      }
    }
  }
}

/// The frame an event carries: its name is the frame's type, its data the frame's data.
fn frame(event: &Event) -> Result<LiveFrame> {
  let data: Value = serde_json::from_str(&event.data)?;
  Ok(serde_json::from_value(
    json!({ "type": event.event, "data": data }),
  )?)
}
