//! The application clock: jobs that run at one of a fixed set of intervals until shutdown.
//! Starting the clock runs every job registered by then once, as its first run, and then each on
//! its interval; ticks missed while the runtime was busy are delayed, not run in a burst.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info_span};

use crate::sync::lock;

/// How often a job runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Every {
  /// every 500 ms
  HalfSecond,
  /// every second
  Second,
  /// every 5 seconds
  FiveSeconds,
  /// every 15 seconds
  FifteenSeconds,
  /// every 30 seconds
  ThirtySeconds,
  /// every minute
  Minute,
  /// every 5 minutes
  FiveMinutes,
  /// every 30 minutes
  ThirtyMinutes,
  /// every hour
  Hour,
}

impl Every {
  /// Every interval, shortest first.
  pub const ALL: [Self; 9] = [
    Self::HalfSecond,
    Self::Second,
    Self::FiveSeconds,
    Self::FifteenSeconds,
    Self::ThirtySeconds,
    Self::Minute,
    Self::FiveMinutes,
    Self::ThirtyMinutes,
    Self::Hour,
  ];

  /// The time between two runs.
  pub fn period(self) -> Duration {
    let seconds = |s| Duration::from_secs(s);
    match self {
      Self::HalfSecond => Duration::from_millis(500),
      Self::Second => seconds(1),
      Self::FiveSeconds => seconds(5),
      Self::FifteenSeconds => seconds(15),
      Self::ThirtySeconds => seconds(30),
      Self::Minute => seconds(60),
      Self::FiveMinutes => seconds(300),
      Self::ThirtyMinutes => seconds(1800),
      Self::Hour => seconds(3600),
    }
  }
}

/// One run of a job, given whether it is the run at the clock's start.
type Job = Arc<dyn Fn(bool) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The clock. Cloning gives another handle to the same clock.
#[derive(Clone)]
pub struct Clock {
  inner: Arc<Inner>,
}

struct Inner {
  jobs: Mutex<Vec<(Every, &'static str, Job)>>,
  shutdown: CancellationToken,
}

impl Clock {
  /// A clock with no jobs that stops with `shutdown` once started.
  pub fn new(shutdown: CancellationToken) -> Self {
    Self {
      inner: Arc::new(Inner {
        jobs: Mutex::new(Vec::new()),
        shutdown,
      }),
    }
  }

  /// Runs `job`, named `name` in the logs, at `every`. `job` is told whether a run is the one
  /// at the clock's start, which a job registered after the start does not get.
  pub fn every<F, Fut>(&self, every: Every, name: &'static str, job: F)
  where
    F: Fn(bool) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
  {
    let job: Job = Arc::new(move |first_run| Box::pin(job(first_run)));
    lock(&self.inner.jobs).push((every, name, job));
  }

  /// Starts every interval: each job runs at once, then on its interval.
  pub fn start(&self) {
    for every in Every::ALL {
      let inner = self.inner.clone();
      tokio::spawn(
        async move {
          let mut ticks = tokio::time::interval(every.period());
          ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
          let mut first_run = true;
          loop {
            tokio::select! {
              () = inner.shutdown.cancelled() => return,
              _ = ticks.tick() => {}
            }
            let due: Vec<(&'static str, Job)> = lock(&inner.jobs)
              .iter()
              .filter(|(at, _, _)| *at == every)
              .map(|(_, name, job)| (*name, job.clone()))
              .collect();
            for (name, job) in due {
              tokio::spawn(job(first_run).instrument(info_span!("clock_job", job = name)));
            }
            first_run = false;
          }
        }
        .instrument(info_span!("clock", ?every)),
      );
    }
    debug!("clock started");
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicU32, Ordering};

  #[tokio::test(start_paused = true)]
  async fn a_job_runs_at_the_start_and_then_on_its_interval() {
    let shutdown = CancellationToken::new();
    let clock = Clock::new(shutdown.clone());
    let runs = Arc::new(Mutex::new(Vec::new()));
    let seen = runs.clone();
    clock.every(Every::FiveSeconds, "test", move |first_run| {
      let seen = seen.clone();
      async move { seen.lock().unwrap().push(first_run) }
    });
    let other = Arc::new(AtomicU32::new(0));
    let counted = other.clone();
    clock.every(Every::Hour, "hourly", move |_| {
      counted.fetch_add(1, Ordering::Relaxed);
      async {}
    });
    clock.start();
    tokio::time::sleep(Duration::from_millis(11_000)).await;
    assert_eq!(*runs.lock().unwrap(), [true, false, false]);
    assert_eq!(
      other.load(Ordering::Relaxed),
      1,
      "only the run at the start"
    );
    shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(runs.lock().unwrap().len(), 3, "nothing runs after shutdown");
  }
}
