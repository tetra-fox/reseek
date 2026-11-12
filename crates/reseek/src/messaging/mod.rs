//! Private conversations and the rooms we are in. Every private message and every room message
//! is stored as it arrives, before anyone is told of it, and kept until the chat retention
//! deletes it. A private message stays unacknowledged until it is read, so the server delivers
//! it again at the next login until then. Rooms are joined at login, those added to the
//! configuration at once, and rejoined after a reconnect; each login is logged so room history
//! can say where it has holes.
//!
//! [`Messaging`] is the engine. The client's events about messages and rooms reach it through
//! [`Messaging::take`], in the order the client raised them, and one task handles them. Events
//! lost on the way are marked in that order by [`Messaging::missed`].

mod away;
mod conversations;
mod export;
mod feed;
mod mentions;
mod pins;
mod retention;
mod rooms;
mod search;
mod service;
mod sessions;
mod unread;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use api_types::messaging::NotifyLevel;
use cascade::Watcher;
use chrono::{DateTime, Utc};
use reseek_shares::lock;
use soulseek_client::{Client, Event};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{Instrument, debug, info_span, warn};

pub use conversations::store::{figures as message_figures, summary as conversation_summary};

use crate::config::Options;
use crate::db::{self, Database, DbError, Location, Schema};
use crate::events::EventBus;
use crate::live::Live;
use crate::network::Network;
use crate::state::StateOwner;
use crate::users::Directory;
use rooms::Rooms;

/// `messaging.db`: conversations, private and room messages, room settings, the pinned chats and
/// the sessions.
pub static SCHEMA: Schema = db::schema!(
  "messaging",
  [
    "0001_initial",
    "0002_sessions_after_loss",
    "0003_conversation_times"
  ]
);

/// How many client events may wait for the messaging task.
const INTAKE: usize = 256;

/// How long the stop waits for the work started on the way, such as joining rooms.
const STOPPING: Duration = Duration::from_secs(5);

/// Opens `messaging.db` in `location`.
pub async fn open(location: &Location) -> Result<Database, DbError> {
  Database::open(&SCHEMA, location).await
}

/// A notification level as stored.
fn notify_text(level: NotifyLevel) -> &'static str {
  match level {
    NotifyLevel::All => "all",
    NotifyLevel::Mentions => "mentions",
    NotifyLevel::None => "none",
  }
}

/// A notification level [`notify_text`] stored.
fn notify_level(text: &str) -> rusqlite::Result<NotifyLevel> {
  match text {
    "all" => Ok(NotifyLevel::All),
    "mentions" => Ok(NotifyLevel::Mentions),
    "none" => Ok(NotifyLevel::None),
    other => Err(rusqlite::Error::FromSqlConversionFailure(
      0,
      rusqlite::types::Type::Text,
      format!("not a notification level: {other}").into(),
    )),
  }
}

/// What the messaging task takes, in the order it came.
enum Taken {
  /// a client event, boxed as most are far larger than the marker
  Event(Box<Event>),
  /// client events were lost before they reached the engine
  Missed,
}

/// The messaging engine.
pub struct Messaging {
  this: Weak<Messaging>,
  db: Database,
  network: Arc<Network>,
  users: Arc<Directory>,
  live: Live,
  events: EventBus,
  config: Watcher<Options>,
  state: StateOwner,
  /// the client events waiting for the task that handles them
  intake: mpsc::Sender<Taken>,
  /// that task
  worker: Mutex<Option<JoinHandle<()>>>,
  /// tells that task to end once it handled every event taken so far
  drained: CancellationToken,
  /// the peers of the open conversations, changed only in the database write that opens or
  /// closes one, so the two never disagree
  active: Arc<Mutex<HashSet<String>>>,
  /// the rooms we are in and the private rooms we belong to
  rooms: Rooms,
  /// held from reading a conversation to pushing it, so the last push has the newest
  conversations_pushing: tokio::sync::Mutex<()>,
  /// asks for the unread counts to be pushed
  unread: unread::Unread,
  /// the running session's row in the session log
  session: Mutex<Option<i64>>,
  /// the username the last login was made with, noted as the login is handled
  session_user: Mutex<Option<String>>,
  /// the work started on the way, so the stop waits for it
  tasks: TaskTracker,
  /// what a message search is tried against before it runs
  probe: search::Probe,
  /// whether we are away, and who was answered while we were
  status: away::Status,
  /// the public room feed, held while the server is asked for it or told to stop
  feed: tokio::sync::Mutex<feed::Feed>,
  /// canceled when the daemon shuts down
  shutdown: CancellationToken,
}

