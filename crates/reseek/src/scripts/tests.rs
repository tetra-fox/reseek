use std::time::Duration;

use api_types::events::ScriptRunCompleted;
use tokio::sync::mpsc;

use super::*;
use crate::testing::{TestApp, app, request};

/// The daemon with `scripts`, the `[integrations.scripts.*]` tables, and the runs it records.
async fn with_scripts(scripts: &str) -> (TestApp, mpsc::UnboundedReceiver<ScriptRunCompleted>) {
  let config = format!("[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n{scripts}");
  let app = app(&config, &[]).await;
  let (sender, runs) = mpsc::unbounded_channel();
  app
    .state
    .events
    .subscribe("runs", Some(EventType::ScriptRunCompleted), move |record| {
      if let Event::ScriptRunCompleted(run) = &record.event {
        sender.send(run.clone()).unwrap();
      }
      async { Ok(()) }
    });
  (app, runs)
}

/// Raises a sample of `kind` through the API, with every text in it starting with `d`.
async fn raise(app: &TestApp, kind: &str, d: &str) -> uuid::Uuid {
  let answer = app
    .send(request(
      "POST",
      &format!("/api/v0/events/{kind}"),
      Some(serde_json::json!(d)),
    ))
    .await;
  assert_eq!(answer.status, 201, "{:?}", answer.body);
  answer.json()["id"].as_str().unwrap().parse().unwrap()
}

async fn next(runs: &mut mpsc::UnboundedReceiver<ScriptRunCompleted>) -> ScriptRunCompleted {
  tokio::time::timeout(Duration::from_secs(20), runs.recv())
    .await
    .expect("the script ran")
    .unwrap()
}

/// A script running `command` in `sh` on the events `on`, with `extra` options.
fn script(name: &str, on: &str, command: &str, extra: &str) -> String {
  format!(
    "[integrations.scripts.{name}]\non = [{on}]\n{extra}\n[integrations.scripts.{name}.run]\n\
     executable = \"/bin/sh\"\narglist = [\"-c\", {command:?}]\n"
  )
}

#[test]
fn a_script_runs_on_its_types_and_wildcards_but_never_on_its_own_record() {
  let on = |names: &[&str]| names.iter().map(ToString::to_string).collect::<Vec<_>>();
  assert!(runs_on(
    &on(&["downloadfilecomplete"]),
    EventType::DownloadFileComplete
  ));
  assert!(!runs_on(
    &on(&["DownloadFileComplete"]),
    EventType::DownloadFileFailed
  ));
  for wildcard in ["any", "ALL", "*"] {
    assert!(runs_on(&on(&[wildcard]), EventType::Noop));
    assert!(!runs_on(&on(&[wildcard]), EventType::ScriptRunCompleted));
  }
  assert!(
    !runs_on(&on(&[]), EventType::Noop),
    "an empty list never runs"
  );
}

#[tokio::test]
async fn a_download_s_outcome_reaches_a_script_and_each_run_is_recorded() {
  let command = r#"echo "$RESEEK_SCRIPT_RESULT|$RESEEK_SCRIPT_REASON|$RESEEK_SCRIPT_REMOTE_PATH|$RESEEK_SCRIPT_LOCAL_PATH|$RESEEK_SCRIPT_EVENT_TYPE"
echo diagnostics >&2
test "$RESEEK_SCRIPT_RESULT" = succeeded || exit 3"#;
  let (app, mut runs) = with_scripts(&script(
    "outcome",
    r#""DownloadFileComplete", "downloadfilefailed""#,
    command,
    "",
  ))
  .await;

  let id = raise(&app, "DownloadFileComplete", "a/").await;
  let succeeded = next(&mut runs).await;
  assert_eq!(
    (
      succeeded.event_id,
      succeeded.script.as_str(),
      succeeded.event_type
    ),
    (id, "outcome", EventType::DownloadFileComplete)
  );
  assert!(succeeded.succeeded && !succeeded.timed_out);
  assert_eq!((succeeded.exit_code, succeeded.error), (Some(0), None));
  assert_eq!(
    succeeded.output, "succeeded||a/remote.file|a/local.file|DownloadFileComplete\ndiagnostics",
    "stderr is output too, not a failure"
  );

  raise(&app, "DownloadFileFailed", "b/").await;
  let failed = next(&mut runs).await;
  assert!(!failed.succeeded);
  assert_eq!(failed.exit_code, Some(3));
  assert!(
    failed
      .output
      .starts_with("failed|b/reason|b/remote.file|b/local.file|DownloadFileFailed"),
    "{}",
    failed.output
  );

  raise(&app, "Noop", "").await;
  tokio::time::sleep(Duration::from_millis(200)).await;
  assert!(
    runs.try_recv().is_err(),
    "a type the script is not on runs nothing"
  );
}

