//! Deciding a peer's request to queue an upload from us, once the network's edge passed it.
//! Requests are decided one at a time per user and [`DECIDING`] at a time over every user; a
//! request waits for its user's turn before it takes one of those, so one user cannot hold them
//! all. When the requests waiting would take more than [`BACKLOG_MILLIS`] at the pace of the
//! last ones accepted, a new request is dropped, since its peer would have given up by then.
//! A request for a file not shared, or shared and gone from disk, is refused, and so is one
//! past the limits of its user's group, unless the user is privileged.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use soulseek_client::{ClientError, LogLimiter};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, SemaphorePermit};
use tracing::{debug, info, trace, warn};

use super::{Uploads, limits};
use crate::shares::{LOCAL_HOST, Resolved};
use crate::sync::lock;
use crate::telemetry::millis;

/// What a peer is told when too many requests wait to be decided.
pub const OVERWHELMED: &str = "Overwhelmed with requests; try again later.";

/// What a peer is told for a file we do not share to them.
pub const NOT_SHARED: &str = "File not shared.";

/// The longest the requests waiting may take, at the pace of the last ones, in milliseconds.
const BACKLOG_MILLIS: f64 = 180_000.0;

/// How many requests are decided at once over every user.
const DECIDING: usize = 10;

/// Requests dropped for too many waiting, which come in bursts.
static DROPPED: LogLimiter = LogLimiter::new();
/// Requests refused, which a peer can repeat at any rate.
static REFUSED: LogLimiter = LogLimiter::new();
/// Shared files missing or changed on disk, which every request for them finds again.
static DRIFTED: LogLimiter = LogLimiter::new();

/// The turns requests take and how long the last ones took.
pub(super) struct Intake {
  /// one request per user at a time; a turn is made when a request first needs it
  turns: Mutex<HashMap<String, Arc<Semaphore>>>,
  /// the requests decided at once over every user
  deciding: Semaphore,
  /// requests waiting for either
  pub(super) waiting: AtomicUsize,
  /// the moving average of the time accepted requests took, in milliseconds, as `f64` bits
  pub(super) latency: AtomicU64,
}

impl Default for Intake {
  fn default() -> Self {
    Self {
      turns: Mutex::new(HashMap::new()),
      deciding: Semaphore::new(DECIDING),
      waiting: AtomicUsize::new(0),
      latency: AtomicU64::new(0f64.to_bits()),
    }
  }
}

/// A request's turn: its user's, then one of the deciding turns.
struct Turn<'a> {
  _user: OwnedSemaphorePermit,
  _deciding: SemaphorePermit<'a>,
}

/// Counts a request as waiting while it lives, so one given up still leaves the count.
struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
  fn drop(&mut self) {
    self.0.fetch_sub(1, Ordering::AcqRel);
  }
}

impl Intake {
  /// Waits for `username`'s turn, then for one of the deciding turns; returns the turn and how
  /// many requests still wait.
  async fn turn(&self, username: &str) -> (Turn<'_>, usize) {
    self.waiting.fetch_add(1, Ordering::AcqRel);
    let waiting = Waiting(&self.waiting);
    // taken under the lock, so the collection never drops a turn a request is about to wait on
    let user = lock(&self.turns)
      .entry(username.to_string())
      .or_insert_with(|| Arc::new(Semaphore::new(1)))
      .clone();
    let user = user
      .acquire_owned()
      .await
      .expect("a user's turn is never closed");
    let deciding = self
      .deciding
      .acquire()
      .await
      .expect("the deciding turns are never closed");
    drop(waiting);
    let left = self.waiting.load(Ordering::Acquire);
    (
      Turn {
        _user: user,
        _deciding: deciding,
      },
      left,
    )
  }

  /// How long, in milliseconds, the requests waiting would take at the pace of the last ones.
  fn backlog(&self) -> f64 {
    let latency = f64::from_bits(self.latency.load(Ordering::Acquire));
    self.waiting.load(Ordering::Acquire) as f64 * latency
  }

  /// Drops the turns of users nobody holds or waits for; skipped when a request is taking a
  /// turn right now.
  pub(super) fn collect_idle(&self) {
    let Ok(mut turns) = self.turns.try_lock() else {
      return debug!("user enqueue turns in use; collected at the next minute");
    };
    let before = turns.len();
    // a request holds a clone from before it waits until it is decided, so a turn only the
    // map refers to is neither held nor waited for
    turns.retain(|_, turn| Arc::strong_count(turn) > 1);
    let dropped = before - turns.len();
    if dropped > 0 {
      debug!(dropped, "idle user enqueue turns dropped");
    }
  }
}

/// Why a request is not queued.
pub(super) enum Refusal {
  /// the peer is told this
  Peer(&'static str),
  /// something here failed; the peer is told only that
  Failed(ClientError),
}

/// A shared file found on disk.
#[derive(Clone, Debug)]
pub(super) struct Located {
  /// where the share index has it
  pub resolved: Resolved,
  /// its size on disk now
  pub size: u64,
}

impl Uploads {
  /// Counts a request a peer sent to queue an upload.
  pub fn request_received(&self) {
    self.telemetry.metrics().enqueue.requests_received.inc();
  }

