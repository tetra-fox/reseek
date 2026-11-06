//! The jobs engine: the states and endings a job goes through and its end told once, what a
//! restart does to each kind, the list's order, and the history kept within `retention.jobs`.

use std::sync::Arc;

use api_types::events::{Event, EventType};
use api_types::jobs::{
  JobCounts, JobEnding, JobKind, JobListQuery, JobPartState, JobStarter, JobStarterKind, JobState,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::db::Location;

/// A jobs engine over `location`, with the `JobFinished` events it raises.
async fn engine(location: &Location) -> (Arc<Jobs>, mpsc::UnboundedReceiver<Event>) {
  let events = EventBus::new();
  let (told, finished) = mpsc::unbounded_channel();
  events.subscribe("test", Some(EventType::JobFinished), move |record| {
    // the test reads what it needs; the rest go when it ends
    let _ = told.send(record.event.clone());
    async { Ok(()) }
  });
  let jobs = Jobs::open(
    location,
    Live::new(CancellationToken::new()),
    events,
    crate::testing::state(),
  )
  .await
  .unwrap();
  (jobs, finished)
}

fn counts(total: u32, succeeded: u32, failed: u32, canceled: u32) -> JobCounts {
  JobCounts {
    total,
    succeeded,
    failed,
    canceled,
    paused: 0,
  }
}

#[test]
fn a_job_ends_as_its_parts_did() {
  let ending = |c: JobCounts| c.ending();
  assert_eq!(ending(counts(3, 3, 0, 0)), JobEnding::Succeeded);
  assert_eq!(
    ending(counts(3, 2, 0, 1)),
    JobEnding::Succeeded,
    "a canceled part was the user's choice"
  );
  assert_eq!(ending(counts(3, 2, 1, 0)), JobEnding::FinishedWithFailures);
  assert_eq!(ending(counts(3, 0, 2, 1)), JobEnding::Failed);
  assert_eq!(ending(counts(2, 0, 0, 2)), JobEnding::Canceled);
  assert_eq!(
    ending(counts(0, 0, 0, 0)),
    JobEnding::Succeeded,
    "nothing to do"
  );
}

#[tokio::test]
async fn a_job_is_ended_and_told_of_once_and_then_stays_as_it_ended() {
  let (jobs, mut finished) = engine(&Location::memory()).await;
  let job = jobs
    .create(NewJob::running(
      JobKind::Maintenance,
      "Cleanup",
      JobStarter::you(),
    ))
    .await
    .unwrap();
  assert_eq!(jobs.state.current().jobs.running, 1);
  let ended = jobs
    .end(
      job.id,
      Ended {
        counts: Some(counts(3, 2, 1, 0)),
        summary: Some("Removed 2 events".into()),
        ..Ended::default()
      },
    )
    .await
    .unwrap()
    .expect("a running job ends");
  assert_eq!(ended.state, JobState::Ended);
  assert_eq!(ended.ending, Some(JobEnding::FinishedWithFailures));
  assert!(!ended.pausable);
  assert!(ended.ended_at.is_some());
  let summary = jobs.state.current().jobs.clone();
  assert_eq!(summary.running, 0);
  assert_eq!(
    summary.last_failed_at, ended.ended_at,
    "a failure marks the bar"
  );

  let again = Ended {
    ending: Some(JobEnding::Succeeded),
    ..Ended::default()
  };
  assert!(
    jobs.end(job.id, again).await.unwrap().is_none(),
    "ended once"
  );
  assert!(
    jobs
      .update(job.id, |job| job.state = JobState::Running)
      .await
      .unwrap()
      .is_none(),
    "an ended job does not change"
  );
  let Some(Event::JobFinished(told)) = finished.recv().await else {
    panic!("JobFinished is raised");
  };
  assert_eq!(
    (told.id, told.ending),
    (job.id, JobEnding::FinishedWithFailures)
  );
  assert_eq!(told.summary.as_deref(), Some("Removed 2 events"));
  assert!(
    tokio::time::timeout(std::time::Duration::from_millis(200), finished.recv())
      .await
      .is_err(),
    "told of once"
  );
}

#[tokio::test]
async fn a_restart_fails_scans_and_maintenance_and_analysis_files_start_over() {
  let dir = tempfile::tempdir().unwrap();
  let location = Location {
    data: dir.path().to_path_buf(),
    backups: dir.path().join("backups"),
    volatile: false,
    log_sql: false,
  };
  let (before, _) = engine(&location).await;
  let scan = before
    .create(NewJob::running(
      JobKind::Scan,
      "Share scan",
      JobStarter::of(JobStarterKind::Resumed),
    ))
    .await
    .unwrap();
  let cleanup = before
    .create(NewJob::running(
      JobKind::Maintenance,
      "Cleanup",
      JobStarter::schedule("cleanup"),
    ))
    .await
    .unwrap();
  let download = before
    .create(NewJob::running(
      JobKind::Download,
      "Album from alice",
      JobStarter::you(),
    ))
    .await
    .unwrap();
  let part = |name: &str, state| NewPart {
    name: name.to_string(),
    state,
    error: None,
    target: None,
    key: None,
  };
  let analysis = before
    .create_with_parts(
      NewJob {
        state: JobState::Running,
        ..NewJob::running(JobKind::Analysis, "Analyze Album", JobStarter::you())
      },
      vec![
        part("1.flac", JobPartState::Succeeded),
        part("2.flac", JobPartState::Running),
        part("3.flac", JobPartState::Waiting),
      ],
    )
    .await
    .unwrap()
    .0;
  before.database().close().await;
  drop(before);

  let (after, _) = engine(&location).await;
  for cut in [scan.id, cleanup.id] {
    let job = after.get(cut).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Ended);
    assert_eq!(job.ending, Some(JobEnding::Failed));
    assert_eq!(job.error.as_deref(), Some(STOPPED));
  }
  let carried = after.get(download.id).await.unwrap().unwrap();
  assert_eq!(carried.state, JobState::Running, "a download job goes on");
  let detail = after.detail(analysis.id).await.unwrap().unwrap();
  assert_eq!(
    detail.job.state,
    JobState::Running,
    "an analysis job goes on"
  );
  let states: Vec<JobPartState> = detail.parts.iter().map(|p| p.state).collect();
  assert_eq!(
    states,
    [
      JobPartState::Succeeded,
      JobPartState::Waiting,
      JobPartState::Waiting
    ],
    "the file under analysis starts over"
  );
}