#[tokio::test]
async fn a_script_past_its_timeout_is_killed_with_what_it_started() {
  let command = r#"sleep 30 & echo $! > started.pid; echo waiting; sleep 30"#;
  let (app, mut runs) =
    with_scripts(&script("slow", r#""Any""#, command, "timeout = \"300ms\"")).await;
  raise(&app, "Noop", "").await;
  let run = next(&mut runs).await;
  assert!(run.timed_out && !run.succeeded);
  assert_eq!(run.exit_code, None);
  assert_eq!(run.error.as_deref(), Some("timed out after 300ms"));
  assert_eq!(run.output, "waiting");
  assert!(run.duration_ms < 5_000, "{}", run.duration_ms);
  let scripts = app.state.paths.scripts.clone();
  let pid: i32 = std::fs::read_to_string(scripts.join("started.pid"))
    .unwrap()
    .trim()
    .parse()
    .unwrap();
  let pid = rustix::process::Pid::from_raw(pid).unwrap();
  let started = std::time::Instant::now();
  // the killed process lingers until its new parent reaps it
  while rustix::process::test_kill_process(pid).is_ok() {
    assert!(
      started.elapsed() < Duration::from_secs(10),
      "what the script started still runs"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}

#[tokio::test]
async fn an_event_too_large_for_the_environment_is_handed_over_in_a_file() {
  let command = r#"test -f "$RESEEK_SCRIPT_DATA_FILE" && echo "$RESEEK_SCRIPT_DATA" && wc -c < "$RESEEK_SCRIPT_DATA_FILE""#;
  let (app, mut runs) = with_scripts(&script(
    "large",
    r#""Noop", "DownloadFileFailed""#,
    command,
    "",
  ))
  .await;
  let large = "x".repeat(60 * 1024);
  let id = raise(&app, "DownloadFileFailed", &large).await;
  let run = next(&mut runs).await;
  assert!(run.succeeded, "{run:?}");
  let mut lines = run.output.lines();
  let envelope: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
  assert_eq!(envelope["id"], id.to_string());
  assert_eq!(envelope["type"], "DownloadFileFailed");
  assert_eq!(
    (
      envelope["data"]["version"].clone(),
      envelope["data"]["truncated"].clone()
    ),
    (0.into(), true.into())
  );
  let size: usize = lines.next().unwrap().trim().parse().unwrap();
  assert!(
    size > 3 * large.len(),
    "the whole event is in the file: {size}"
  );
  let left: Vec<_> = std::fs::read_dir(&app.state.paths.scripts)
    .unwrap()
    .filter_map(Result::ok)
    .filter(|entry| entry.file_name().to_string_lossy().starts_with(".event-"))
    .collect();
  assert!(left.is_empty(), "the file goes with the run");
}

#[tokio::test]
async fn a_script_that_cannot_start_is_recorded_with_why() {
  let config = "[integrations.scripts.missing]\non = [\"Any\"]\n\
                [integrations.scripts.missing.run]\nexecutable = \"/nonexistent/program\"\n";
  let (app, mut runs) = with_scripts(config).await;
  raise(&app, "Noop", "").await;
  let run = next(&mut runs).await;
  assert!(!run.succeeded && !run.timed_out);
  assert_eq!(run.exit_code, None);
  assert!(
    run
      .error
      .as_deref()
      .unwrap()
      .starts_with("/nonexistent/program could not be started"),
    "{run:?}"
  );
  tokio::time::sleep(Duration::from_millis(200)).await;
  assert!(runs.try_recv().is_err(), "its own record runs no script");
}

/// A run of nothing for the event numbered `n`.
fn job(n: u32) -> Job {
  Job {
    name: "busy".into(),
    script: Box::default(),
    record: Arc::new(EventRecord {
      id: uuid::Uuid::from_u128(n.into()),
      timestamp: chrono::Utc::now(),
      event: Event::Noop(api_types::events::NoFields { version: 0 }),
    }),
  }
}

#[test]
fn a_full_queue_drops_the_oldest_run() {
  let queue = Queue::default();
  for n in 0..=WAITING as u32 {
    assert!(queue.push(job(n)).is_ok());
  }
  let waiting = lock(&queue.waiting);
  assert_eq!(waiting.jobs.len(), WAITING);
  assert_eq!(
    waiting.jobs.front().unwrap().record.id,
    uuid::Uuid::from_u128(1)
  );
  assert_eq!(queue.dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn a_closed_queue_takes_its_script_back_until_its_task_ends() {
  let queue = Queue::holding(job(1));
  queue.close();
  assert!(
    queue.push(job(2)).is_ok(),
    "added back before its task ended"
  );
  for n in [1, 2] {
    assert!(matches!(queue.next(), Next::Run(job) if job.record.id == uuid::Uuid::from_u128(n)));
  }
  assert!(
    matches!(queue.next(), Next::Wait),
    "open again, so its task waits"
  );
  queue.close();
  assert!(matches!(queue.next(), Next::End));
  assert!(queue.ended());
  assert!(
    queue.push(job(3)).is_err(),
    "a queue whose task ended takes no run"
  );
}

#[tokio::test]
async fn a_run_the_shutdown_kills_is_in_the_event_log() {
  let command = "echo started; touch running; sleep 30";
  let (app, _runs) = with_scripts(&script("slow", r#""Noop""#, command, "")).await;
  let id = raise(&app, "Noop", "").await;
  let running = app.state.paths.scripts.join("running");
  let started = std::time::Instant::now();
  while !running.exists() {
    assert!(
      started.elapsed() < Duration::from_secs(20),
      "the script never started"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  app.state.shutdown.cancel();
  app.state.stop().await;
  let query: api_types::events::EventsQuery =
    serde_json::from_value(serde_json::json!({ "type": "ScriptRunCompleted" })).unwrap();
  let logged = app.state.services.events.list(query).await.unwrap().items;
  let [record] = &logged[..] else {
    panic!("one run recorded: {logged:?}");
  };
  let Event::ScriptRunCompleted(run) = &record.event else {
    unreachable!("the query asked for script runs");
  };
  assert_eq!(run.event_id, id);
  assert!(!run.succeeded && !run.timed_out && run.exit_code.is_none());
  assert_eq!(run.error.as_deref(), Some("stopped with the daemon"));
  assert_eq!(run.output, "started");
}

#[tokio::test]
async fn a_script_taken_out_of_the_options_lets_its_task_end_at_once() {
  let (app, mut runs) = with_scripts(&script("gone", r#""Noop""#, "true", "")).await;
  raise(&app, "Noop", "").await;
  next(&mut runs).await;
  std::fs::write(
    app.dir.path().join("config.toml"),
    "[web.authentication]\ntrusted_networks = [\"127.0.0.0/8\"]\n",
  )
  .unwrap();
  app.state.config.reload().await.unwrap();
  let started = std::time::Instant::now();
  // no event comes after the change, so only the options' own change can close the queue
  loop {
    let ended = lock(&app.state.scripts.queues)
      .get("gone")
      .is_none_or(|queue| queue.ended());
    if ended {
      break;
    }
    assert!(
      started.elapsed() < Duration::from_secs(10),
      "its task still waits"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}
