//! The upload speed limits of the groups: one bucket per group, filled every 100 ms with its
//! group's `speed_limit`, `privileged` with the global upload limit in force, the alternative
//! one while the alternative limits are switched on. Every chunk of an
//! upload asks the bucket of the group its user is in at that moment, then the client's own
//! bucket for every upload, so the slower limit governs. What the group's bucket granted and
//! the chunk did not move, because the client's limit took less or the socket took less, goes
//! back to the bucket it came from.
//!
//! A change of a group's speed or of the global speed gives every group a new full bucket at
//! once; a chunk already waiting on an old bucket is served by it within 100 ms.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use soulseek_client::{Governor as ChunkGovernor, Reporter, TokenBucket};
use tracing::debug;

use crate::config::{Groups, Options};
use crate::sync::lock;
use crate::users::{DEFAULT_GROUP, LEECHERS, PRIVILEGED};

/// Looks up the group an upload's user is in now.
pub type CurrentGroup = Arc<dyn Fn() -> String + Send + Sync>;

/// The bucket an upload's last chunk was granted bytes by, with how many.
type LastGrant = Arc<Mutex<Option<(Arc<TokenBucket>, u32)>>>;

/// The groups' buckets.
pub struct Governor {
  buckets: RwLock<Arc<HashMap<String, Arc<TokenBucket>>>>,
  /// what the buckets were made from
  built_from: Mutex<(Groups, Option<u32>)>,
  /// the alternative speed limits are in force
  alternative: AtomicBool,
}

/// One bucket per group for `options`, each full, with the alternative global limit when
/// `alternative`.
fn buckets(options: &Options, alternative: bool) -> HashMap<String, Arc<TokenBucket>> {
  let groups = &options.transfers.groups;
  // an unset limit is the client's no limit
  let bucket = |limit: Option<u32>| Arc::new(TokenBucket::for_speed(limit.unwrap_or(0)));
  let mut buckets = HashMap::from([
    (
      PRIVILEGED.to_string(),
      bucket(options.transfers.upload_speed_limit(alternative)),
    ),
    (
      DEFAULT_GROUP.to_string(),
      bucket(groups.default.upload.speed_limit),
    ),
    (
      LEECHERS.to_string(),
      bucket(groups.leechers.upload.speed_limit),
    ),
  ]);
  for (name, group) in &groups.user_defined {
    buckets.insert(name.clone(), bucket(group.upload.speed_limit));
  }
  buckets
}

/// What made the buckets: the groups and the global speed in force.
fn made_from(options: &Options, alternative: bool) -> (Groups, Option<u32>) {
  (
    options.transfers.groups.clone(),
    options.transfers.upload_speed_limit(alternative),
  )
}

impl Governor {
  /// The buckets for `options`, with the normal speed limits in force.
  pub fn new(options: &Options) -> Self {
    Self {
      buckets: RwLock::new(Arc::new(buckets(options, false))),
      built_from: Mutex::new(made_from(options, false)),
      alternative: AtomicBool::new(false),
    }
  }

  /// Makes every bucket again, full, when a group or the global speed changed.
  pub fn configure(&self, options: &Options) {
    let mut built_from = lock(&self.built_from);
    let alternative = self.alternative.load(Ordering::Acquire);
    let wanted = made_from(options, alternative);
    if *built_from == wanted {
      return;
    }
    *self.buckets.write().unwrap_or_else(PoisonError::into_inner) =
      Arc::new(buckets(options, alternative));
    *built_from = wanted;
    debug!("upload speed limits of the groups made again");
  }

  /// Puts the alternative speed limits in force, or the normal ones, for `options`.
  pub fn switch(&self, alternative: bool, options: &Options) {
    self.alternative.store(alternative, Ordering::Release);
    self.configure(options);
  }

  /// The bucket of `group`, or the default group's for a group without one.
  pub fn bucket(&self, group: &str) -> Arc<TokenBucket> {
    let buckets = self
      .buckets
      .read()
      .unwrap_or_else(PoisonError::into_inner)
      .clone();
    buckets
      .get(group)
      .or_else(|| buckets.get(DEFAULT_GROUP))
      .expect("the default group always has a bucket")
      .clone()
  }

