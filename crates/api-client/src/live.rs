//! The event stream: `GET /live` read as [`LiveFrame`]s, opened again after a lost connection
//! with `Last-Event-ID`, so the daemon sends what was missed when it still has it. A refused
//! reopening is tried again, after `Retry-After` when the answer gives one, except a 401 or 403,
//! which trying again does not change.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use api_types::live::{Bye, ByeReason, LiveFrame};
use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};
use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde_json::{Value, json};
use tracing::debug;

use super::{Client, Error, Result, Route, refused};

/// How long to wait before opening the stream again, until the daemon says otherwise. The
/// daemon's `retry: 1000` comes in a block without data, which eventsource-stream 0.2 drops
/// with its `retry` (`EventStream::dispatch`), so this matches it.
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
    let events = self.connect().await.map_err(|refusal| refusal.error)?;
    let following = Following {
      live: self,
      events: Some(events),
      retry: RETRY,
      waiting: None,
      closed: false,
    };
    Ok(LiveStream(Box::pin(futures_util::stream::unfold(
      following, next,
    ))))
  }

  async fn connect(&self) -> std::result::Result<Events, Refusal> {
    let route = Route::new("live")
      .query(&[("topics", &self.topics)])
      .map_err(Refusal::from)?;
    let mut request = self.client.get(route).header("accept", "text/event-stream");
    if let Some(id) = self.last_event_id {
      request = request.header("last-event-id", id.to_string());
    }
    let response = request.send().await.map_err(Error::from)?;
    if response.status().is_success() {
      return Ok(Box::pin(response.bytes_stream().eventsource()));
    }
    // a `Retry-After` that is a date, or not a number of seconds, leaves the stream's own delay
    let retry_after = response
      .headers()
      .get(RETRY_AFTER)
      .and_then(|value| value.to_str().ok())
      .and_then(|value| value.trim().parse::<u64>().ok())
      .map(Duration::from_secs);
    Err(Refusal {
      error: refused(response).await,
      retry_after,
    })
  }
}

/// Why the stream did not open, and how long the daemon asked to wait before trying again.
struct Refusal {
  error: Error,
  /// the answer's `Retry-After` in seconds; a date or nothing leaves the stream's own delay
  retry_after: Option<Duration>,
}

impl<E: Into<Error>> From<E> for Refusal {
  fn from(error: E) -> Self {
    Self {
      error: error.into(),
      retry_after: None,
    }
  }
}

