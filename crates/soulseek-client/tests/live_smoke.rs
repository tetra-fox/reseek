//! The live smoke example against a local soulfind instead of the real server: one user shares
//! files in a room and another offers a smaller file with no free upload slot. The example has to
//! log in, join that room, take both answers, ask the first for the file its pick rule names,
//! receive it and exit 0. The pick rule's edges are tested in the example itself. Run by
//! `just live-smoke-local`, which builds the example first.
mod support;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use soulseek_client::{BrowseReply, ClientOptions, SearchReply, UploadInput, UploadRequest};
use soulseek_proto::RoomType;
use soulseek_proto::peer::{BrowseResponse, Directory, File, SearchResponse};
use support::soulfind::{self, Soulfind, User};
use support::transfers::{self, Asked, content};
use support::{PATIENCE, free_port};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const ROOM: &str = "smoke";
/// the word the shared files are found by
const PROBE: &str = "reseeksmoke";
const DIRECTORY: &str = "@@smoke\\probe";
const MAX_SIZE: u64 = 100_000;
/// the file the example must pick: the smallest non-empty one from a user with a free slot, its
/// name winning the tie with `TIED`
const PICKED: &str = "reseeksmoke a.flac";
const TIED: &str = "reseeksmoke b.flac";
const SMALL: u64 = 40_000;
/// how long the whole example may take; its search alone waits ten seconds for late answers
const RUN: Duration = Duration::from_secs(120);

fn file(name: &str, size: u64) -> File {
  File {
    code: 1,
    filename: name.into(),
    size,
    extension: "flac".into(),
    attributes: Vec::new(),
  }
}

/// a user that answers searches for the probe with `files`, which it shares in one directory
fn answering(options: &mut ClientOptions, username: &str, free_slot: bool, files: Vec<File>) {
  let username = username.to_owned();
  let files: Vec<File> = files
    .into_iter()
    .map(|shared| File {
      filename: format!("{DIRECTORY}\\{}", shared.filename).into(),
      ..shared
    })
    .collect();
  options.resolvers.search_response_resolver = Some(Arc::new(move |_, token, query| {
    let matches = query
      .terms
      .iter()
      .any(|term| term.eq_ignore_ascii_case(PROBE));
    let response = SearchResponse {
      username: username.clone(),
      token,
      files: files.clone(),
      has_free_upload_slot: free_slot,
      upload_speed: 1_000_000,
      queue_length: 0,
      locked_files: Vec::new(),
    };
    Box::pin(async move { Ok(matches.then_some(SearchReply::Response(response))) })
  }));
}

/// The sharer: in the room, with an empty file and two files of one size; it lists them when
/// browsed and queues every upload asked for
async fn sharer(server: &Arc<Soulfind>) -> (User, Asked) {
  let files = vec![
    file("reseeksmoke empty.flac", 0),
    file(TIED, SMALL),
    file(PICKED, SMALL),
  ];
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let username = soulfind::username("sharer");
  answering(&mut options, &username, true, files.clone());
  options.resolvers.browse_response_resolver = Some(Arc::new(move |_, _| {
    let listing = BrowseResponse {
      directories: vec![Directory {
        name: DIRECTORY.into(),
        files: files.clone(),
      }],
      locked_directories: Vec::new(),
    };
    Box::pin(async move { Ok(BrowseReply::Response(listing)) })
  }));
  let asked = transfers::queueing(&mut options);
  let user = server.login_as(username, options).await;
  user
    .client
    .join_room(ROOM, RoomType::Public)
    .await
    .expect("the sharer joins the room");
  (user, asked)
}

/// a user whose smaller file the example must pass over, since it has no free slot
async fn busy(server: &Arc<Soulfind>) -> User {
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let username = soulfind::username("busy");
  answering(
    &mut options,
    &username,
    false,
    vec![file("reseeksmoke busy.flac", SMALL / 2)],
  );
  server.login_as(username, options).await
}

/// where `cargo build --example live_smoke` put the example: beside this test's own directory
fn example() -> PathBuf {
  let tests = std::env::current_exe().expect("the test knows where it runs from");
  let profile = tests
    .parent()
    .and_then(|deps| deps.parent())
    .expect("the test runs from the target directory");
  let example = profile
    .join("examples")
    .join(format!("live_smoke{}", std::env::consts::EXE_SUFFIX));
  assert!(
    example.is_file(),
    "{} is missing: build it with `cargo build -p soulseek-client --example live_smoke`, as \
     `just live-smoke-local` does",
    example.display()
  );
  example
}

#[tokio::test]
#[ignore = "runs the live smoke example; run by just live-smoke-local"]
async fn the_live_smoke_example_passes_against_soulfind() {
  let Some(server) = soulfind::server() else {
    return;
  };
  let (sharer, mut asked) = sharer(&server).await;
  let _busy = busy(&server).await;

  // everything the example writes goes to one file, read back for the failure message
  let log = transfers::scratch().join("example.log");
  let output = std::fs::File::create(&log).unwrap();
  let mut example = Command::new(example())
    .env("RESEEK_TEST_SERVER", server.address().to_string())
    .env("RESEEK_TEST_USERNAME", soulfind::username("smoker"))
    .env("RESEEK_TEST_PASSWORD", soulfind::PASSWORD)
    .env_remove("RUST_LOG")
    .args(["--room", ROOM, "--query", PROBE])
    .args(["--max-size", &MAX_SIZE.to_string()])
    .args(["--listen-port", &free_port().to_string()])
    .stdin(Stdio::null())
    .stdout(output.try_clone().unwrap())
    .stderr(output)
    .kill_on_drop(true)
    .spawn()
    .expect("the example runs");
  let written = || std::fs::read_to_string(&log).unwrap();

  // the example asks for the file it picked, and the sharer uploads it
  let (username, filename) = tokio::select! {
    request = tokio::time::timeout(RUN, asked.recv()) => request
      .expect("the example asks for a file")
      .expect("the sharer is still listening"),
    status = example.wait() => panic!(
      "the example ended with {} before asking for a file:\n{}",
      status.unwrap(),
      written()
    ),
  };
  assert_eq!(filename.as_str(), format!("{DIRECTORY}\\{PICKED}"));
  let source = transfers::scratch().join("picked.flac");
  std::fs::write(&source, content(SMALL as usize)).unwrap();
  let uploaded = tokio::time::timeout(
    PATIENCE * 3,
    sharer.client.upload(
      UploadRequest::new(username, filename, UploadInput::File(source)),
      &CancellationToken::new(),
    ),
  )
  .await
  .expect("the upload ends")
  .expect("the upload succeeds");
  assert_eq!(uploaded.bytes_transferred, SMALL);

  let status = tokio::time::timeout(RUN, example.wait())
    .await
    .expect("the example exits")
    .unwrap();
  let written = written();
  assert!(
    status.success(),
    "the example ended with {status}:\n{written}"
  );
  // the busy user's answer was there to pass over; this reads the client's "search completed"
  // log line, so renaming that line or its field means changing this
  assert!(
    written.contains("search completed") && written.contains("responses=2"),
    "{written}"
  );
}
