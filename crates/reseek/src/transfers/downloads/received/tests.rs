//! What lets a file sent unasked in, and what the engine does with offers: a file whose
//! download from its sender is unfinished, or was canceled within the hour, is refused before any
//! job is made, and a sender is held to the files they may send at once even when their offers
//! all come together.

use std::net::IpAddr;
use std::sync::Mutex;

use api_types::transfers::TransferOutcome;
use chrono::Utc;
use soulseek_client::ClientError;

use super::*;
use crate::testing::{TRUSTED, TestApp, app, sent_jobs};
use crate::transfers::store;

/// Everyone may send files, two at a time.
fn everyone() -> String {
  format!("{TRUSTED}[transfers.download.received]\nfrom = [\"default\"]\nmax_files = 2\n")
}

/// The address the offers in these tests come from.
const HERE: [u8; 4] = [10, 0, 0, 1];

/// Writes `record` into the daemon's `transfers.db`, as an enqueue writes it.
async fn insert(app: &TestApp, record: &Record) {
  let mut record = record.clone();
  app
    .state
    .transfers
    .write(move |conn| {
      record.queue_position = Some(store::next_position(conn)?);
      store::insert(conn, &record)
    })
    .await
    .unwrap();
}

/// A download of `filename` from alice that ended canceled `ago`.
fn canceled(filename: &str, ago: chrono::Duration) -> Record {
  let mut record = Record::download("alice", filename, 3);
  record.complete(TransferOutcome::Cancelled, None).unwrap();
  record.ended_at = Some(Utc::now() - ago);
  record
}

#[tokio::test]
async fn a_file_not_finished_or_canceled_lately_is_refused_before_any_job() {
  let app = app(&everyone(), &[]).await;
  let downloads = &app.state.downloads;
  let mut paused = Record::download("alice", "@@s\\paused.flac", 3);
  paused.paused = true;
  insert(&app, &paused).await;
  insert(
    &app,
    &canceled("@@s\\canceled.flac", chrono::Duration::minutes(5)),
  )
  .await;
  for file in ["@@s\\paused.flac", "@@s\\canceled.flac"] {
    let refused = downloads.receive("alice", HERE.into(), file, 3).await;
    assert!(
      matches!(refused, Err(ClientError::TransferRejected(_))),
      "{file}: {refused:?}"
    );
  }
  assert_eq!(sent_jobs(&app).await, 0, "a refused offer makes no job");

  insert(
    &app,
    &canceled("@@s\\long ago.flac", chrono::Duration::hours(2)),
  )
  .await;
  let mut removed = canceled("@@s\\removed.flac", chrono::Duration::minutes(5));
  removed.removed = true;
  insert(&app, &removed).await;
  for file in ["@@s\\long ago.flac", "@@s\\removed.flac"] {
    let taken = downloads.receive("alice", HERE.into(), file, 3).await;
    assert!(taken.is_ok(), "{file}: {taken:?}");
  }
  assert_eq!(
    sent_jobs(&app).await,
    1,
    "files sent close together are one job"
  );
}

#[tokio::test]
async fn offers_that_come_together_are_held_to_the_files_one_address_may_send() {
  let app = app(&everyone(), &[]).await;
  let downloads = &app.state.downloads;
  // names nobody confirms are the sender's to pick, so each offer comes under another
  let offers = ["bob", "carol", "dave", "erin"].map(|name| async move {
    let file = format!("@@s\\{name}.flac");
    downloads.receive(name, IpAddr::from(HERE), &file, 3).await
  });
  let answers = futures_util::future::join_all(offers).await;
  let taken = answers.iter().filter(|answer| answer.is_ok()).count();
  assert_eq!(taken, 2, "{answers:?}");
}

