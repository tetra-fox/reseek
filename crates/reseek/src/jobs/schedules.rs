//! The schedules: the recurring work the user sees on the schedules card, each run when its
//! option makes it due and on Run now, and the daemon's own upkeep that runs at fixed
//! intervals and is not shown. A scheduled run always updates its card; its job enters the
//! list only when it removed something or failed. Run now is started by the user, lists its
//! job and leaves the next run where it was.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use api_types::jobs::{JobEnding, JobSchedule, JobStarter};
use api_types::live::LiveFrame;
use chrono::{DateTime, Local, Utc};
use reseek_shares::ScanOrigin;
use reseek_shares::lock;
use reseek_shares::schedule::Schedule as Timing;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::MissedTickBehavior;
use tracing::{Instrument, debug, info_span, warn};

use super::RunOutcome;
use super::maintenance;
use super::store::LastRun;
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::config::Options;

/// How long the scheduler waits at most before it looks at the schedules again.
const LOOK_AGAIN: Duration = Duration::from_secs(60);

/// The recurring work the schedules card shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Schedule {
  /// rescans the shared folders
  ShareRescan,
  /// deletes old data as the retention options say
  Cleanup,
  /// vacuums every database
  Vacuum,
  /// checks for a new release
  ReleaseCheck,
  /// sends the next wishlist search
  Wishlist,
}

impl Schedule {
  /// The name the API, the jobs it starts and its kept last run know it by.
  pub fn name(self) -> &'static str {
    match self {
      Self::ShareRescan => "share-rescan",
      Self::Cleanup => "cleanup",
      Self::Vacuum => "vacuum",
      Self::ReleaseCheck => "release-check",
      Self::Wishlist => "wishlist",
    }
  }
}

/// A run of a schedule, as it ends.
type Work = Pin<Box<dyn Future<Output = RunOutcome> + Send>>;

/// A schedule as the card shows it and the scheduler runs it.
struct Entry {
  schedule: Schedule,
  title: &'static str,
  description: &'static str,
  /// the option that sets it, none when it is fixed
  option: Option<&'static str>,
  /// how often it runs now, and why it is off or fixed when it is
  timing: Timing,
  note: Option<String>,
  /// the run it counts from when it never ran: none to be due at once
  never_ran_from: Option<DateTime<Utc>>,
  /// whether the user may run it now
  runnable: bool,
}

/// Every schedule, as the options, the flags and the server set them now.
fn entries(app: &AppState, options: &Options, started: DateTime<Utc>) -> Vec<Entry> {
  let flags = &app.startup.values.flags;
  // the release check is turned off by the flag or by the build, which no setting changes
  let release_off = if flags.no_version_check {
    Some("Turned off by --no-version-check")
  } else if app.state.current().version.is_development {
    Some("Development builds are not checked")
  } else if app.state.current().version.is_canary {
    Some("Canary builds are not checked")
  } else {
    None
  };
  let wishlist = app.searches.wishlist_interval();
  vec![
    Entry {
      schedule: Schedule::ShareRescan,
      title: "Share rescan",
      description: "Finds changes in the shared folders",
      option: Some("shares.cache.retention"),
      timing: options.shares.cache.retention,
      note: None,
      never_ran_from: Some(started),
      runnable: true,
    },
    Entry {
      schedule: Schedule::Cleanup,
      title: "Cleanup",
      description: "Deletes old data, as set in Retention",
      option: Some("schedules.cleanup"),
      timing: options.schedules.cleanup,
      note: None,
      never_ran_from: None,
      runnable: true,
    },
    Entry {
      schedule: Schedule::Vacuum,
      title: "Database vacuum",
      description: "Frees unused space in the databases",
      option: Some("schedules.vacuum"),
      timing: options.schedules.vacuum,
      note: None,
      never_ran_from: Some(started),
      runnable: true,
    },
    Entry {
      schedule: Schedule::ReleaseCheck,
      title: "Release check",
      description: "Checks for a new version of reseek",
      option: release_off.is_none().then_some("schedules.release_check"),
      timing: if release_off.is_some() {
        Timing::Off
      } else {
        options.schedules.release_check
      },
      note: release_off.map(str::to_string),
      never_ran_from: None,
      runnable: release_off.is_none(),
    },
    Entry {
      schedule: Schedule::Wishlist,
      title: "Wishlist",
      description: "Searches for the next wishlist item",
      option: None,
      timing: wishlist.map_or(Timing::Off, Timing::every),
      note: Some(if wishlist.is_some() {
        "Set by the Soulseek server".to_string()
      } else {
        "Waiting for the Soulseek server".to_string()
      }),
      never_ran_from: Some(started),
      // the server sets how often a wishlist search may go out
      runnable: false,
    },
  ]
}

