//! An agent's socket served without a network, on a clock the test moves: an agent whose
//! shares are being scanned is waited for past the two minutes, but not past its deferral.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use api_types::relay::{RELAY_PROTOCOL, RelayEnvelope, RelayMessage};
use axum::extract::ws::Message;
use futures_util::{Sink, Stream};
use tokio::sync::mpsc;

use crate::testing::app;
use reseek_relay_proto::credential::{Key, Purpose};
use reseek_relay_proto::frames;

const SECRET: &str = "nas-secret-0123456789";

/// A socket whose frames the test sends and reads.
struct Fake {
  frames: mpsc::Receiver<Result<Message, axum::Error>>,
  sent: mpsc::Sender<Message>,
}

impl Stream for Fake {
  type Item = Result<Message, axum::Error>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.frames.poll_recv(cx)
  }
}

impl Sink<Message> for Fake {
  type Error = axum::Error;

  fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), axum::Error>> {
    Poll::Ready(Ok(()))
  }

  fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), axum::Error> {
    self.sent.try_send(item).map_err(axum::Error::new)
  }

  fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), axum::Error>> {
    Poll::Ready(Ok(()))
  }

  fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), axum::Error>> {
    Poll::Ready(Ok(()))
  }
}

/// Lets the served socket handle what was sent to it.
async fn settle() {
  for _ in 0..50 {
    tokio::task::yield_now().await;
  }
}

/// The frames the socket sent since the last look, without its pings.
fn written(from: &mut mpsc::Receiver<Message>) -> Vec<Message> {
  std::iter::from_fn(|| from.try_recv().ok())
    .filter(|m| !matches!(m, Message::Ping(_)))
    .collect()
}

fn text(message: RelayMessage) -> Result<Message, axum::Error> {
  let envelope = RelayEnvelope {
    v: RELAY_PROTOCOL,
    id: None,
    message,
  };
  Ok(Message::Text(
    serde_json::to_string(&envelope).unwrap().into(),
  ))
}

fn read(message: &Message) -> RelayMessage {
  let Message::Text(text) = message else {
    panic!("not a text frame: {message:?}");
  };
  frames::decode(text.as_str()).unwrap().message
}

#[tokio::test]
async fn a_scanning_agent_is_waited_for_until_its_deferral_runs_out() {
  let config = format!(
    "[relay]\nenabled = true\nmode = \"controller\"\n\
     [relay.controller_timeouts]\nsync_deferral = \"3m\"\n\
     [relay.agents.nas]\ninstance_name = \"nas\"\nsecret = \"{SECRET}\"\n"
  );
  let app = app(&config, &[]).await;
  let controller = app.state.relay.controller.clone().unwrap();
  tokio::time::pause();
  let (to_socket, frames) = mpsc::channel(64);
  let (sent, mut from_socket) = mpsc::channel(1024);
  let served = tokio::spawn(
    controller
      .clone()
      .serve_socket(Fake { frames, sent }, [127, 0, 0, 1].into()),
  );
  settle().await;
  let RelayMessage::Challenge { token, .. } = read(&written(&mut from_socket)[0]) else {
    panic!("no challenge");
  };
  let credential = Key::derive(SECRET, "nas").credential(Purpose::Login, &token);
  let login = RelayMessage::Login {
    agent: "nas".into(),
    credential,
    version: crate::version::info().current,
    capabilities: Vec::new(),
  };
  to_socket.send(text(login)).await.unwrap();
  settle().await;
  assert!(matches!(
    read(&written(&mut from_socket)[0]),
    RelayMessage::LoginOk { .. }
  ));

  // a pong every 15 seconds keeps the socket from going silent, a deferral every minute
  let deferral = RelayMessage::SyncDeferred {
    reason: "scanning".into(),
  };
  for quarter in 1..=24u32 {
    tokio::time::advance(Duration::from_secs(15)).await;
    settle().await;
    let closed = written(&mut from_socket).into_iter().find_map(|m| match m {
      Message::Close(frame) => Some(frame.map(|f| f.code)),
      _ => None,
    });
    // the deferral ends 5 minutes after the login, which the timer may round up past
    let elapsed = quarter * 15;
    if elapsed < 300 {
      assert_eq!(closed, None, "open at {elapsed}s, past the two minutes");
    } else if closed.is_some() || elapsed > 300 {
      assert_eq!(
        closed,
        Some(Some(frames::close::TIMEOUT)),
        "closed at {elapsed}s"
      );
      break;
    }
    to_socket
      .send(Ok(Message::Pong(Default::default())))
      .await
      .unwrap();
    if quarter % 4 == 0 {
      to_socket.send(text(deferral.clone())).await.unwrap();
    }
    settle().await;
  }
  served.await.unwrap();
  assert!(controller.agents().is_empty());
}
