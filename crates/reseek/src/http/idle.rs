//! A request body that ends with an error once its client sends nothing for [`BODY_IDLE`] while
//! the handler waits for more. hyper bounds the wait for a request's headers but not for its
//! body, so without this a client that stops partway through a body keeps its request open, and
//! what was read of it, for as long as it likes. Time the handler spends not reading does not
//! count.

use std::error::Error;
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes, HttpBody};
use hyper::body::{Frame, SizeHint};
use tokio::time::Sleep;

/// How long a request's body may go without bytes while it is read.
const BODY_IDLE: Duration = Duration::from_secs(60);

/// The error a body ends with when its client stopped sending.
#[derive(Debug)]
struct Stalled;

impl fmt::Display for Stalled {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "the client sent nothing for {} seconds",
      BODY_IDLE.as_secs()
    )
  }
}

impl Error for Stalled {}

/// `body`, ended with an error when its client sends nothing for `BODY_IDLE`.
pub fn bounded(body: Body) -> Body {
  Body::new(Idle { body, timer: None })
}

/// Whether `error`, from reading a request's body, is that its client stopped sending.
pub fn stalled(error: &(dyn Error + 'static)) -> bool {
  std::iter::successors(Some(error), |&e| e.source()).any(|e| e.is::<Stalled>())
}

struct Idle {
  body: Body,
  /// started when a read finds nothing, cleared when a frame comes
  timer: Option<Pin<Box<Sleep>>>,
}

impl HttpBody for Idle {
  type Data = Bytes;
  type Error = axum::Error;

  fn poll_frame(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
    let this = &mut *self;
    if let Poll::Ready(frame) = Pin::new(&mut this.body).poll_frame(cx) {
      this.timer = None;
      return Poll::Ready(frame);
    }
    let timer = this
      .timer
      .get_or_insert_with(|| Box::pin(tokio::time::sleep(BODY_IDLE)));
    match timer.as_mut().poll(cx) {
      Poll::Ready(()) => Poll::Ready(Some(Err(axum::Error::new(Stalled)))),
      Poll::Pending => Poll::Pending,
    }
  }

  fn is_end_stream(&self) -> bool {
    self.body.is_end_stream()
  }

  fn size_hint(&self) -> SizeHint {
    self.body.size_hint()
  }
}
