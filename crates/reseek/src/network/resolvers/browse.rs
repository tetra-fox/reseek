//! Our share listing for a peer that browses us, streamed from the browse cache the shares keep
//! so a large listing is neither built nor held in memory per request, and the contents of one
//! shared directory for a peer that asks for a folder. Both are what the peer's user sees of
//! our shares, which depends on whether they are our buddy and ask from the address the server
//! reports for them.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use api_types::activity::ActivityKind;
use api_types::users::blocking::BlockScope;
use reseek_shares::{lock, traverses};
use soulseek_client::{BrowseReply, EncodedFrame, LogLimiter};
use soulseek_proto::RemoteFilename;
use soulseek_proto::peer::{BrowseResponse, Directory};
use tracing::{debug, error, trace, warn};

use super::{Edges, blocked, suspicious};
use crate::shares::View;
use crate::telemetry::millis;

/// Browses that found the cache missing, which repeat while it cannot be written.
static UNCACHED: LogLimiter = LogLimiter::new();
/// Browse cache writes that failed on a browse.
static UNWRITTEN: LogLimiter = LogLimiter::new();
/// Browse cache reads that failed.
static UNREAD: LogLimiter = LogLimiter::new();
/// Folder requests the index could not answer.
static UNLISTED: LogLimiter = LogLimiter::new();

/// A peer browses us: a blocked user sees an empty share, anyone else the shares they see.
pub async fn browse(edges: &Edges, username: &str, endpoint: SocketAddr) -> BrowseReply {
  let metrics = &edges.telemetry.metrics().browse;
  metrics.requests_received.inc();
  if blocked(edges, username, Some(endpoint), BlockScope::Browse) {
    return BrowseReply::Response(empty_listing());
  }
  edges.activity.record(Some(username), ActivityKind::Browse);
  let started = Instant::now();
  let sees = edges.users.sees_from(username, endpoint.ip()).await;
  let reply = listing(edges, &edges.shares.view(&sees)).await;
  let elapsed = millis(started);
  metrics.response_latency.observe(elapsed);
  metrics.response_latency_current.update(elapsed);
  metrics.responses_sent.inc();
  debug!(username, "Sent browse response");
  reply
}

/// Our share listing as `view` sees it, from its browse cache. A cache that is missing, or was
/// read before a relay agent last came or went, is written first; a cache that can be neither
/// read nor written gives an empty listing.
async fn listing(edges: &Edges, view: &View) -> BrowseReply {
  if let Ok(reply) = current(edges, view).await {
    return reply;
  }
  let _rewriting = edges.rebuilding.hold(view).await;
  // a browse that waited for the lock finds the cache the one before it wrote
  match current(edges, view).await {
    Ok(reply) => return reply,
    Err(Unwritten::Missing) => {
      if let Some(suppressed) = UNCACHED.allow() {
        warn!(suppressed, "Browse response not cached. Rebuilding...");
      }
    }
    Err(Unwritten::Behind) => {
      debug!("browse cache of the view not written yet or since an agent came or went; writing it")
    }
  }
  let opened = match edges.shares.rebuild_browse_cache(view).await {
    Ok(opened) => opened,
    Err(e) => {
      if let Some(suppressed) = UNWRITTEN.allow() {
        error!(error = %e, suppressed, "Error caching browse response");
      }
      return BrowseReply::Response(empty_listing());
    }
  };
  match framed(opened).await {
    Ok(frame) => BrowseReply::Encoded(frame),
    Err(e) => unread(&edges.shares.browse_cache(view), &e),
  }
}

/// The views whose browse cache a browse is writing or waits to write, each with its own lock.
#[derive(Default)]
pub(super) struct Rewrites(Mutex<HashMap<View, Rewrite>>);

/// A view's lock and how many browses hold or wait for it, counted under the map's lock.
#[derive(Default)]
struct Rewrite {
  lock: Arc<tokio::sync::Mutex<()>>,
  browses: usize,
}

impl Rewrites {
  /// Waits until no other browse writes the cache of `view`, then holds it until the returned
  /// value is dropped.
  async fn hold(&self, view: &View) -> Rewriting<'_> {
    let held = {
      let mut views = lock(&self.0);
      let rewrite = views.entry(view.clone()).or_default();
      rewrite.browses += 1;
      rewrite.lock.clone()
    };
    // made before the wait, so a browse dropped while it waits still counts itself out
    let mut rewriting = Rewriting {
      rewrites: self,
      view: view.clone(),
      guard: None,
    };
    rewriting.guard = Some(held.lock_owned().await);
    rewriting
  }
}