  /// The governor and the reporter of one upload whose user is in the group `group` gives at
  /// each chunk.
  pub fn pace(self: &Arc<Self>, group: CurrentGroup) -> (ChunkGovernor, Reporter) {
    let last: LastGrant = Arc::default();
    let granting = last.clone();
    let governor = self.clone();
    let chunk: ChunkGovernor = Arc::new(move |_, want, cancel| {
      let bucket = governor.bucket(&group());
      let last = granting.clone();
      Box::pin(async move {
        match bucket.get(want, &cancel).await {
          Ok(granted) => {
            *lock(&last) = Some((bucket, granted));
            granted
          }
          // the client sees the cancel and ends the upload
          Err(_) => 0,
        }
      })
    });
    let reporter: Reporter = Arc::new(move |_, _, _, moved| {
      let taken = lock(&last).take();
      if let Some((bucket, granted)) = taken {
        bucket.give_back(granted.saturating_sub(moved));
      }
    });
    (chunk, reporter)
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use tokio_util::sync::CancellationToken;

  use super::*;

  fn options(toml: &str) -> Options {
    toml::from_str(toml).unwrap()
  }

  /// A snapshot no governor here looks at.
  fn transfer() -> soulseek_client::Transfer {
    soulseek_client::Transfer {
      direction: soulseek_proto::TransferDirection::Upload,
      username: "alice".into(),
      filename: "a.flac".into(),
      token: 1,
      remote_token: None,
      state: soulseek_client::TransferState::IN_PROGRESS,
      size: 1_000_000,
      start_offset: 0,
      bytes_transferred: 0,
      bytes_remaining: 1_000_000,
      percent_complete: 0.0,
      average_speed: 0.0,
      start_time: None,
      end_time: None,
      elapsed_time: None,
      remaining_time: None,
      ip_end_point: None,
      error: None,
    }
  }

  #[tokio::test(start_paused = true)]
  async fn a_speed_change_rebuckets_a_running_upload_at_its_next_chunk() {
    let governor = Arc::new(Governor::new(&options(
      "[transfers.groups.default.upload]\nspeed_limit = 1\n",
    )));
    let (chunk, report) = governor.pace(Arc::new(|| DEFAULT_GROUP.to_string()));
    let never = CancellationToken::new();
    assert_eq!(chunk(&transfer(), 50_000, never.clone()).await, 102);
    report(&transfer(), 50_000, 102, 102);
    governor.configure(&options(
      "[transfers.groups.default.upload]\nspeed_limit = 100\n",
    ));
    let before = tokio::time::Instant::now();
    assert_eq!(
      chunk(&transfer(), 50_000, never.clone()).await,
      10_240,
      "the new bucket, full"
    );
    assert_eq!(tokio::time::Instant::now(), before, "without a wait");
    tokio::time::advance(Duration::from_millis(100)).await;
    assert_eq!(chunk(&transfer(), 50_000, never).await, 10_240);
  }

  #[tokio::test(start_paused = true)]
  async fn what_a_chunk_did_not_move_goes_back_to_its_group() {
    let governor = Arc::new(Governor::new(&options(
      "[transfers.groups.leechers.upload]\nspeed_limit = 1\n",
    )));
    let (chunk, report) = governor.pace(Arc::new(|| LEECHERS.to_string()));
    let never = CancellationToken::new();
    assert_eq!(chunk(&transfer(), 1000, never.clone()).await, 102);
    // the client's limit let 40 through and 30 were written
    report(&transfer(), 1000, 40, 30);
    let before = tokio::time::Instant::now();
    assert_eq!(
      chunk(&transfer(), 1000, never).await,
      72,
      "102 - 30 came back"
    );
    assert_eq!(tokio::time::Instant::now(), before);
  }

  #[test]
  fn a_group_without_a_bucket_takes_the_default_ones_and_privileged_the_global_speed() {
    let governor = Governor::new(&options(
      "[transfers.upload]\nspeed_limit = 7\n[transfers.groups.default.upload]\nspeed_limit = 3\n",
    ));
    assert!(Arc::ptr_eq(
      &governor.bucket("blacklisted"),
      &governor.bucket(DEFAULT_GROUP)
    ));
    assert!(!Arc::ptr_eq(
      &governor.bucket(PRIVILEGED),
      &governor.bucket(DEFAULT_GROUP)
    ));
  }
}