  /// Counts a request the network's edge refused.
  pub fn request_refused(&self) {
    self.telemetry.metrics().enqueue.requests_rejected.inc();
  }

  /// Whether the requests waiting would take more than `BACKLOG_MILLIS` at the pace of the
  /// last ones; a request that finds them so is counted as dropped and refused.
  pub fn overwhelmed(&self, username: &str) -> bool {
    let backlog = self.intake.backlog();
    if backlog <= BACKLOG_MILLIS {
      return false;
    }
    let enqueue = &self.telemetry.metrics().enqueue;
    enqueue.requests_dropped.inc();
    enqueue.requests_rejected.inc();
    if let Some(suppressed) = DROPPED.allow() {
      warn!(
        username,
        backlog_ms = backlog as u64,
        suppressed,
        "upload request dropped; too many wait to be decided"
      );
    }
    true
  }

  /// Decides the request of `username`, who sees the shares of the groups `sees`, for `filename`,
  /// which passed the network's edge, and queues the upload when it is accepted. A refusal is
  /// the reason the peer is told.
  pub async fn request(
    &self,
    username: &str,
    filename: &str,
    sees: &BTreeSet<String>,
  ) -> Result<(), ClientError> {
    let (_turn, waiting) = self.intake.turn(username).await;
    let enqueue = &self.telemetry.metrics().enqueue;
    enqueue.queue_depth.set(waiting as f64);
    let started = Instant::now();
    let decision = self.decide(username, filename, sees).await;
    let decided = millis(started);
    enqueue.decision_latency.observe(decided);
    enqueue.decision_latency_current.update(decided);
    let located = match decision {
      Ok(located) => located,
      Err(Refusal::Peer(reason)) => {
        enqueue.requests_rejected.inc();
        if let Some(suppressed) = REFUSED.allow() {
          debug!(
            username,
            filename, reason, suppressed, "upload request refused"
          );
        }
        return Err(ClientError::DownloadEnqueue(reason.to_string()));
      }
      Err(Refusal::Failed(error)) => return Err(error),
    };
    self.enqueue(username, filename, located).await?;
    let total = millis(started);
    enqueue.latency.observe(total);
    let average = enqueue.latency_current.update(total);
    self
      .intake
      .latency
      .store(average.to_bits(), Ordering::Release);
    enqueue.requests_accepted.inc();
    info!(
      username,
      filename,
      ms = total as u64,
      decision_ms = decided as u64,
      "upload request accepted"
    );
    Ok(())
  }

  /// Queues `files` to be offered to `username`, who did not ask for them, each decided as a
  /// request of theirs would be, with the shares they see as a buddy when they are one. These
  /// are our own sends, so the figures of the requests peers send are left alone.
  pub async fn send(
    &self,
    username: &str,
    files: Vec<String>,
  ) -> api_types::transfers::SendFilesResponse {
    let (sees, _) = self.users.search_sees(username, None).await;
    let mut sent = api_types::transfers::SendFilesResponse {
      queued: Vec::new(),
      failed: Vec::new(),
    };
    let (_turn, _) = self.intake.turn(username).await;
    for filename in files {
      let queued = match self.decide(username, &filename, &sees).await {
        Ok(located) => self.enqueue(username, &filename, located).await,
        Err(Refusal::Peer(reason)) => Err(ClientError::DownloadEnqueue(reason.to_string())),
        Err(Refusal::Failed(error)) => Err(error),
      };
      match queued {
        Ok(()) => {
          info!(username, filename, "file queued to be sent");
          sent.queued.push(filename);
        }
        Err(e) => {
          debug!(username, filename, error = %e, "file not sent");
          sent.failed.push(api_types::transfers::EnqueueFailure {
            filename,
            message: match e {
              ClientError::DownloadEnqueue(reason) => reason,
              other => other.to_string(),
            },
          });
        }
      }
    }
    sent
  }

  /// Whether `username` may have `filename`: shared with them, on disk, and within the limits
  /// of their group unless they are privileged.
  async fn decide(
    &self,
    username: &str,
    filename: &str,
    sees: &BTreeSet<String>,
  ) -> Result<Located, Refusal> {
    let located = self
      .locate(sees, filename)
      .await
      .map_err(Refusal::Failed)?
      .ok_or(Refusal::Peer(NOT_SHARED))?;
    let client = self.client().map_err(Refusal::Failed)?;
    let placement = self.users.fetch_placement(&client, username, false).await;
    if placement.privileged {
      debug!(username, "Limits bypassed; the user is privileged");
      return Ok(located);
    }
    let now = Utc::now();
    let name = username.to_string();
    let figures = self
      .db
      .read(move |conn| limits::statistics(conn, &name, now))
      .await
      .map_err(|e| {
        Refusal::Failed(ClientError::client(
          format!("the upload figures of {username} were not read: {e}"),
          None,
        ))
      })?;
    trace!(username, ?figures, "upload figures");
    let options = self.config.current();
    match limits::refusal(&options.values, &placement.limits, &figures, located.size) {
      Some(reason) => Err(Refusal::Peer(reason)),
      None => Ok(located),
    }
  }

