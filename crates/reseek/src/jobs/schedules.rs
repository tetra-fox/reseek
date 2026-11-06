//! The schedules: the recurring work the user sees on the schedules card, each run when its
//! option makes it due and on Run now, and the daemon's own upkeep that runs at fixed
//! intervals and is not shown. A scheduled run always updates its card; its job enters the
//! list only when it removed something or failed. Run now is started by the user, lists its
//! job and leaves the next run where it was.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use api_types::jobs::{JobEnding, JobSchedule, JobStarter};
use api_types::live::LiveFrame;
use chrono::{DateTime, Local, Utc};
use reseek_shares::ScanOrigin;
use reseek_shares::schedule::Schedule;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::MissedTickBehavior;
use tracing::{Instrument, debug, info_span, warn};

use super::RunOutcome;
use super::maintenance;
use super::scans::SHARE_RESCAN;
use super::store::LastRun;
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::config::Options;
use crate::sync::lock;

/// How long the scheduler waits at most before it looks at the schedules again.
const LOOK_AGAIN: Duration = Duration::from_secs(60);

/// The cleanup schedule.
pub const CLEANUP: &str = "cleanup";
/// The database vacuum schedule.
pub const VACUUM: &str = "vacuum";
/// The release check schedule.
pub const RELEASE_CHECK: &str = "release-check";
/// The wishlist schedule.
pub const WISHLIST: &str = "wishlist";

