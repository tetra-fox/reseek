//! The client against another implementation: soulfind as the server and the soulseek-rs CLI as
//! the other user. We search it, browse it and download from it while it serves a share; then it
//! searches us and downloads from us, which is an upload of ours. Run by `just e2e`, which finds
//! both programs and hands the CLI over in `SOULSEEK_RS_BIN`.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use soulseek_client::{
  ClientOptions, DownloadOutput, DownloadRequest, SearchOptions, SearchReply, SearchScope,
  UploadInput, UploadRequest,
};
use soulseek_proto::peer::{File, SearchResponse};
use soulseek_proto::server::{GetStatusRequest, UserStatus};
use soulseek_proto::{ServerCode, ServerRequest, UserPresence};
use support::soulfind::{self, Soulfind, User};
use support::soulseek_rs::Peer;
use support::transfers::{self, content, scratch};
use support::{PATIENCE, free_port};
use tokio_util::sync::CancellationToken;

/// the word both sides' shared files are found by
const PROBE: &str = "reseekprobe";
const THEIR_SIZE: usize = 200_000;
const OUR_SIZE: usize = 150_000;
const OUR_FILE: &str = "@@reseek\\interop\\reseekprobe ours.flac";
const PASSWORD: &str = "interop-password";

/// our client: listening, answering searches for the probe with one file, and queueing every
/// upload the other user asks for
async fn ours(server: &Arc<Soulfind>) -> (User, transfers::Asked) {
  let mut options = ClientOptions {
    listen_port: free_port(),
    ..ClientOptions::default()
  };
  let username = soulfind::username("reseek");
  let answer_as = username.clone();
  options.resolvers.search_response_resolver = Some(Arc::new(move |_, token, query| {
    let matches = query
      .terms
      .iter()
      .any(|term| term.eq_ignore_ascii_case(PROBE));
    let response = SearchResponse {
      username: answer_as.clone(),
      token,
      files: vec![File {
        code: 1,
        filename: OUR_FILE.into(),
        size: OUR_SIZE as u64,
        extension: "flac".into(),
        attributes: Vec::new(),
      }],
      has_free_upload_slot: true,
      upload_speed: 1_000_000,
      queue_length: 0,
      locked_files: Vec::new(),
    };
    Box::pin(async move { Ok(matches.then_some(SearchReply::Response(response))) })
  }));
  let asked = transfers::queueing(&mut options);
  (server.login_as(username, options).await, asked)
}

/// waits until `username` is online, as the server reports it
async fn online(user: &User, username: &str) {
  tokio::time::timeout(PATIENCE * 3, async {
    loop {
      let status: UserStatus = user
        .ask(
          soulseek_client::WaitKey::ServerUser {
            code: ServerCode::GetStatus,
            username: username.to_owned(),
          },
          ServerRequest::GetStatus(GetStatusRequest {
            username: username.to_owned(),
          }),
        )
        .await;
      if status.presence == UserPresence::Online {
        return;
      }
      tokio::time::sleep(Duration::from_millis(200)).await;
    }
  })
  .await
  .expect("the other user comes online");
}

/// searches for the probe until the other user's share answers
async fn their_file(user: &User, username: &str) -> File {
  tokio::time::timeout(PATIENCE * 6, async {
    loop {
      let options = SearchOptions {
        search_timeout: Duration::from_secs(3),
        ..SearchOptions::default()
      };
      let (_, responses) = user
        .client
        .search(
          PROBE,
          SearchScope::Network,
          None,
          options,
          &CancellationToken::new(),
        )
        .await
        .unwrap();
      if let Some(response) = responses
        .into_iter()
        .find(|response| response.username == username)
      {
        return response
          .files
          .into_iter()
          .next()
          .expect("the answer has the file");
      }
    }
  })
  .await
  .expect("the other user's share answers the search")
}

fn write(path: &Path, len: usize) {
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, content(len)).unwrap();
}