  /// Where the shared file `filename` is, with its size on disk; `None` when it is not shared,
  /// is in a share the groups `sees` do not see, or is gone from disk. A local file gone, or whose size changed, asks the shares for a
  /// scan; one that changed is taken at its size on disk. A relay agent's file is taken at its
  /// indexed size when the options trust agents' listings, else the agent is asked.
  pub(super) async fn locate(
    &self,
    sees: &BTreeSet<String>,
    filename: &str,
  ) -> Result<Option<Located>, ClientError> {
    let view = self.shares.view(sees);
    let resolved = self
      .shares
      .resolve_file(filename, &view)
      .await
      .map_err(|e| ClientError::client(format!("the share index could not be read: {e}"), None))?;
    let Some(resolved) = resolved else {
      trace!(filename, "requested file not shared");
      return Ok(None);
    };
    if resolved.host != LOCAL_HOST {
      return Ok(self.locate_on_agent(filename, resolved).await);
    }
    let size = match tokio::fs::metadata(&resolved.path).await {
      Ok(metadata) if metadata.is_file() => metadata.len(),
      found => {
        if let Some(suppressed) = DRIFTED.allow() {
          let error = found.err().map(|e| e.to_string());
          warn!(
            path = %resolved.path.display(),
            error,
            suppressed,
            "The shared file could not be located on disk. A share scan should be performed"
          );
        }
        self.scan_needed().await;
        return Ok(None);
      }
    };
    if size != resolved.size {
      if let Some(suppressed) = DRIFTED.allow() {
        warn!(
          path = %resolved.path.display(),
          indexed = resolved.size,
          size,
          suppressed,
          "a shared file's size changed since the scan; a share scan should be performed"
        );
      }
      self.scan_needed().await;
    }
    Ok(Some(Located { resolved, size }))
  }

  /// The relay agent's file `filename`, `resolved` from its index; `None` when its agent is not
  /// ready, or says the file is gone, or does not answer in time.
  async fn locate_on_agent(&self, filename: &str, resolved: Resolved) -> Option<Located> {
    let relay = self
      .relay
      .as_ref()
      .expect("only a relay controller has agents' shares");
    let agent = resolved.host.as_str();
    if !relay.ready(agent) {
      debug!(agent, filename, "the agent holding the file is not ready");
      return None;
    }
    if self.trusts_agents {
      let size = resolved.size;
      return Some(Located { resolved, size });
    }
    let (exists, length) = relay.file_info(agent, filename).await?;
    if !exists || length == 0 {
      debug!(agent, filename, "The file could not be located on Agent");
      return None;
    }
    if length != resolved.size {
      warn!(
        agent,
        filename,
        indexed = resolved.size,
        length,
        "The file's size differs between the share and disk. A share scan should be performed"
      );
    }
    Some(Located {
      resolved,
      size: length,
    })
  }

  /// Asks the shares for a scan, after a shared file was found missing or changed.
  async fn scan_needed(&self) {
    if let Err(e) = self.shares.request_scan().await {
      warn!(error = %e, "share scan not requested");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn a_user_waits_for_their_own_turn_before_one_of_the_ten() {
    let intake = Arc::new(Intake::default());
    let (first, left) = intake.turn("alice").await;
    assert_eq!(left, 0);
    // alice's second request waits on alice, holding none of the ten
    let second = tokio::spawn({
      let intake = intake.clone();
      async move {
        let (_turn, _) = intake.turn("alice").await;
      }
    });
    tokio::task::yield_now().await;
    assert_eq!(intake.waiting.load(Ordering::Acquire), 1);
    assert_eq!(intake.deciding.available_permits(), DECIDING - 1);
    let (_bob, _) = intake.turn("bob").await;
    assert_eq!(intake.deciding.available_permits(), DECIDING - 2);
    drop(first);
    second.await.unwrap();
    assert_eq!(intake.waiting.load(Ordering::Acquire), 0);
  }

  #[tokio::test]
  async fn idle_turns_are_collected_and_held_ones_kept() {
    let intake = Intake::default();
    let (held, _) = intake.turn("alice").await;
    {
      let (_done, _) = intake.turn("bob").await;
    }
    intake.collect_idle();
    let turns: Vec<String> = intake.turns.lock().unwrap().keys().cloned().collect();
    assert_eq!(turns, ["alice"]);
    drop(held);
    intake.collect_idle();
    assert!(intake.turns.lock().unwrap().is_empty());
  }

  #[test]
  fn the_backlog_is_the_waiting_requests_at_the_pace_of_the_last_ones() {
    let intake = Intake::default();
    intake.waiting.store(4, Ordering::Release);
    assert_eq!(intake.backlog(), 0.0, "no pace before an accepted request");
    intake.latency.store(45_001f64.to_bits(), Ordering::Release);
    assert!(intake.backlog() > BACKLOG_MILLIS);
    intake.latency.store(45_000f64.to_bits(), Ordering::Release);
    assert!(intake.backlog() <= BACKLOG_MILLIS);
  }
}