impl Messaging {
  /// The engine over `db`, reaching the server through `network`, asking `users` who is
  /// blocked and touching them, raising its events on `events`, pushing to `live` and keeping
  /// `State.rooms` in `state`. A session the last run left open is closed when it was last seen.
  #[allow(
    clippy::too_many_arguments,
    reason = "the engine takes each part of the daemon it uses"
  )]
  pub async fn open(
    db: Database,
    network: Arc<Network>,
    users: Arc<Directory>,
    live: Live,
    events: EventBus,
    config: Watcher<Options>,
    state: StateOwner,
    shutdown: CancellationToken,
  ) -> Result<Arc<Self>, DbError> {
    let active = db.read(conversations::store::active).await?;
    let probe = db.read(search::Probe::of).await?;
    db.write(sessions::close_crashed).await?;
    let (intake, receiver) = mpsc::channel(INTAKE);
    let messaging = Arc::new_cyclic(|this| Self {
      this: this.clone(),
      db,
      network,
      users,
      live,
      events,
      config,
      state,
      intake,
      worker: Mutex::new(None),
      drained: CancellationToken::new(),
      active: Arc::new(Mutex::new(active.into_iter().collect())),
      rooms: Rooms::default(),
      conversations_pushing: tokio::sync::Mutex::new(()),
      unread: unread::Unread::default(),
      session: Mutex::new(None),
      session_user: Mutex::new(None),
      tasks: TaskTracker::new(),
      probe,
      status: away::Status::default(),
      feed: tokio::sync::Mutex::default(),
      shutdown,
    });
    messaging.restore_status().await;
    let worker = tokio::spawn(
      messaging
        .clone()
        .work(receiver)
        .instrument(info_span!("messaging")),
    );
    *lock(&messaging.worker) = Some(worker);
    messaging.follow_blocks();
    messaging.tasks.spawn(
      messaging
        .clone()
        .push_unread()
        .instrument(info_span!("chat_unread")),
    );
    messaging.tasks.spawn(
      messaging
        .clone()
        .idle_away()
        .instrument(info_span!("auto_away")),
    );
    messaging.tasks.spawn(
      messaging
        .clone()
        .follow_feed()
        .instrument(info_span!("public_feed")),
    );
    Ok(messaging)
  }

  /// `messaging.db`.
  pub fn database(&self) -> &Database {
    &self.db
  }

  fn this(&self) -> Arc<Self> {
    self
      .this
      .upgrade()
      .expect("the engine is alive while a method runs on it")
  }

  fn client(&self) -> &Client {
    self.network.client()
  }

  /// Our username: the one logged in with, else the configured one.
  fn us(&self) -> Option<String> {
    self
      .client()
      .username()
      .or_else(|| self.config.current().values.soulseek.username.clone())
  }

  /// The username of the last login, which every room event of a session follows; `None`
  /// before the first.
  fn logged_in_as(&self) -> Option<String> {
    lock(&self.session_user).clone()
  }

  fn logged_in_now(&self) -> bool {
    self.client().state().is_logged_in()
  }

  /// Hands a client event about a message, a room, a login or a disconnect to the messaging
  /// task, waiting while it is behind.
  pub async fn take(&self, event: Event) {
    if self
      .intake
      .send(Taken::Event(Box::new(event)))
      .await
      .is_err()
    {
      debug!("messaging stopped; client event not handled");
    }
  }

  /// Tells the messaging task that client events were lost here, after those taken so far and
  /// before those taken next, waiting while it is behind.
  pub async fn missed(&self) {
    if self.intake.send(Taken::Missed).await.is_err() {
      debug!("messaging stopped; lost client events not marked");
    }
  }

  /// Handles the events taken, in order, until the stop and every event taken before it.
  async fn work(self: Arc<Self>, mut receiver: mpsc::Receiver<Taken>) {
    // when the last event was handled, which is where events lost after it go missing
    let mut handled_at = Utc::now();
    loop {
      let taken = tokio::select! {
        biased;
        taken = receiver.recv() => taken,
        () = self.drained.cancelled() => {
          while let Ok(taken) = receiver.try_recv() {
            handled_at = self.take_in(taken, handled_at).await;
          }
          return;
        }
      };
      match taken {
        Some(taken) => handled_at = self.take_in(taken, handled_at).await,
        None => return,
      }
    }
  }

  /// Handles `taken`, the last event before it handled at `handled_at`; returns when it was
  /// handled.
  async fn take_in(&self, taken: Taken, handled_at: DateTime<Utc>) -> DateTime<Utc> {
    match taken {
      Taken::Event(event) => self.handle(*event).await,
      Taken::Missed => self.events_missed(handled_at).await,
    }
    Utc::now()
  }

  async fn handle(&self, event: Event) {
    match event {
      Event::PrivateMessageReceived(message) => self.received(message).await,
      Event::LoggedIn { username, .. } => self.session_started(username).await,
      Event::Disconnected { at, .. } => {
        self.session_ended(at.into()).await;
        self.rooms_disconnected();
        self.status_disconnected();
        self.feed_disconnected().await;
      }
      other => {
        let span = info_span!("room", room = rooms::room_of(&other));
        self.room_event(other).instrument(span).await;
      }
    }
  }

  /// Stops at shutdown, once the client is stopped: the events taken are handled, the session
  /// is closed, and the work on the way gets a moment to end.
  pub async fn stop(&self) {
    self.drained.cancel();
    let worker = lock(&self.worker).take();
    if let Some(worker) = worker
      && let Err(e) = worker.await
    {
      warn!(error = %e, "the messaging task ended badly");
    }
    self.session_ended(Utc::now()).await;
    self.tasks.close();
    if tokio::time::timeout(STOPPING, self.tasks.wait())
      .await
      .is_err()
    {
      debug!(
        running = self.tasks.len(),
        "messaging work left at shutdown"
      );
    }
  }
}