/// When `entry` is next due after its last run `last`, none when it is off.
async fn due(
  app: &AppState,
  entry: &Entry,
  last: Option<&LastRun>,
  now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
  let scheduled = last.and_then(|last| last.scheduled_at);
  let counted_from = match entry.schedule {
    // the last complete scan counts, whatever started it, and so does a scheduled one that
    // failed or was canceled, which would else be due again at once
    Schedule::ShareRescan => match app.shares.last_trusted_scan().await {
      Ok(trusted) => trusted.max(scheduled),
      Err(e) => {
        warn!(error = %e, "the last share scan not read; the rescan waits");
        return None;
      }
    },
    // what a release check found is in the state, which a restart empties, so a check made
    // before this start does not count
    Schedule::ReleaseCheck => scheduled.filter(|at| *at >= app.jobs.scheduler_started),
    Schedule::Cleanup | Schedule::Vacuum | Schedule::Wishlist => scheduled,
  };
  entry
    .timing
    .next(counted_from.or(entry.never_ran_from), now, &Local)
}

/// The schedules card.
pub async fn cards(app: &AppState) -> Result<Vec<JobSchedule>, ApiError> {
  let last_runs = app.jobs.last_runs().await.map_err(ApiError::internal)?;
  Ok(cards_with(app, &last_runs).await)
}

async fn cards_with(app: &AppState, last_runs: &HashMap<String, LastRun>) -> Vec<JobSchedule> {
  let options = app.config.current();
  let now = Utc::now();
  let mut cards = Vec::new();
  for entry in entries(app, &options.values, app.jobs.scheduler_started) {
    let last = last_runs.get(entry.schedule.name());
    let running = running(app, entry.schedule);
    let next_run_at = due(app, &entry, last, now).await;
    cards.push(JobSchedule {
      name: entry.schedule.name().to_string(),
      title: entry.title.to_string(),
      description: entry.description.to_string(),
      every: (entry.timing != Timing::Off).then(|| entry.timing.to_string()),
      option: entry.option.map(str::to_string),
      note: entry.note,
      runnable: entry.runnable,
      running,
      last_run_at: last.and_then(|l| l.run_at),
      last_ended_at: last.and_then(|l| l.ended_at),
      last_ending: last.and_then(|l| l.ending),
      last_summary: last.and_then(|l| l.summary.clone()),
      last_error: last.and_then(|l| l.error.clone()),
      last_job_id: last.and_then(|l| l.job_id),
      next_run_at,
    });
  }
  cards
}

