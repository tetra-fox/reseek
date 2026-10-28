//! The daemon's smoke run: everything through its API, as a user of it would. On a test server,
//! with the soulseek-rs CLI as the other user, the daemon shares a file, searches the other
//! user's share, browses it, downloads from it and uploads to it. On the real network it
//! searches, browses a user who answered and downloads one small file, moving on to the next
//! file in pick order when a user cannot be reached.
//!
//! The end-to-end test, the real-network test and the smoke program the NixOS VM test runs all
//! call into this file, so the three check the same steps.

use std::path::Path;
use std::time::{Duration, Instant};

use api_client::{Client, LocalFile};
use api_types::files::ContentQuery;
use api_types::search::{SearchRequest, SearchResponse};
use api_types::state::State;
use api_types::transfers::{QueueDownloadRequest, Transfer, TransferDirection, TransferStates};
use api_types::users::UserPresence;
use tracing::info;

use super::soulseek_rs::Peer;

/// The word both sides' shared files are found by.
pub const PROBE: &str = "reseekprobe";
/// The file the daemon shares, under its share directory.
const OURS: &str = "reseekprobe ours.flac";
/// The file the other user shares, under its share directory.
const THEIRS: &str = "reseekprobe theirs.flac";
const OUR_SIZE: usize = 150_000;
const THEIR_SIZE: usize = 200_000;

/// How long a step on the test server may take; a VM is slower than the machine.
const STEP: Duration = Duration::from_secs(60);
/// How long a search on the real network collects responses.
const LIVE_SEARCH: Duration = Duration::from_secs(30);
/// How long one download on the real network may take before the next file is tried.
const LIVE_DOWNLOAD: Duration = Duration::from_secs(180);
/// How many files of the pick order are tried before the run fails.
const LIVE_TRIES: usize = 8;

/// The value `check` gives once it gives one, asked every 200 ms; fails the run naming `what`
/// with the last thing seen when `limit` passes first.
async fn eventually<T, F, Fut>(what: &str, limit: Duration, mut check: F) -> T
where
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, String>>,
{
  let started = Instant::now();
  loop {
    match check().await {
      Ok(value) => return value,
      Err(seen) if started.elapsed() > limit => panic!("never {what}: {seen}"),
      Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
    }
  }
}

/// Bytes that differ from one offset to the next, so a shifted or truncated copy shows.
fn bytes(len: usize, seed: u8) -> Vec<u8> {
  (0..len)
    .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
    .collect()
}

/// Writes `len` bytes at `path`, making its directory.
fn write(path: &Path, len: usize, seed: u8) -> Vec<u8> {
  let content = bytes(len, seed);
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, &content).unwrap();
  content
}

/// The daemon's state once it is logged in to its server.
pub async fn logged_in(api: &Client, limit: Duration) -> State {
  eventually("logged in", limit, || async {
    let state = api.application().await.map_err(|e| e.to_string())?;
    if state.server.is_logged_in {
      Ok(state)
    } else {
      Err(format!("{:?}", state.server.state))
    }
  })
  .await
}

/// Starts a search for `text` and returns its responses once it completed.
async fn search(api: &Client, text: &str, timeout: Duration) -> Vec<SearchResponse> {
  let request: SearchRequest = serde_json::from_value(serde_json::json!({
    "searchText": text,
    "searchTimeout": timeout.as_millis() as u64,
  }))
  .unwrap();
  let search = api.start_search(&request).await.unwrap();
  eventually("completed the search", timeout + STEP, || async {
    let now = api
      .search(search.id, false)
      .await
      .map_err(|e| e.to_string())?;
    if now.is_complete {
      Ok(())
    } else {
      Err(format!("{now:?}"))
    }
  })
  .await;
  api.search_responses(search.id).await.unwrap()
}

