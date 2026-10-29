//! The event stream's hub: every frame published gets the next sequence number, is written once
//! as server-sent-event text, kept a while for streams that reconnect, and queued for each
//! open stream that follows one of its topics.
//!
//! Frames are published with [`Live::publish`]. A stream registers with [`Live::open`], which
//! also decides whether a stream that reconnects can be given what it missed.

mod snapshots;
mod stream;
pub mod topics;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use api_types::live::LiveFrame;
use axum::body::Bytes;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, trace};

pub use snapshots::frames as snapshot;
use stream::Queue;
pub use stream::{Revalidate, body};
pub use topics::Topic;

use crate::sync::lock;

/// How many published frames are kept for streams that reconnect.
const RING_FRAMES: usize = 4096;

/// How long a published frame is kept for streams that reconnect.
const RING_AGE: Duration = Duration::from_secs(60);

/// The event stream's hub. Cloning gives another handle to the same hub.
#[derive(Clone)]
pub struct Live {
  hub: Arc<Hub>,
}

struct Hub {
  state: Mutex<HubState>,
  /// canceled when the daemon shuts down, which closes every stream
  cancel: CancellationToken,
}

struct HubState {
  /// the sequence number of the last frame published, the starting number before the first
  last: u64,
  /// the sequence number of the last frame dropped from the ring
  evicted_through: u64,
  ring: VecDeque<Arc<Published>>,
  streams: HashMap<u64, Arc<Queue>>,
  next_stream: u64,
  /// open streams by whom the cap counts them against
  per_principal: HashMap<String, u32>,
  /// the data of the last `metrics.update`, which is sent again only when it changed
  last_metrics: Option<String>,
}

/// The lowest and highest first sequence numbers: past any one an earlier run of the daemon
/// can have reached, and below 2^53 so a JavaScript client reads every id exactly.
const FIRST_SEQ: std::ops::Range<u64> = 1 << 32..1 << 48;

impl HubState {
  /// A hub with nothing published, whose sequence starts at a random number, so an id kept
  /// from an earlier run is not in its ring.
  fn new() -> Self {
    let start = rand::Rng::random_range(&mut rand::rng(), FIRST_SEQ);
    Self {
      last: start,
      evicted_through: start,
      ring: VecDeque::new(),
      streams: HashMap::new(),
      next_stream: 0,
      per_principal: HashMap::new(),
      last_metrics: None,
    }
  }
}

/// A frame as published: its number, its topics and its text, shared by every stream.
pub struct Published {
  seq: u64,
  at: Instant,
  topics: Vec<Topic>,
  /// an `application.state` frame, which a later one makes obsolete
  state: bool,
  block: Bytes,
}

/// A stream asking to be registered.
pub struct Opening {
  /// the topics it follows
  pub topics: Vec<Topic>,
  /// whom the stream cap counts it against: the principal's subject, with the client's address
  /// for a trusted network
  pub holder: String,
  /// how many streams one principal may hold
  pub limit: u32,
  /// the last sequence number the client saw, when it is reconnecting
  pub last_seen: Option<u64>,
}

/// A registered stream.
pub struct Registered {
  queue: Arc<Queue>,
  /// whether the frames since `last_seen` are queued, so snapshots are not needed
  pub resumed: bool,
  /// the sequence number the stream's first frames carry: `last_seen` when resumed, else the
  /// last frame published before the stream registered
  pub at: u64,
  /// the stream's number, for logs
  pub id: u64,
  hub: Arc<Hub>,
  holder: String,
}

impl Drop for Registered {
  fn drop(&mut self) {
    let mut hub = self.hub.lock();
    hub.streams.remove(&self.id);
    if let Some(count) = hub.per_principal.get_mut(&self.holder) {
      *count -= 1;
      if *count == 0 {
        hub.per_principal.remove(&self.holder);
      }
    }
  }
}

/// The principal already holds as many streams as it may.
#[derive(Debug)]
pub struct TooManyStreams;

impl Live {
  /// A hub whose streams say goodbye and close when `cancel` is canceled.
  pub fn new(cancel: CancellationToken) -> Self {
    Self {
      hub: Arc::new(Hub {
        state: Mutex::new(HubState::new()),
        cancel,
      }),
    }
  }

