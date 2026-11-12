//! The agent's share uploads against a controller the test plays: the socket's frames come to
//! the test, and the uploads go to a server that answers as the test says. A re-upload that
//! fails is tried again with no change to ask for it, a first upload refused as too large
//! copies nothing and is a refusal, and an upload stops when its socket closes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use api_types::relay::{RelayErrorCode, RelayMessage};
use cascade::{Layers, Watcher};
use reseek_relay_proto::frames;
use reseek_shares::{Setup, Shares, lock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::AgentClient;
use super::dial::Settings;
use super::session::Link;
use crate::options::Options;

/// How the test's controller answers a share upload.
#[derive(Clone, Copy)]
enum Answer {
  /// reads the upload and answers with this status
  Status(u16),
  /// takes the connection and never answers
  Silent,
}

/// An agent sharing one file, logged in on a socket whose frames the test reads.
struct Agent {
  dir: tempfile::TempDir,
  agent: Arc<AgentClient>,
  link: Arc<Link>,
  frames: mpsc::Receiver<String>,
  /// the share uploads the test's controller took
  posts: Arc<AtomicUsize>,
  shutdown: CancellationToken,
}

impl Drop for Agent {
  fn drop(&mut self) {
    self.shutdown.cancel();
  }
}

impl Agent {
  /// The next frame the agent sent, without its pings.
  async fn next(&mut self) -> RelayMessage {
    let text = tokio::time::timeout(Duration::from_secs(10), self.frames.recv())
      .await
      .expect("the agent sends a frame")
      .expect("the socket is open");
    frames::decode(&text).unwrap().message
  }

  /// Answers the agent's next ask for a share upload token with `answer`.
  async fn answer_ask(&mut self, answer: RelayMessage) {
    let asked = self.next().await;
    assert!(
      matches!(asked, RelayMessage::SharesBegin { .. }),
      "{asked:?}"
    );
    self.agent.handle(&self.link, &frames::encode(None, answer));
  }

  fn copied(&self) -> bool {
    self.dir.path().join("uploads/share_index.db").exists()
  }
}

/// A token for a share upload.
fn token() -> RelayMessage {
  RelayMessage::SharesToken {
    token: Uuid::new_v4(),
    expires_in: 300_000,
  }
}

/// Waits until `check` holds, for at most ten seconds.
async fn eventually(check: impl Fn() -> bool) {
  for _ in 0..200 {
    if check() {
      return;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  panic!("never held");
}

/// A controller's HTTP side that answers its share uploads with `answers` in turn, counting
/// them in `posts`; its address.
async fn controller(answers: Vec<Answer>, posts: Arc<AtomicUsize>) -> String {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    let mut answers = answers.into_iter();
    loop {
      let (mut connection, _) = listener.accept().await.unwrap();
      posts.fetch_add(1, Ordering::SeqCst);
      let answer = answers.next().expect("no more uploads than answers");
      tokio::spawn(async move {
        let Answer::Status(status) = answer else {
          // held open, never answered
          return std::future::pending::<()>().await;
        };
        read_request(&mut connection).await;
        let head =
          format!("HTTP/1.1 {status} Answered\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        connection.write_all(head.as_bytes()).await.unwrap();
      });
    }
  });
  format!("http://{address}/")
}

/// Reads one request with a `content-length` from `connection`.
async fn read_request(connection: &mut tokio::net::TcpStream) {
  let mut read = Vec::new();
  let mut buffer = [0u8; 8192];
  let (head, length) = loop {
    let n = connection.read(&mut buffer).await.unwrap();
    assert!(n > 0, "the request ends early");
    read.extend_from_slice(&buffer[..n]);
    if let Some(end) = read.windows(4).position(|w| w == b"\r\n\r\n") {
      let head = String::from_utf8_lossy(&read[..end]).to_lowercase();
      let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map(|value| value.trim().parse::<usize>().unwrap())
        .expect("the upload has a length");
      break (end + 4, length);
    }
  };
  while read.len() < head + length {
    let n = connection.read(&mut buffer).await.unwrap();
    assert!(n > 0, "the request ends early");
    read.extend_from_slice(&buffer[..n]);
  }
}

/// An agent sharing one file in `dir/share`, with its controller's uploads answered by
/// `answers`, logged in on a socket that had an upload when `ready`.
async fn agent(answers: Vec<Answer>, ready: bool) -> Agent {
  let dir = tempfile::tempdir().unwrap();
  let posts = Arc::new(AtomicUsize::new(0));
  let address = controller(answers, posts.clone()).await;
  let share = dir.path().join("share");
  std::fs::create_dir_all(&share).unwrap();
  std::fs::write(share.join("one.flac"), "one").unwrap();
  let config = format!(
    "instance_name = \"nas\"\n\
     [relay.controller]\naddress = \"{address}\"\napi_key = \"agents-key-0123456789\"\n\
     secret = \"nas-secret-0123456789\"\n[shares]\ndirectories = [\"{}\"]\n",
    share.display()
  );
  std::fs::write(dir.path().join("config.toml"), config).unwrap();
  let config = Watcher::new(Layers::<Options>::new().files(dir.path(), ["config.toml"])).unwrap();
  let path = |name: &str| {
    let made = dir.path().join(name);
    std::fs::create_dir_all(&made).unwrap();
    made
  };
  let setup = Setup {
    cache: path("cache"),
    backups: path("backups"),
    downloads: path("downloads"),
    scan_at_startup: true,
    force_scan: false,
    volatile: false,
    case_sensitive: false,
    rescan_on_schedule: false,
    browse_caches: false,
    scans: Box::new(|_| Box::pin(async {})),
    publish: Box::new(|_| {}),
  };
  let shutdown = CancellationToken::new();
  let shares = Shares::open(&config, |o: &Options| &o.shares, setup, shutdown.clone())
    .await
    .unwrap();
  shares.initialized().await;
  assert!(!shares.loading_or_scanning() && shares.index_filled());
  let settings = Arc::new(Settings::new(&config.current().values).unwrap());
  let uploads = path("uploads");
  let agent = AgentClient::new(config, shares, path("received"), uploads, shutdown.clone());
  let (link, frames) = Link::opened(settings, ready);
  *lock(&agent.link) = Some(link.clone());
  Agent {
    dir,
    agent,
    link,
    frames,
    posts,
    shutdown,
  }
}

#[tokio::test]
async fn a_failed_upload_after_one_succeeded_is_tried_again_without_another_change() {
  let mut agent = agent(vec![Answer::Status(502), Answer::Status(200)], true).await;
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  eventually(|| agent.link.sync_failures() == 1).await;
  // nothing changed locally; the wait after the failure asks again
  agent.answer_ask(token()).await;
  eventually(|| {
    agent.posts.load(Ordering::SeqCst) == 2
      && agent.link.sync_failures() == 0
      && !agent.agent.syncing.load(Ordering::SeqCst)
  })
  .await;
  assert!(!agent.link.is_closed());
}

#[tokio::test]
async fn a_first_upload_refused_as_too_large_copies_nothing_and_is_a_refusal() {
  let mut agent = agent(Vec::new(), false).await;
  agent.agent.request_sync();
  let asked = agent.next().await;
  let RelayMessage::SharesBegin { database_bytes, .. } = asked else {
    panic!("{asked:?}");
  };
  assert!(database_bytes > 0, "the ask carries the index's size");
  assert!(
    !agent.copied(),
    "the index is copied only once a token came"
  );
  let too_large = RelayMessage::Error {
    code: RelayErrorCode::TooLarge,
    detail: Some("at most 1 byte".into()),
    id: None,
  };
  agent
    .agent
    .handle(&agent.link, &frames::encode(None, too_large));
  eventually(|| agent.link.is_closed()).await;
  let refusal = agent.link.refusal().expect("the refusal is kept");
  assert!(refusal.contains("TooLarge"), "{refusal}");
  assert!(!agent.copied());
  assert_eq!(agent.posts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_upload_stops_when_its_socket_closes() {
  let mut agent = agent(vec![Answer::Silent], true).await;
  agent.agent.request_sync();
  agent.answer_ask(token()).await;
  eventually(|| agent.posts.load(Ordering::SeqCst) == 1).await;
  assert!(agent.agent.syncing.load(Ordering::SeqCst));
  agent.link.close(frames::close::NORMAL);
  eventually(|| !agent.agent.syncing.load(Ordering::SeqCst)).await;
  assert_eq!(
    agent.link.sync_failures(),
    0,
    "a closed socket is no failure"
  );
}