#[tokio::test]
async fn a_sender_nothing_is_known_of_yet_is_asked_about_before_being_refused() {
  use soulseek_proto::server::{GetPeerAddressResponse, UserStatus};
  use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};

  let config = format!("{TRUSTED}[transfers.download.received]\nfrom = [\"privileged\"]\n");
  let app = crate::testing::soulseek::app(&config).await;
  // alice is privileged when asked; as after a restart, nothing was said of her before
  let server = crate::testing::soulseek::Server::start(|request| match request {
    ServerRequest::GetStatus(asked) => vec![ServerResponse::GetStatus(UserStatus {
      username: asked.username.clone(),
      presence: UserPresence::Online,
      is_privileged: true,
    })],
    ServerRequest::GetPeerAddress(asked) => {
      vec![ServerResponse::GetPeerAddress(GetPeerAddressResponse {
        username: asked.username.clone(),
        ip: std::net::Ipv4Addr::LOCALHOST,
        port: 2234,
        obfuscation: None,
      })]
    }
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let localhost = IpAddr::from(std::net::Ipv4Addr::LOCALHOST);
  let taken = app
    .state
    .downloads
    .receive("alice", localhost, "@@s\\gift.flac", 3)
    .await;
  assert!(taken.is_ok(), "{taken:?}");
}

#[tokio::test]
async fn a_sender_asked_about_and_refused_is_not_put_in_the_directory() {
  use soulseek_proto::server::UserStatus;
  use soulseek_proto::{ServerRequest, ServerResponse, UserPresence};

  let config = format!("{TRUSTED}[transfers.download.received]\nfrom = [\"privileged\"]\n");
  let app = crate::testing::soulseek::app(&config).await;
  // bob, whose name is the sender's to pick, is not privileged
  let server = crate::testing::soulseek::Server::start(|request| match request {
    ServerRequest::GetStatus(asked) => vec![ServerResponse::GetStatus(UserStatus {
      username: asked.username.clone(),
      presence: UserPresence::Online,
      is_privileged: false,
    })],
    _ => Vec::new(),
  })
  .await;
  server.login(&app).await;
  let localhost = IpAddr::from(std::net::Ipv4Addr::LOCALHOST);
  let users = &app.state.users;
  let refused = app
    .state
    .downloads
    .receive("bob", localhost, "@@s\\spam.flac", 3)
    .await;
  assert!(
    matches!(refused, Err(ClientError::TransferRejected(_))),
    "{refused:?}"
  );
  // the writer takes touches in order, so once a row made after them is there, any touch of
  // bob's is written
  users.touch("carol", crate::users::Touch::Asked).await;
  let known = |name: &'static str| async move { users.profile(name).await.unwrap().user.known };
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    while !known("carol").await {
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
  })
  .await
  .expect("the touch writer never wrote");
  assert!(!known("bob").await, "the refused sender has a row");
}

#[tokio::test]
async fn a_sender_no_group_the_server_decides_could_let_in_is_refused_without_asking() {
  let config = format!("{TRUSTED}[transfers.download.received]\nfrom = [\"buddies\"]\n");
  let app = crate::testing::soulseek::app(&config).await;
  // a status asked of this server is never answered
  let server = crate::testing::soulseek::Server::silent().await;
  server.login(&app).await;
  let localhost = IpAddr::from(std::net::Ipv4Addr::LOCALHOST);
  let refused = tokio::time::timeout(
    std::time::Duration::from_secs(2),
    app
      .state
      .downloads
      .receive("bob", localhost, "@@s\\spam.flac", 3),
  )
  .await
  .expect("the server was asked about a sender no answer could let in");
  assert!(
    matches!(refused, Err(ClientError::TransferRejected(_))),
    "{refused:?}"
  );
}

#[test]
fn buddies_behind_one_address_each_send_their_own_files_but_strangers_share_one_count() {
  let senders = Arc::new(Mutex::new(Senders::default()));
  let here = IpAddr::from(HERE);
  let reserve = |everyone, name| Senders::reserve(&senders, counted_as(everyone, name, here), 1);
  let alice = reserve(false, "alice");
  assert!(
    reserve(false, "bob").is_some(),
    "a buddy is counted by name"
  );
  let stranger = reserve(true, "carol");
  assert!(
    reserve(true, "dave").is_none(),
    "when everyone may send, one address is one sender"
  );
  drop((alice, stranger));
}

fn received(from: &[&str]) -> Received {
  Received {
    from: from.iter().map(|g| g.to_string()).collect(),
    max_size: Some(bytesize::ByteSize::mib(10)),
    max_files: 2,
  }
}

/// a buddy, in default like everyone
fn buddy(group: &str) -> bool {
  group == "buddies" || group == DEFAULT_GROUP
}

#[test]
fn a_file_sent_unasked_is_taken_only_from_the_groups_allowed() {
  let take =
    |from: &[&str], in_group: fn(&str) -> bool| refusal(&received(from), in_group, 1).is_none();
  assert!(!take(&[], buddy), "no one may send files by default");
  assert!(take(&["buddies"], buddy));
  assert!(
    take(&[DEFAULT_GROUP], |g| g == DEFAULT_GROUP),
    "default is everyone"
  );
  assert!(!take(&["buddies"], |g| g == DEFAULT_GROUP));
  assert!(
    !take(&["friends"], buddy),
    "a group the sender is not in, or that does not exist, lets no one in"
  );
}

#[test]
fn a_file_sent_unasked_is_held_to_its_size() {
  let options = received(&["buddies"]);
  let mib = 1024 * 1024;
  assert!(refusal(&options, buddy, 10 * mib).is_none());
  assert!(refusal(&options, buddy, 10 * mib + 1).is_some());
  let unlimited = Received {
    max_size: None,
    ..options
  };
  assert!(refusal(&unlimited, buddy, u64::MAX).is_none());
}