/// An open event stream: every frame in order, and an error each time opening it again fails.
/// It ends when the daemon says the credential stopped being valid, or refuses a reopening with
/// 401 or 403.
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
  /// how long the next attempt to open waits first, `None` to open at once
  waiting: Option<Duration>,
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
      if let Some(wait) = state.waiting.take() {
        tokio::time::sleep(wait).await;
      }
      match state.live.connect().await {
        Ok(events) => {
          state.events = Some(events);
          continue;
        }
        Err(Refusal { error, retry_after }) => {
          state.closed = matches!(
            error.status(),
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
          );
          state.waiting = Some(retry_after.unwrap_or(state.retry));
          return Some((Err(error), state));
        }
      }
    };
    match events.next().await {
      Some(Ok(event)) => {
        if let Some(retry) = event.retry {
          state.retry = retry;
        }
        // an id that is no sequence number cannot be resumed after, so a reopened stream
        // starts over with snapshots
        let id = event.id.parse::<u64>().ok();
        state.live.last_event_id = id;
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
        state.waiting = Some(state.retry);
      }
      None => {
        debug!("event stream ended; opening it again");
        state.events = None;
        state.waiting = Some(state.retry);
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

#[cfg(test)]
mod tests {
  use api_types::auth::{PrincipalSummary, Role};
  use api_types::live::Hello;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio::net::TcpListener;

  use super::*;

  fn hello() -> String {
    let frame = LiveFrame::Hello(Hello {
      server_time: "2026-10-25T12:00:00Z".parse().unwrap(),
      version: api_types::state::VersionInfo {
        full: "0.1.0 (0.1.0)".into(),
        current: "0.1.0".into(),
        latest: None,
        is_update_available: None,
        is_canary: false,
        is_development: true,
      },
      principal: PrincipalSummary {
        name: "admin".into(),
        role: Role::Administrator,
      },
      topics: vec!["application".into()],
      resumed: false,
    });
    serde_json::to_value(frame).unwrap()["data"].to_string()
  }

  /// A daemon that answers the `n`th connection with `answers[n]`, an empty one closing the
  /// connection unanswered, and sends back each request's `Last-Event-ID`.
  async fn daemon(answers: Vec<String>) -> (String, tokio::sync::mpsc::Receiver<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (seen, asked) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
      for answer in answers {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 4096];
        let read = socket.read(&mut request).await.unwrap();
        let text = String::from_utf8_lossy(&request[..read]).to_lowercase();
        let last = text
          .lines()
          .find_map(|line| line.strip_prefix("last-event-id: "))
          .map(|id| id.trim().to_string());
        seen.send(last).await.unwrap();
        socket.write_all(answer.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
      }
    });
    (base, asked)
  }

  fn events(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}")
  }

  fn refusal(status: &str, headers: &str) -> String {
    let body = r#"{"error":"busy","detail":null}"#;
    format!(
      "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{headers}content-length: {}\r\n\
       connection: close\r\n\r\n{body}",
      body.len()
    )
  }

  #[tokio::test]
  async fn a_busy_or_restarting_daemon_is_tried_again_and_a_refused_credential_ends_it() {
    let hello = hello();
    let (base, mut asked) = daemon(vec![
      events(&format!("id: 7\nevent: hello\ndata: {hello}\n\n")),
      String::new(),
      refusal("429 Too Many Requests", "retry-after: 0\r\n"),
      refusal("503 Service Unavailable", ""),
      refusal("502 Bad Gateway", ""),
      events(&format!(
        "id: 9\nevent: hello\ndata: {hello}\n\nid: 10\nevent: bye\ndata: {{\"reason\":\"unauthorized\"}}\n\n"
      )),
    ])
    .await;
    let client = Client::new(&base).unwrap();
    let mut stream = client.live(&["application"]).open().await.unwrap();
    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(first.id, Some(7));
    let dropped = stream.next().await.unwrap().unwrap_err();
    assert_eq!(dropped.status(), None, "a connection closed unanswered");
    for status in [429, 503, 502] {
      let error = stream.next().await.unwrap().unwrap_err();
      assert_eq!(error.status().map(|s| s.as_u16()), Some(status));
    }
    assert!(matches!(
      stream.next().await.unwrap().unwrap().frame,
      LiveFrame::Hello(_)
    ));
    let bye = stream.next().await.unwrap().unwrap();
    assert!(matches!(bye.frame, LiveFrame::Bye(_)));
    assert!(
      stream.next().await.is_none(),
      "a refused credential ends the stream"
    );
    assert_eq!(asked.recv().await.unwrap(), None);
    for _ in 0..5 {
      assert_eq!(
        asked.recv().await.unwrap().as_deref(),
        Some("7"),
        "resumed after what was seen"
      );
    }
  }

  #[tokio::test]
  async fn a_reopening_refused_with_401_ends_the_stream() {
    let hello = hello();
    let (base, _asked) = daemon(vec![
      events(&format!("id: 1\nevent: hello\ndata: {hello}\n\n")),
      refusal("401 Unauthorized", ""),
    ])
    .await;
    let client = Client::new(&base).unwrap();
    let mut stream = client.live(&["application"]).open().await.unwrap();
    stream.next().await.unwrap().unwrap();
    let error = stream.next().await.unwrap().unwrap_err();
    assert_eq!(error.status().map(|s| s.as_u16()), Some(401));
    assert!(stream.next().await.is_none());
  }
}