/// The download `id` from `username` once it ended, or the last state seen when `limit` passes.
async fn ended(
  api: &Client,
  username: &str,
  id: uuid::Uuid,
  limit: Duration,
) -> Result<Transfer, String> {
  let started = Instant::now();
  loop {
    let transfer = api
      .transfer(TransferDirection::Download, username, id, false)
      .await
      .map_err(|e| e.to_string())?;
    if transfer.state.contains(TransferStates::Completed) {
      return Ok(transfer);
    }
    if started.elapsed() > limit {
      return Err(format!("still {} after {limit:?}", transfer.state));
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
  }
}

/// Enqueues the download of `filename` from `username`.
async fn enqueue(api: &Client, username: &str, filename: &str, size: u64) -> uuid::Uuid {
  let files = [QueueDownloadRequest {
    filename: filename.to_string(),
    size,
  }];
  let answer = api.enqueue(username, &files).await.unwrap();
  assert!(
    answer.failed.is_empty(),
    "not enqueued: {:?}",
    answer.failed
  );
  answer.enqueued[0].id
}

/// The smoke run on a test server: the daemon `api` talks to, logged in as `ours` and sharing
/// `share`, against `peer`, another user of the same server. Fails the run at the first step
/// that goes wrong.
pub async fn local(api: &Client, ours: &str, share: &Path, peer: &Peer) {
  let state = logged_in(api, STEP).await;
  info!(username = ?state.user.username, "the daemon is logged in");

  let our_file = share.join("probe").join(OURS);
  let our_content = write(&our_file, OUR_SIZE, 1);
  api.scan_shares(true).await.unwrap();
  eventually("shared the probe file", STEP, || async {
    let contents = api.share_contents().await.map_err(|e| e.to_string())?;
    let names: Vec<&str> = contents
      .iter()
      .flat_map(|directory| &directory.files)
      .map(|file| file.filename.as_str())
      .collect();
    if names.iter().any(|name| name.ends_with(OURS)) {
      Ok(())
    } else {
      Err(format!("{names:?}"))
    }
  })
  .await;
  info!("the daemon shares the probe file");

  let their_content = write(&peer.share().join("probe").join(THEIRS), THEIR_SIZE, 2);
  std::fs::create_dir_all(peer.downloads()).unwrap();
  let serving = peer.serve();
  eventually("saw the other user online", STEP, || async {
    let status = api
      .user_status(&peer.username)
      .await
      .map_err(|e| e.to_string())?;
    if status.presence == UserPresence::Online {
      Ok(())
    } else {
      Err(format!("{:?}", status.presence))
    }
  })
  .await;

  let theirs = eventually("found the other user's file", STEP * 2, || async {
    let responses = search(api, PROBE, Duration::from_secs(5)).await;
    responses
      .into_iter()
      .find(|response| response.username == peer.username)
      .and_then(|response| response.files.into_iter().next())
      .ok_or_else(|| "no answer from the other user yet".to_string())
  })
  .await;
  assert!(theirs.filename.ends_with(THEIRS), "{}", theirs.filename);
  assert_eq!(theirs.size, THEIR_SIZE as u64);
  info!(
    filename = theirs.filename,
    "the search found the other user's file"
  );

  let browsed = api.browse(&peer.username).await.unwrap();
  let listed: Vec<&str> = browsed
    .directories
    .iter()
    .flat_map(|directory| &directory.files)
    .map(|file| file.filename.as_str())
    .collect();
  assert!(
    listed.iter().any(|name| name.ends_with(THEIRS)),
    "{listed:?}"
  );
  info!(
    files = listed.len(),
    "the browse listed the other user's share"
  );

  let id = enqueue(api, &peer.username, &theirs.filename, theirs.size).await;
  let done = ended(api, &peer.username, id, STEP * 2).await.unwrap();
  assert_eq!(
    done.state,
    TransferStates::Completed | TransferStates::Succeeded,
    "{done:?}"
  );
  let fetched = api
    .file_content(
      LocalFile::Download(&peer.username, id),
      &ContentQuery::default(),
    )
    .await
    .unwrap();
  assert!(fetched == their_content, "the downloaded file differs");
  info!("the download from the other user landed whole");
  drop(serving);

  let found = peer.run(&["search", PROBE, "--search-timeout", "5"]).await;
  let found = String::from_utf8_lossy(&found.stdout);
  let line = found
    .lines()
    .find(|line| line.split('\t').next() == Some(ours))
    .unwrap_or_else(|| panic!("the daemon's answer is among the results:\n{found}"));
  let filename = line.rsplit('\t').next().unwrap().to_string();
  assert!(filename.ends_with(OURS), "{line}");
  info!(filename, "the other user found the daemon's file");

  let size = OUR_SIZE.to_string();
  let mut fetch = peer.command(&[
    "download",
    ours,
    &filename,
    "--size",
    &size,
    "--timeout",
    "60",
  ]);
  let fetched = tokio::task::spawn_blocking(move || fetch.output())
    .await
    .unwrap()
    .expect("soulseek-rs runs");
  assert!(
    fetched.status.success(),
    "the other user's download failed: {}",
    String::from_utf8_lossy(&fetched.stderr)
  );
  let landed = String::from_utf8_lossy(&fetched.stdout).trim().to_string();
  assert!(
    std::fs::read(&landed).unwrap() == our_content,
    "the uploaded file differs"
  );
  eventually("recorded the upload as succeeded", STEP, || async {
    let users = api
      .uploads(&Default::default())
      .await
      .map_err(|e| e.to_string())?;
    let upload = users
      .iter()
      .filter(|user| user.username == peer.username)
      .flat_map(|user| &user.directories)
      .flat_map(|directory| &directory.files)
      .find(|transfer| transfer.filename == filename);
    match upload {
      Some(upload) if upload.state == TransferStates::Completed | TransferStates::Succeeded => {
        Ok(())
      }
      other => Err(format!("{other:?}")),
    }
  })
  .await;
  info!("the upload to the other user finished");
}

/// A file a search response offered, in the order the live run tries them.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
  /// the user who offered it
  pub username: String,
  /// its remote path
  pub filename: String,
  /// its size in bytes
  pub size: u64,
}