/// A browse holding or waiting for a view's lock; the view is forgotten once no browse does.
struct Rewriting<'a> {
  rewrites: &'a Rewrites,
  view: View,
  guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Drop for Rewriting<'_> {
  fn drop(&mut self) {
    drop(self.guard.take());
    let mut views = lock(&self.rewrites.0);
    let rewrite = views
      .get_mut(&self.view)
      .expect("a view stays in the map while a browse counted in it lives");
    rewrite.browses -= 1;
    if rewrite.browses == 0 {
      views.remove(&self.view);
    }
  }
}

/// Why a browse cache is to be written before it is sent.
enum Unwritten {
  /// the shares say it is current, but its file is gone
  Missing,
  /// it is not written for this view, or not since an agent came or went
  Behind,
}

/// The browse cache of `view` as the reply to a browse when it is current; a cache that cannot
/// be read gives an empty listing.
async fn current(edges: &Edges, view: &View) -> Result<BrowseReply, Unwritten> {
  let file = edges
    .shares
    .current_browse_cache(view)
    .ok_or(Unwritten::Behind)?;
  match cached(&file).await {
    Ok(frame) => Ok(BrowseReply::Encoded(frame)),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Unwritten::Missing),
    Err(e) => Ok(unread(&file, &e)),
  }
}

/// The browse cache `file` as a frame read from it while it is sent. The file is replaced
/// whole, never written in place, so the one opened here stays complete.
async fn cached(file: &Path) -> std::io::Result<EncodedFrame> {
  framed(tokio::fs::File::open(file).await?).await
}

/// The browse cache `opened` as a frame read from it while it is sent.
async fn framed(opened: tokio::fs::File) -> std::io::Result<EncodedFrame> {
  let len = opened.metadata().await?.len();
  Ok(EncodedFrame {
    len,
    reader: Box::pin(opened),
  })
}

/// The empty listing sent when the browse cache at `file` could not be read for `error`.
fn unread(file: &Path, error: &std::io::Error) -> BrowseReply {
  if let Some(suppressed) = UNREAD.allow() {
    warn!(path = %file.display(), %error, suppressed, "browse cache not read; an empty share sent");
  }
  BrowseReply::Response(empty_listing())
}

fn empty_listing() -> BrowseResponse {
  BrowseResponse {
    directories: Vec::new(),
    locked_directories: Vec::new(),
  }
}

/// A peer asks for the contents of `directory`: a name with a `.` or `..` part gets no
/// directories, a blocked user the directory empty.
pub async fn directory_contents(
  edges: &Edges,
  username: &str,
  endpoint: SocketAddr,
  directory: &RemoteFilename,
) -> Option<Vec<Directory>> {
  if traverses(directory.as_str()) {
    suspicious(username, "folder", directory.as_str());
    return Some(Vec::new());
  }
  if blocked(edges, username, Some(endpoint), BlockScope::Browse) {
    return Some(vec![Directory {
      name: directory.clone(),
      files: Vec::new(),
    }]);
  }
  folder(edges, username, endpoint, directory).await
}

/// The shared directory `directory` and every directory below it, each with its own files; a
/// directory we do not share, or that `username` asking from `endpoint` does not see, comes back
/// empty. `None`, and no answer, when the index cannot be read.
async fn folder(
  edges: &Edges,
  username: &str,
  endpoint: SocketAddr,
  directory: &RemoteFilename,
) -> Option<Vec<Directory>> {
  let sees = edges.users.sees_from(username, endpoint.ip()).await;
  let view = edges.shares.view(&sees);
  match edges
    .shares
    .folder(username, directory.as_str(), &view)
    .await
  {
    Ok(directories) => {
      trace!(username, %directory, directories = directories.len(), "folder contents found");
      Some(directories)
    }
    Err(e) => {
      if let Some(suppressed) = UNLISTED.allow() {
        warn!(username, %directory, error = %e, suppressed, "Failed to resolve directory contents");
      }
      None
    }
  }
}

#[cfg(test)]
mod tests {
  use soulseek_proto::PeerMessage;
  use tokio::io::AsyncReadExt;

  use super::*;
  use crate::network::resolvers::tests::{edges, exposed, from};
  use crate::testing::{TestApp, app};

  /// A daemon sharing one album from `dir`, with `mallory` blocked by name.
  async fn shared(dir: &std::path::Path) -> TestApp {
    let album = dir.join("Music").join("Album");
    std::fs::create_dir_all(album.join("Disc 2")).unwrap();
    std::fs::write(album.join("01.mp3"), "one").unwrap();
    std::fs::write(album.join("Disc 2").join("02.mp3"), "two").unwrap();
    let config = format!(
      "[shares]\ndirectories = [\"{}\"]\n\
       [transfers.groups.blacklisted]\nmembers = [\"mallory\"]\n",
      dir.join("Music").display()
    );
    app(&config, &[]).await
  }