  /// Sends `frame` to every stream that follows one of its topics.
  pub fn publish(&self, frame: LiveFrame) {
    let topics = topics::routes(&frame);
    if topics.is_empty() {
      error!("a control frame was published; streams write those themselves");
      return;
    }
    let (kind, data) = match parts(&frame) {
      Ok(parts) => parts,
      Err(e) => {
        error!(error = %e, "a frame could not be written as JSON");
        return;
      }
    };
    let state = matches!(frame, LiveFrame::ApplicationState(_));
    let mut hub = self.hub.lock();
    if matches!(frame, LiveFrame::MetricsUpdate(_)) {
      if hub.last_metrics.as_deref() == Some(data.as_str()) {
        return;
      }
      hub.last_metrics = Some(data.clone());
    }
    hub.last += 1;
    let seq = hub.last;
    let published = Arc::new(Published {
      seq,
      at: Instant::now(),
      block: block(seq, &kind, &data),
      topics,
      state,
    });
    // a trace line per log record would itself be a log record, without end
    if !published.topics.contains(&Topic::Logs) {
      trace!(seq, kind, "frame published");
    }
    hub.ring.push_back(published.clone());
    hub.evict();
    for queue in hub.streams.values() {
      if queue.follows(&published.topics) {
        queue.push(published.clone());
      }
    }
  }

  /// Whether an open stream follows `topic`.
  pub fn followed(&self, topic: Topic) -> bool {
    let topic = [topic];
    self.hub.lock().streams.values().any(|q| q.follows(&topic))
  }

  /// Registers a stream. When it saw a frame that is still kept, everything for its topics
  /// since then is queued for it and it needs no snapshots.
  pub fn open(&self, opening: Opening) -> Result<Registered, TooManyStreams> {
    let mut guard = self.hub.lock();
    let hub = &mut *guard;
    let count = hub.per_principal.entry(opening.holder.clone()).or_default();
    if *count >= opening.limit {
      return Err(TooManyStreams);
    }
    *count += 1;
    hub.evict();
    let queue = Arc::new(Queue::new(opening.topics));
    let resumed = match opening.last_seen {
      Some(seen) if seen >= hub.evicted_through && seen <= hub.last => hub
        .ring
        .iter()
        .filter(|p| p.seq > seen && queue.follows(&p.topics))
        .all(|p| queue.push(p.clone())),
      _ => false,
    };
    if !resumed {
      queue.clear();
    }
    let at = match opening.last_seen {
      Some(seen) if resumed => seen,
      _ => hub.last,
    };
    hub.next_stream += 1;
    let id = hub.next_stream;
    hub.streams.insert(id, queue.clone());
    Ok(Registered {
      queue,
      resumed,
      at,
      id,
      hub: self.hub.clone(),
      holder: opening.holder,
    })
  }
}

impl Hub {
  fn lock(&self) -> MutexGuard<'_, HubState> {
    lock(&self.state)
  }
}

impl HubState {
  /// Drops the frames past the ring's size or age.
  fn evict(&mut self) {
    let now = Instant::now();
    while let Some(oldest) = self.ring.front() {
      if self.ring.len() <= RING_FRAMES && now.duration_since(oldest.at) <= RING_AGE {
        break;
      }
      self.evicted_through = oldest.seq;
      self.ring.pop_front();
    }
  }
}

/// A frame's `type` and its `data` as JSON text.
fn parts(frame: &LiveFrame) -> Result<(String, String), serde_json::Error> {
  let serde_json::Value::Object(mut object) = serde_json::to_value(frame)? else {
    unreachable!("a tagged enum is written as an object");
  };
  let kind = match object.remove("type") {
    Some(serde_json::Value::String(kind)) => kind,
    _ => unreachable!("a tagged enum has its tag"),
  };
  let data = object.remove("data").unwrap_or(serde_json::Value::Null);
  Ok((kind, serde_json::to_string(&data)?))
}

/// One server-sent event: its id, its type and its data.
fn block(seq: u64, kind: &str, data: &str) -> Bytes {
  Bytes::from(format!("id: {seq}\nevent: {kind}\ndata: {data}\n\n"))
}

/// `frame` as a server-sent event with the id `seq`, for the frames a stream writes itself.
pub fn render(seq: u64, frame: &LiveFrame) -> Result<Bytes, serde_json::Error> {
  let (kind, data) = parts(frame)?;
  Ok(block(seq, &kind, &data))
}