#[tokio::test]
async fn the_list_runs_first_then_waits_then_ends_newest_first_and_filters() {
  let (jobs, _) = engine(&Location::memory()).await;
  let make = |kind, name: &str, state| NewJob {
    state,
    ..NewJob::running(kind, name, JobStarter::you())
  };
  let old = jobs
    .create(make(JobKind::Maintenance, "Cleanup", JobState::Running))
    .await
    .unwrap();
  jobs.end(old.id, Ended::default()).await.unwrap();
  let queued = jobs
    .create(make(JobKind::Analysis, "Analyze B", JobState::Queued))
    .await
    .unwrap();
  let newer = jobs
    .create(make(JobKind::Scan, "Share scan", JobState::Running))
    .await
    .unwrap();
  jobs.end(newer.id, Ended::default()).await.unwrap();
  let running = jobs
    .create(make(
      JobKind::Download,
      "Album from alice",
      JobState::Running,
    ))
    .await
    .unwrap();
  let ids: Vec<_> = jobs
    .list(JobListQuery::default())
    .await
    .unwrap()
    .into_iter()
    .map(|j| j.id)
    .collect();
  assert_eq!(ids, [running.id, queued.id, newer.id, old.id]);
  let only = |query| async { jobs.list(query).await.unwrap().len() };
  assert_eq!(
    only(JobListQuery {
      kind: Some(JobKind::Analysis),
      ..JobListQuery::default()
    })
    .await,
    1
  );
  assert_eq!(
    only(JobListQuery {
      q: Some("ALBUM".into()),
      ..JobListQuery::default()
    })
    .await,
    1,
    "the name is searched ignoring case"
  );
  assert_eq!(
    only(JobListQuery {
      state: Some(JobState::Ended),
      ..JobListQuery::default()
    })
    .await,
    2
  );
}

#[tokio::test]
async fn history_keeps_the_newest_ended_jobs_within_its_age_and_count() {
  let (jobs, _) = engine(&Location::memory()).await;
  let mut ended = Vec::new();
  for n in 0..4 {
    let job = jobs
      .create(NewJob::running(
        JobKind::Maintenance,
        format!("Run {n}"),
        JobStarter::you(),
      ))
      .await
      .unwrap();
    jobs.end(job.id, Ended::default()).await.unwrap();
    ended.push(job.id);
  }
  let running = jobs
    .create(NewJob::running(
      JobKind::Scan,
      "Share scan",
      JobStarter::you(),
    ))
    .await
    .unwrap();
  // the first two ended an hour ago
  for id in &ended[..2] {
    let at = crate::db::timestamp(Utc::now() - chrono::Duration::hours(1));
    let id = id.to_string();
    jobs
      .database()
      .write(move |conn| {
        conn.execute(
          "UPDATE jobs SET ended_at = ?2 WHERE id = ?1",
          rusqlite::params![id, at],
        )
      })
      .await
      .unwrap();
  }
  assert_eq!(
    jobs.prune(None, None).await.unwrap(),
    0,
    "no limit, none go"
  );
  assert_eq!(
    jobs
      .prune(Some(chrono::Duration::minutes(30)), None)
      .await
      .unwrap(),
    2,
    "the two past the age"
  );
  assert_eq!(
    jobs.prune(None, Some(1)).await.unwrap(),
    1,
    "past the count"
  );
  let left: Vec<_> = jobs
    .list(JobListQuery::default())
    .await
    .unwrap()
    .into_iter()
    .map(|j| j.id)
    .collect();
  assert_eq!(
    left,
    [running.id, ended[3]],
    "a running job is never pruned"
  );
  assert!(jobs.remove(vec![running.id]).await.unwrap().is_empty());
  assert_eq!(jobs.clear_ended().await.unwrap(), 1);
}