  async fn bytes(reply: BrowseReply) -> Vec<u8> {
    let BrowseReply::Encoded(mut frame) = reply else {
      panic!("not streamed from the cache: {reply:?}");
    };
    let mut bytes = Vec::new();
    frame.reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes.len() as u64, frame.len);
    bytes
  }

  #[tokio::test]
  async fn a_browse_streams_the_cache_and_writes_a_missing_one() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path()).await;
    let edges = edges(&app);
    let cache = app.state.shares.browse_cache(&View::everything());
    let peer = from("198.51.100.7");
    assert_eq!(
      bytes(browse(&edges, "alice", peer).await).await,
      std::fs::read(&cache).unwrap(),
      "the cache as it is"
    );

    std::fs::remove_file(&cache).unwrap();
    let (logs, _capturing) = crate::testing::captured_logs();
    let sent = bytes(browse(&edges, "alice", peer).await).await;
    assert!(warned_missing(&logs), "{:?}", logs.snapshot());
    assert_eq!(sent, std::fs::read(&cache).unwrap(), "written again");
    let PeerMessage::BrowseResponse(listing) = PeerMessage::decode(&sent).unwrap() else {
      panic!("not a browse response");
    };
    let names: Vec<&str> = listing
      .directories
      .iter()
      .map(|d| d.name.as_str())
      .collect();
    assert_eq!(names, ["Music", "Music\\Album", "Music\\Album\\Disc 2"]);

    let empty = browse(&edges, "mallory", peer).await;
    assert!(matches!(empty, BrowseReply::Response(r) if r.directories.is_empty()));
    let metrics = &app.state.telemetry.metrics().browse;
    assert_eq!(metrics.requests_received.get(), 3.0);
    assert_eq!(
      metrics.responses_sent.get(),
      2.0,
      "a blocked user's empty share is not counted"
    );
    assert!(exposed(&app).contains("\nreseek_browse_response_latency_count 2\n"));
  }

  /// The directory names of a browse response.
  fn directories(sent: &[u8]) -> Vec<String> {
    let PeerMessage::BrowseResponse(listing) = PeerMessage::decode(sent).unwrap() else {
      panic!("not a browse response");
    };
    listing
      .directories
      .iter()
      .map(|d| d.name.as_str().to_string())
      .collect()
  }

  /// Whether `logs` hold a warning that a browse cache was missing.
  fn warned_missing(logs: &crate::logging::LogBuffer) -> bool {
    logs.snapshot().iter().any(|record| {
      record.level == api_types::logs::LogLevel::Warning
        && record.message.contains("Browse response not cached")
    })
  }

  #[tokio::test]
  async fn a_browse_right_after_an_agent_loads_lists_its_files_writing_only_its_own_view() {
    let dir = tempfile::tempdir().unwrap();
    for share in ["Music", "Private"] {
      std::fs::create_dir_all(dir.path().join(share).join("Album")).unwrap();
      std::fs::write(dir.path().join(share).join("Album/01.mp3"), "one").unwrap();
    }
    let config = format!(
      "[shares]\ndirectories = [\"{}\", \"{}\"]\n[shares.visible_to]\nPrivate = [\"friends\"]\n",
      dir.path().join("Music").display(),
      dir.path().join("Private").display()
    );
    let app = app(&config, &[]).await;
    let edges = edges(&app);
    let peer = from("198.51.100.7");
    let (logs, _capturing) = crate::testing::captured_logs();
    let before = directories(&bytes(browse(&edges, "alice", peer).await).await);
    assert!(
      !before.iter().any(|d| d.starts_with("Private")),
      "{before:?}"
    );
    assert!(
      !warned_missing(&logs),
      "a view's first browse writes its cache as a matter of course"
    );

    crate::testing::load_agent_host(&app.state.shares, "nas", "Agent").await;
    let after = directories(&bytes(browse(&edges, "alice", peer).await).await);
    assert!(after.contains(&"Agent".to_string()), "{after:?}");
    let everything = app.state.shares.browse_cache(&View::everything());
    let unbrowsed = directories(&std::fs::read(&everything).unwrap());
    assert!(
      !unbrowsed.contains(&"Agent".to_string()),
      "the view nobody browsed is left to the rewrite after the agents settle: {unbrowsed:?}"
    );
    assert!(!warned_missing(&logs));
  }

  /// Whether `future`, polled once, is still waiting.
  fn pending<F: Future>(future: std::pin::Pin<&mut F>) -> bool {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    future.poll(&mut cx).is_pending()
  }

  #[tokio::test]
  async fn a_browse_never_waits_for_another_views_cache_being_written() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path()).await;
    let edges = edges(&app);
    let peer = from("198.51.100.7");
    crate::testing::load_agent_host(&app.state.shares, "nas", "Agent").await;
    let hiding = View::of(
      &reseek_shares::options::Shares {
        visible_to: [("Music".to_string(), vec!["family".to_string()])].into(),
        ..Default::default()
      },
      &std::collections::BTreeSet::new(),
    );
    assert_ne!(hiding, View::everything());
    assert_eq!(
      app.state.shares.current_browse_cache(&View::everything()),
      None,
      "alice's cache is behind the agent"
    );
    // another view's browse is writing its cache
    let writing = edges.rebuilding.hold(&hiding).await;
    let browsed = tokio::time::timeout(
      std::time::Duration::from_secs(10),
      browse(&edges, "alice", peer),
    )
    .await
    .expect("alice's browse waited for another view");
    let listed = directories(&bytes(browsed).await);
    assert!(listed.contains(&"Agent".to_string()), "{listed:?}");
    // a browse of the held view waits, and lets go of it whether it gives up before or after
    // the browse holding the view is done
    let mut first = Box::pin(edges.rebuilding.hold(&hiding));
    assert!(pending(first.as_mut()), "the same view waits");
    drop(first);
    let mut last = Box::pin(edges.rebuilding.hold(&hiding));
    assert!(pending(last.as_mut()));
    drop(writing);
    drop(last);
    assert!(
      lock(&edges.rebuilding.0).is_empty(),
      "a view nobody holds or waits for is forgotten"
    );
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn a_view_is_forgotten_when_two_browses_of_it_end_together() {
    let rewrites = Arc::new(Rewrites::default());
    for round in 0..20_000 {
      let both = [(); 2].map(|()| {
        let rewrites = rewrites.clone();
        tokio::spawn(async move { drop(rewrites.hold(&View::everything()).await) })
      });
      for browse in both {
        browse.await.unwrap();
      }
      assert!(lock(&rewrites.0).is_empty(), "left behind in round {round}");
    }
  }

  #[tokio::test]
  async fn a_folder_request_gets_the_folder_and_what_is_below_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = shared(dir.path()).await;
    let edges = edges(&app);
    let peer = from("198.51.100.7");
    let album = RemoteFilename::from("Music\\Album");
    let folder = directory_contents(&edges, "alice", peer, &album)
      .await
      .unwrap();
    let listed: Vec<(&str, Vec<&str>)> = folder
      .iter()
      .map(|d| {
        (
          d.name.as_str(),
          d.files.iter().map(|f| f.filename.as_str()).collect(),
        )
      })
      .collect();
    assert_eq!(
      listed,
      [
        ("Music\\Album", vec!["01.mp3"]),
        ("Music\\Album\\Disc 2", vec!["02.mp3"])
      ]
    );
    let unknown = RemoteFilename::from("Music\\Elsewhere");
    let folder = directory_contents(&edges, "alice", peer, &unknown)
      .await
      .unwrap();
    assert_eq!(folder.len(), 1);
    assert!(folder[0].files.is_empty());
  }

  #[tokio::test]
  async fn a_buddy_name_on_an_unconfirmed_connection_is_answered_as_everyone() {
    let dir = tempfile::tempdir().unwrap();
    let album = dir.path().join("Music").join("Album");
    std::fs::create_dir_all(&album).unwrap();
    std::fs::write(album.join("01.mp3"), "one").unwrap();
    let config = format!(
      "[shares]\ndirectories = [\"{}\"]\n[shares.visible_to]\nMusic = [\"buddies\"]\n",
      dir.path().join("Music").display()
    );
    let app = app(&config, &[]).await;
    app
      .state
      .users
      .set_buddy("alice", Default::default())
      .await
      .unwrap();
    let edges = edges(&app);
    // logged out, the server tells nothing of alice, so a connection in her name proves nothing
    let peer = from("198.51.100.7");
    let PeerMessage::BrowseResponse(listing) =
      PeerMessage::decode(&bytes(browse(&edges, "alice", peer).await).await).unwrap()
    else {
      panic!("not a browse response");
    };
    assert!(listing.directories.is_empty(), "{:?}", listing.directories);
    let album = RemoteFilename::from("Music\\Album");
    let folder = directory_contents(&edges, "alice", peer, &album)
      .await
      .unwrap();
    assert!(folder.iter().all(|d| d.files.is_empty()), "{folder:?}");
    let file = RemoteFilename::from("Music\\Album\\01.mp3");
    let queued = super::super::enqueue_download(&edges, "alice", peer, &file).await;
    assert!(
      matches!(&queued, Err(soulseek_client::ClientError::DownloadEnqueue(r)) if r == super::super::NOT_SHARED),
      "{queued:?}"
    );
  }
}