/// A schedule as the card shows it and the scheduler runs it.
struct Entry {
  name: &'static str,
  title: &'static str,
  description: &'static str,
  /// the option that sets it, none when it is fixed
  option: Option<&'static str>,
  /// how often it runs now, and why it is off or fixed when it is
  schedule: Schedule,
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
    Some("Turned off by the no-version-check flag")
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
      name: SHARE_RESCAN,
      title: "Share rescan",
      description: "Scans the shared folders again for what changed.",
      option: Some("shares.cache.retention"),
      schedule: options.shares.cache.retention,
      note: None,
      never_ran_from: Some(started),
      runnable: true,
    },
    Entry {
      name: CLEANUP,
      title: "Cleanup",
      description: "Deletes what the retention settings let go: finished transfers, searches, \
                    chat history, events, users, downloaded files, sessions and ended jobs.",
      option: Some("schedules.cleanup"),
      schedule: options.schedules.cleanup,
      note: None,
      never_ran_from: None,
      runnable: true,
    },
    Entry {
      name: VACUUM,
      title: "Database vacuum",
      description: "Rebuilds every database without its free space.",
      option: Some("schedules.vacuum"),
      schedule: options.schedules.vacuum,
      note: None,
      never_ran_from: Some(started),
      runnable: true,
    },
    Entry {
      name: RELEASE_CHECK,
      title: "Release check",
      description: "Checks whether a newer version of reseek has been released.",
      option: release_off.is_none().then_some("schedules.release_check"),
      schedule: if release_off.is_some() {
        Schedule::Off
      } else {
        options.schedules.release_check
      },
      note: release_off.map(str::to_string),
      never_ran_from: None,
      runnable: release_off.is_none(),
    },
    Entry {
      name: WISHLIST,
      title: "Wishlist",
      description: "Searches the next wishlist item again, one item each time.",
      option: None,
      schedule: wishlist.map_or(Schedule::Off, Schedule::every),
      note: Some(if wishlist.is_some() {
        "The Soulseek server sets how often".to_string()
      } else {
        "Waits for the Soulseek server to say how often".to_string()
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
  let counted_from = match entry.name {
    // the last complete scan counts, whatever started it, and so does a scheduled one that
    // failed or was canceled, which would else be due again at once
    SHARE_RESCAN => match app.shares.last_trusted_scan().await {
      Ok(trusted) => trusted.max(scheduled),
      Err(e) => {
        warn!(error = %e, "the last share scan not read; the rescan waits");
        return None;
      }
    },
    // what a release check found is in the state, which a restart empties, so a check made
    // before this start does not count
    RELEASE_CHECK => scheduled.filter(|at| *at >= app.jobs.scheduler_started),
    _ => scheduled,
  };
  entry
    .schedule
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
    let last = last_runs.get(entry.name);
    let running = if entry.name == SHARE_RESCAN {
      app.state.current().shares.scanning
    } else {
      lock(&app.jobs.running_schedules).contains(entry.name)
    };
    let next_run_at = due(app, &entry, last, now).await;
    cards.push(JobSchedule {
      name: entry.name.to_string(),
      title: entry.title.to_string(),
      description: entry.description.to_string(),
      every: (entry.schedule != Schedule::Off).then(|| entry.schedule.to_string()),
      option: entry.option.map(str::to_string),
      note: entry.note,
      runnable: entry.runnable,
      running,
      last_run_at: last.and_then(|l| l.run_at),
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
      let busy = lock(&app.jobs.running_schedules).contains(entry.name)
        || (entry.name == SHARE_RESCAN && app.shares.scanning());
      if busy {
        continue;
      }
      let Some(due) = due(&app, &entry, last_runs.get(entry.name), now).await else {
        continue;
      };
      if due <= now {
        start(&app, entry.name, JobStarter::schedule(entry.name)).await;
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
    .find(|entry| entry.name == name)
    .ok_or_else(|| ApiError::not_found(format!("No schedule named {name}")))?;
  if !entry.runnable {
    return Err(ApiError::conflict(
      format!("{} cannot be run now", entry.title),
      entry.note,
    ));
  }
  if entry.name == SHARE_RESCAN {
    return app
      .shares
      .scan(false, ScanOrigin::Asked)
      .map_err(|_| ApiError::conflict("A share scan is already in progress", None));
  }
  if lock(&app.jobs.running_schedules).contains(entry.name) {
    return Err(ApiError::conflict(
      format!("{} is running", entry.title),
      None,
    ));
  }
  start(app, entry.name, JobStarter::you()).await;
  Ok(())
}

/// Starts a run of the schedule `name` by `starter` in the background: a run the schedule
/// started counts as its last scheduled one from its start.
async fn start(app: &AppState, name: &'static str, starter: JobStarter) {
  let scheduled = starter.kind == api_types::jobs::JobStarterKind::Schedule;
  if name == SHARE_RESCAN {
    if let Err(e) = app.shares.scan(false, ScanOrigin::Scheduled) {
      debug!(?e, "scheduled share rescan left to the scan going on");
    }
    return;
  }
  if !lock(&app.jobs.running_schedules).insert(name) {
    return;
  }
  let started_at = Utc::now();
  if scheduled {
    app
      .jobs
      .update_last_run(name, move |last| last.scheduled_at = Some(started_at))
      .await;
  } else {
    app.jobs.schedules_changed.notify_one();
  }
  let app = app.clone();
  tokio::spawn(
    async move {
      let outcome = work(&app, name, starter).await;
      lock(&app.jobs.running_schedules).remove(name);
      app
        .jobs
        .update_last_run(name, move |last| {
          last.run_at = Some(started_at);
          last.ending = Some(outcome.ending);
          last.error = outcome.error;
          last.summary = outcome.summary;
          last.job_id = outcome.job_id;
        })
        .await;
    }
    .instrument(info_span!("schedule", schedule = name)),
  );
}

/// One run of the schedule `name` by `starter`, listed from its start when the user started it.
async fn work(app: &AppState, name: &'static str, starter: JobStarter) -> RunOutcome {
  let listed = starter.kind == api_types::jobs::JobStarterKind::You;
  match name {
    CLEANUP => maintenance::cleanup(app, starter, listed).await,
    VACUUM => maintenance::vacuum(app, starter, listed).await,
    RELEASE_CHECK => release_check(app).await,
    WISHLIST => app.searches.wishlist_turn().await,
    other => unreachable!("no schedule runs {other}"),
  }
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

/// Runs `work` every `period` until the daemon stops, first at once; a run missed while the
/// runtime was busy is delayed, not run twice. This is the daemon's own upkeep, which is not a
/// job.
pub fn upkeep<F, Fut>(app: &AppState, period: Duration, name: &'static str, work: F)
where
  F: Fn() -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let shutdown = app.shutdown.clone();
  tokio::spawn(
    async move {
      let mut ticks = tokio::time::interval(period);
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

  /// The entry `name` running every `every`, as if its option set that.
  fn entry(name: &'static str, every: Duration) -> Entry {
    Entry {
      name,
      title: name,
      description: "",
      option: None,
      schedule: Schedule::every(every),
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
    let rescan = entry(SHARE_RESCAN, half_day);
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
    let release = entry(RELEASE_CHECK, half_day);
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