/// The non-empty files of at most `max_size` bytes from users with a free upload slot,
/// smallest first, the file name and then the username breaking ties. An empty file would show
/// nothing about bytes landing.
pub fn pick_order(responses: &[SearchResponse], max_size: u64) -> Vec<Candidate> {
  let mut candidates: Vec<Candidate> = responses
    .iter()
    .filter(|response| response.has_free_upload_slot)
    .flat_map(|response| {
      response.files.iter().map(|file| Candidate {
        username: response.username.clone(),
        filename: file.filename.clone(),
        size: file.size,
      })
    })
    .filter(|candidate| candidate.size > 0 && candidate.size <= max_size)
    .collect();
  candidates.sort_by(|a, b| {
    a.size
      .cmp(&b.size)
      .then_with(|| a.filename.cmp(&b.filename))
      .then_with(|| a.username.cmp(&b.username))
  });
  candidates
}

/// The smoke run on the real network: the daemon `api` talks to searches for `query`, then
/// browses and downloads files in pick order until one lands whole. A user that cannot be
/// browsed, or whose download fails or does not finish in time, is passed over with the reason
/// logged; the run fails once `LIVE_TRIES` files were tried.
pub async fn live(api: &Client, query: &str, max_size: u64) {
  let state = logged_in(api, STEP).await;
  info!(username = ?state.user.username, "the daemon is logged in");
  let responses = search(api, query, LIVE_SEARCH).await;
  let candidates = pick_order(&responses, max_size);
  info!(
    responses = responses.len(),
    candidates = candidates.len(),
    "searched"
  );
  assert!(
    !candidates.is_empty(),
    "none of the {} responses offered a non-empty file of at most {max_size} bytes from a user \
     with a free upload slot",
    responses.len()
  );
  let mut passed_over: Vec<String> = Vec::new();
  let mut unreachable: Vec<String> = Vec::new();
  for candidate in candidates.iter().take(LIVE_TRIES) {
    if unreachable.contains(&candidate.username) {
      continue;
    }
    info!(
      username = candidate.username,
      filename = candidate.filename,
      size = candidate.size,
      "trying"
    );
    match tokio::time::timeout(STEP * 2, api.browse(&candidate.username)).await {
      Ok(Ok(browsed)) => info!(
        username = candidate.username,
        directories = browsed.directories.len(),
        "browsed"
      ),
      outcome => {
        let reason = match outcome {
          Ok(Err(e)) => e.to_string(),
          _ => "the browse did not finish".to_string(),
        };
        info!(
          username = candidate.username,
          reason, "passed over: not browsed"
        );
        passed_over.push(format!("{}: browse: {reason}", candidate.username));
        unreachable.push(candidate.username.clone());
        continue;
      }
    }
    let id = enqueue(
      api,
      &candidate.username,
      &candidate.filename,
      candidate.size,
    )
    .await;
    let outcome = ended(api, &candidate.username, id, LIVE_DOWNLOAD).await;
    match outcome {
      Ok(done) if done.state == TransferStates::Completed | TransferStates::Succeeded => {
        let fetched = api
          .file_content(
            LocalFile::Download(&candidate.username, id),
            &ContentQuery::default(),
          )
          .await
          .unwrap();
        assert_eq!(
          fetched.len() as u64,
          candidate.size,
          "the file holds what was announced"
        );
        info!(bytes = fetched.len(), "downloaded");
        return;
      }
      outcome => {
        let reason = match outcome {
          Ok(done) => format!("{}: {}", done.state, done.exception.unwrap_or_default()),
          Err(reason) => reason,
        };
        info!(
          username = candidate.username,
          reason, "passed over: not downloaded"
        );
        passed_over.push(format!("{}: download: {reason}", candidate.username));
        // a download still running would hold the slot of the next try
        if let Err(e) = api
          .cancel_transfer(TransferDirection::Download, &candidate.username, id, true)
          .await
        {
          info!(error = %e, "the download was not cancelled");
        }
      }
    }
  }
  panic!(
    "no file downloaded; passed over:\n{}",
    passed_over.join("\n")
  );
}