/// Runs the schedules until the daemon stops: each when it is due, the card pushed whenever a
/// run starts or ends or an option changes.
pub async fn run(app: AppState) {
  let mut changes = app.config.changes();
  let mut pushed: Option<Vec<JobSchedule>> = None;
  loop {
    let last_runs = match app.jobs.last_runs().await {
      Ok(last_runs) => last_runs,
      Err(e) => {
        warn!(error = %e, "the schedules' last runs not read; trying again in a minute");
        tokio::select! {
          () = app.shutdown.cancelled() => return,
          () = tokio::time::sleep(Duration::from_secs(60)) => continue,
        }
      }
    };
    let options = app.config.current();
    let now = Utc::now();
    let mut next: Option<DateTime<Utc>> = None;
    for entry in entries(&app, &options.values, app.jobs.scheduler_started) {
      // while any scan runs the rescan waits, and the scan's end wakes the loop
      if running(&app, entry.schedule) {
        continue;
      }
      let last = last_runs.get(entry.schedule.name());
      let Some(due) = due(&app, &entry, last, now).await else {
        continue;
      };
      if due <= now {
        let starter = JobStarter::schedule(entry.schedule.name());
        start(&app, entry.schedule, starter).await;
      } else {
        next = Some(next.map_or(due, |next| next.min(due)));
      }
    }
    let cards = cards_with(&app, &last_runs).await;
    if pushed.as_ref() != Some(&cards) {
      app.live.publish(LiveFrame::JobsSchedules(cards.clone()));
      pushed = Some(cards);
    }
    // the server's wishlist interval comes with a login, which nothing else announces
    let wait = next
      .map_or(LOOK_AGAIN, |next| {
        (next - Utc::now()).to_std().unwrap_or(Duration::ZERO)
      })
      .min(LOOK_AGAIN);
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      () = tokio::time::sleep(wait) => {}
      () = app.jobs.schedules_changed.notified() => {}
      change = changes.recv() => match change {
        Ok(_) | Err(RecvError::Lagged(_)) => {}
        Err(RecvError::Closed) => return,
      },
    }
  }
}

/// Runs the schedule `name` now, started by the user; its next run stays where it was. 404 for
/// no such schedule, 409 while it runs.
pub async fn run_now(app: &AppState, name: &str) -> Result<(), ApiError> {
  let options = app.config.current();
  let entry = entries(app, &options.values, app.jobs.scheduler_started)
    .into_iter()
    .find(|entry| entry.schedule.name() == name)
    .ok_or_else(|| ApiError::not_found(format!("No schedule named {name}")))?;
  if !entry.runnable {
    return Err(ApiError::conflict(
      format!("{} cannot be run now", entry.title),
      entry.note,
    ));
  }
  if entry.schedule == Schedule::ShareRescan {
    return app
      .shares
      .scan(false, ScanOrigin::Asked)
      .map_err(|_| ApiError::conflict("A share scan is already in progress", None));
  }
  if running(app, entry.schedule) {
    return Err(ApiError::conflict(
      format!("{} is running", entry.title),
      None,
    ));
  }
  start(app, entry.schedule, JobStarter::you()).await;
  Ok(())
}

/// Whether a run of `schedule` is going on: for the rescan, any scan of the shares.
fn running(app: &AppState, schedule: Schedule) -> bool {
  match schedule {
    Schedule::ShareRescan => app.shares.scanning(),
    Schedule::Cleanup | Schedule::Vacuum | Schedule::ReleaseCheck | Schedule::Wishlist => {
      lock(&app.jobs.running_schedules).contains(&schedule)
    }
  }
}

/// Starts a run of `schedule` by `starter` in the background: a run the schedule started
/// counts as its last scheduled one from its start. The rescan is a share scan, which keeps
/// its own last run.
async fn start(app: &AppState, schedule: Schedule, starter: JobStarter) {
  let scheduled = starter.kind == api_types::jobs::JobStarterKind::Schedule;
  let listed = starter.kind == api_types::jobs::JobStarterKind::You;
  let run = app.clone();
  let work: Work = match schedule {
    Schedule::ShareRescan => {
      if let Err(e) = app.shares.scan(false, ScanOrigin::Scheduled) {
        debug!(?e, "scheduled share rescan left to the scan going on");
      }
      return;
    }
    Schedule::Cleanup => Box::pin(async move { maintenance::cleanup(&run, starter, listed).await }),
    Schedule::Vacuum => Box::pin(async move { maintenance::vacuum(&run, starter, listed).await }),
    Schedule::ReleaseCheck => Box::pin(async move { release_check(&run).await }),
    Schedule::Wishlist => Box::pin(async move { run.searches.wishlist_turn().await }),
  };
  if !lock(&app.jobs.running_schedules).insert(schedule) {
    return;
  }
  let started_at = Utc::now();
  if scheduled {
    app
      .jobs
      .update_last_run(schedule, move |last| last.scheduled_at = Some(started_at))
      .await;
  } else {
    app.jobs.schedules_changed.notify_one();
  }
  let app = app.clone();
  tokio::spawn(
    async move {
      let outcome = work.await;
      let ended_at = Utc::now();
      lock(&app.jobs.running_schedules).remove(&schedule);
      app
        .jobs
        .update_last_run(schedule, move |last| {
          last.run_at = Some(started_at);
          last.ended_at = Some(ended_at);
          last.ending = Some(outcome.ending);
          last.error = outcome.error;
          last.summary = outcome.summary;
          last.job_id = outcome.job_id;
        })
        .await;
    }
    .instrument(info_span!("schedule", schedule = schedule.name())),
  );
}

