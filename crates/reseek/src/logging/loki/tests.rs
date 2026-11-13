use std::collections::VecDeque;
use std::sync::Mutex;

use axum::http::StatusCode;
use reseek_shares::lock;
use tracing_subscriber::layer::SubscriberExt;

use super::*;

/// A stand-in Loki answering each push with the next of `answers`, then 204, and handing over
/// every body it was sent.
async fn receiver(answers: &[StatusCode]) -> (String, mpsc::UnboundedReceiver<serde_json::Value>) {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}/", listener.local_addr().unwrap());
  let (sender, pushes) = mpsc::unbounded_channel();
  let answers = Arc::new(Mutex::new(VecDeque::from(answers.to_vec())));
  let push = move |axum::Json(body): axum::Json<serde_json::Value>| {
    sender.send(body).unwrap();
    let answer = lock(&answers).pop_front().unwrap_or(StatusCode::NO_CONTENT);
    async move { answer }
  };
  let app = axum::Router::new().route("/loki/api/v1/push", axum::routing::post(push));
  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
  (url, pushes)
}

async fn next(pushes: &mut mpsc::UnboundedReceiver<serde_json::Value>) -> serde_json::Value {
  tokio::time::timeout(Duration::from_secs(10), pushes.recv())
    .await
    .expect("a push came")
    .unwrap()
}

/// Logs what `log` logs, inside a process span, into `queue` with `labels` taking its fields.
fn logged(labels: &Labels, queue: Queue, log: impl FnOnce()) {
  let subscriber = tracing_subscriber::registry()
    .with(labels.clone())
    .with(lines(queue));
  tracing::subscriber::with_default(subscriber, || {
    let process = tracing::info_span!(
      PROCESS_SPAN,
      instance_name = "default",
      invocation_id = %uuid::Uuid::nil(),
      pid = 42u32
    );
    let _in = process.enter();
    log();
  });
}

/// The lines of the stream of `level` in `body`.
fn lines_of(body: &serde_json::Value, level: &str) -> Vec<String> {
  body["streams"]
    .as_array()
    .unwrap()
    .iter()
    .find(|stream| stream["stream"]["level"] == level)
    .map(|stream| {
      stream["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value[1].as_str().unwrap().to_string())
        .collect()
    })
    .unwrap_or_default()
}

async fn client() -> reqwest::Client {
  crate::http::outbound::client(None).await.unwrap()
}

#[tokio::test]
async fn lines_go_in_one_batch_per_push_labeled_by_the_process() {
  let (url, mut pushes) = receiver(&[]).await;
  let labels = Labels::default();
  let (queue, sink) = channel(&url, labels.clone(), 16);
  logged(&labels, queue, || {
    tracing::info!(username = "alice", "logged in");
    tracing::warn!("disconnected");
    tracing::info!("logged in again");
  });
  let running = sink.start(client().await);
  let body = next(&mut pushes).await;
  running.finish().await;
  let stream = &body["streams"][0]["stream"];
  assert_eq!(
    stream,
    &serde_json::json!({
      "app": "reseek",
      "instance_name": "default",
      "invocation_id": uuid::Uuid::nil().to_string(),
      "pid": "42",
      "level": "info"
    })
  );
  let info = lines_of(&body, "info");
  assert_eq!(info.len(), 2);
  assert_eq!(
    info[0], "[INF] [reseek::logging::loki::tests] logged in username=\"alice\"",
    "the file's format without its time, and without the process span"
  );
  assert_eq!(lines_of(&body, "warn").len(), 1);
  let time: u128 = body["streams"][0]["values"][0][0]
    .as_str()
    .unwrap()
    .parse()
    .unwrap();
  assert!(
    time > 1_700_000_000_000_000_000,
    "nanoseconds since the epoch: {time}"
  );
  assert!(pushes.try_recv().is_err(), "one push for all three");
}

#[tokio::test]
async fn a_full_queue_drops_lines_without_waiting_and_says_how_many() {
  let (url, mut pushes) = receiver(&[]).await;
  let labels = Labels::default();
  let (queue, sink) = channel(&url, labels.clone(), 2);
  // nothing takes from the queue yet, so logging would wait here if it could
  logged(&labels, queue, || {
    for n in 0..5 {
      tracing::info!(n, "line");
    }
  });
  let running = sink.start(client().await);
  let body = next(&mut pushes).await;
  running.finish().await;
  assert_eq!(lines_of(&body, "info").len(), 2);
  assert_eq!(
    lines_of(&body, "warn"),
    ["[WRN] 3 log lines dropped before they reached Loki; its queue was full"]
  );
}

#[tokio::test]
async fn a_push_loki_refuses_is_tried_again() {
  let (url, mut pushes) = receiver(&[StatusCode::SERVICE_UNAVAILABLE]).await;
  let labels = Labels::default();
  let (queue, sink) = channel(&url, labels.clone(), 16);
  logged(&labels, queue, || tracing::error!("disk full"));
  let running = sink.start(client().await);
  let refused = next(&mut pushes).await;
  let again = next(&mut pushes).await;
  running.finish().await;
  assert_eq!(refused, again);
  assert_eq!(
    lines_of(&again, "error"),
    ["[ERR] [reseek::logging::loki::tests] disk full"]
  );
}

#[tokio::test]
async fn what_is_queued_when_the_daemon_stops_is_pushed() {
  let (url, mut pushes) = receiver(&[]).await;
  let labels = Labels::default();
  let (queue, sink) = channel(&url, labels.clone(), 16);
  let running = sink.start(client().await);
  logged(&labels, queue, || tracing::info!("reseek stopped"));
  running.finish().await;
  let body = next(&mut pushes).await;
  assert_eq!(lines_of(&body, "info").len(), 1);
}