#[tokio::test]
#[ignore = "drives the soulseek-rs CLI; run by just e2e"]
async fn search_browse_and_transfers_both_ways_with_soulseek_rs() {
  let binary = std::env::var_os("SOULSEEK_RS_BIN")
    .expect("SOULSEEK_RS_BIN names the soulseek-rs CLI; run this through just e2e");
  let server = soulfind::server().expect("just e2e checks soulfind is there");
  let peer = Peer {
    binary: binary.into(),
    server: server.address(),
    username: soulfind::username("rsuser"),
    password: PASSWORD.to_string(),
    listen_port: free_port(),
    root: scratch(),
  };
  write(
    &peer.share().join("probe").join("reseekprobe theirs.flac"),
    THEIR_SIZE,
  );
  std::fs::create_dir_all(peer.downloads()).unwrap();
  let (ours, mut asked) = ours(&server).await;

  // the other user serves its share: we search it, browse it and download from it
  let serving = peer.serve();
  online(&ours, &peer.username).await;
  let theirs = their_file(&ours, &peer.username).await;
  assert_eq!(theirs.size, THEIR_SIZE as u64);
  assert!(
    theirs
      .filename
      .as_str()
      .ends_with("reseekprobe theirs.flac")
  );
  let listing = ours
    .client
    .browse(
      &peer.username,
      Default::default(),
      &CancellationToken::new(),
    )
    .await
    .unwrap();
  let browsed: Vec<_> = listing
    .directories
    .iter()
    .flat_map(|directory| &directory.files)
    .map(|file| file.filename.as_str().to_owned())
    .collect();
  assert!(
    browsed
      .iter()
      .any(|name| name.ends_with("reseekprobe theirs.flac")),
    "{browsed:?}"
  );
  let destination = scratch().join("theirs.flac");
  let downloaded = tokio::time::timeout(
    PATIENCE * 6,
    ours.client.download(
      DownloadRequest::new(
        &peer.username,
        theirs.filename.clone(),
        theirs.size,
        DownloadOutput::File(destination.clone()),
      ),
      &CancellationToken::new(),
    ),
  )
  .await
  .expect("the download ends")
  .unwrap();
  assert_eq!(downloaded.bytes_transferred, THEIR_SIZE as u64);
  assert_eq!(std::fs::read(&destination).unwrap(), content(THEIR_SIZE));
  drop(serving);

  // the other user searches us
  let found = peer.run(&["search", PROBE, "--search-timeout", "5"]).await;
  let found = String::from_utf8_lossy(&found.stdout);
  let ours_found = found
    .lines()
    .find(|line| line.split('\t').next() == Some(ours.username.as_str()))
    .unwrap_or_else(|| panic!("our answer is among the results:\n{found}"));
  assert!(ours_found.ends_with(OUR_FILE), "{ours_found}");

  // and downloads from us, which we upload
  let source = scratch().join("ours.flac");
  write(&source, OUR_SIZE);
  let size = OUR_SIZE.to_string();
  let username = ours.username.clone();
  let fetch = tokio::spawn({
    let command = peer.command(&[
      "download",
      &username,
      OUR_FILE,
      "--size",
      &size,
      "--timeout",
      "60",
    ]);
    async move {
      let mut command = command;
      tokio::task::spawn_blocking(move || command.output())
        .await
        .unwrap()
        .expect("soulseek-rs runs")
    }
  });
  let (asker, filename) = transfers::asked(&mut asked).await;
  assert_eq!(
    (asker.as_str(), filename.as_str()),
    (peer.username.as_str(), OUR_FILE)
  );
  let uploaded = ours
    .client
    .upload(
      UploadRequest::new(asker, filename, UploadInput::File(source)),
      &CancellationToken::new(),
    )
    .await
    .unwrap();
  assert_eq!(uploaded.bytes_transferred, OUR_SIZE as u64);
  let fetched = tokio::time::timeout(PATIENCE * 6, fetch)
    .await
    .expect("the other user's download ends")
    .unwrap();
  assert!(
    fetched.status.success(),
    "soulseek-rs download failed: {}",
    String::from_utf8_lossy(&fetched.stderr)
  );
  let local = String::from_utf8_lossy(&fetched.stdout).trim().to_owned();
  assert_eq!(std::fs::read(&local).unwrap(), content(OUR_SIZE));
}