/// Reads the release feed and puts what it says into the state.
async fn release_check(app: &AppState) -> RunOutcome {
  match app.releases.update(&app.state).await {
    Ok(version) => RunOutcome {
      ending: JobEnding::Succeeded,
      error: None,
      summary: Some(match (version.is_update_available, &version.latest) {
        (Some(true), Some(latest)) => format!("Version {latest} is available"),
        (Some(false), _) => "This is the latest version".to_string(),
        // only a development or canary build is not checked, and its schedule is off
        _ => "This build is not checked".to_string(),
      }),
      job_id: None,
    },
    Err(e) => {
      warn!(error = e, "Failed to check version");
      RunOutcome {
        ending: JobEnding::Failed,
        error: Some(e),
        summary: None,
        job_id: None,
      }
    }
  }
}

/// Runs `work` every `period` until the daemon stops, the first time once `first` has passed; a
/// run missed while the runtime was busy is delayed, not run twice. This is the daemon's own
/// upkeep, which is not a job.
pub fn upkeep<F, Fut>(
  app: &AppState,
  first: Duration,
  period: Duration,
  name: &'static str,
  work: F,
) where
  F: Fn() -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let shutdown = app.shutdown.clone();
  tokio::spawn(
    async move {
      let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + first, period);
      ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
      loop {
        tokio::select! {
          () = shutdown.cancelled() => return,
          _ = ticks.tick() => {}
        }
        work().await;
      }
    }
    .instrument(info_span!("upkeep", work = name)),
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::testing::{TRUSTED, app};

  /// The entry `schedule` running every `every`, as if its option set that.
  fn entry(schedule: Schedule, every: Duration) -> Entry {
    Entry {
      schedule,
      title: schedule.name(),
      description: "",
      option: None,
      timing: Timing::every(every),
      note: None,
      never_ran_from: None,
      runnable: true,
    }
  }

  #[tokio::test]
  async fn a_scheduled_rescan_that_failed_waits_and_a_release_check_from_before_the_start_does_not()
  {
    let app = app(TRUSTED, &[]).await;
    let state = &app.state;
    let half_day = Duration::from_secs(12 * 3600);

    // a scheduled rescan failed now, after the startup scan that completed
    let rescan = entry(Schedule::ShareRescan, half_day);
    let trusted = state.shares.last_trusted_scan().await.unwrap();
    let now = Utc::now();
    let failed_at = now;
    assert!(trusted.is_none_or(|trusted| trusted < failed_at));
    let failed = LastRun {
      scheduled_at: Some(failed_at),
      ending: Some(JobEnding::Failed),
      ..LastRun::default()
    };
    assert_eq!(
      due(state, &rescan, Some(&failed), now).await,
      Some(failed_at + chrono::Duration::hours(12)),
      "it waits an interval from its own run"
    );

    // the release check's result lives in the state, so a run before this start does not count
    let release = entry(Schedule::ReleaseCheck, half_day);
    let started = state.jobs.scheduler_started;
    let before = LastRun {
      scheduled_at: Some(started - chrono::Duration::hours(2)),
      ..LastRun::default()
    };
    assert_eq!(due(state, &release, Some(&before), now).await, Some(now));
    let since = LastRun {
      scheduled_at: Some(started),
      ..LastRun::default()
    };
    assert_eq!(
      due(state, &release, Some(&since), now).await,
      Some(started + chrono::Duration::hours(12))
    );
  }
}
