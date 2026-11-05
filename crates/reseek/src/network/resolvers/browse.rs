//! Our share listing for a peer that browses us, streamed from the browse cache the shares keep
//! so a large listing is neither built nor held in memory per request, and the contents of one
//! shared directory for a peer that asks for a folder. Both are what the peer's user sees of
//! our shares, which depends on whether they are our buddy.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

use api_types::activity::ActivityKind;
use api_types::users::blocking::BlockScope;
use soulseek_client::{BrowseReply, EncodedFrame, LogLimiter};
use soulseek_proto::RemoteFilename;
use soulseek_proto::peer::{BrowseResponse, Directory};
use tracing::{debug, error, trace, warn};

use super::{Edges, blocked, suspicious};
use crate::safety::traverses;
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
  let view = edges.shares.view(edges.users.sees(username));
  let reply = listing(edges, &view).await;
  let elapsed = millis(started);
  metrics.response_latency.observe(elapsed);
  metrics.response_latency_current.update(elapsed);
  metrics.responses_sent.inc();
  debug!(username, "Sent browse response");
  reply
}

/// Our share listing as `view` sees it, from its browse cache. A missing cache is written
/// first; a cache that can be neither read nor written gives an empty listing.
async fn listing(edges: &Edges, view: &View) -> BrowseReply {
  let file = edges.shares.browse_cache(view);
  match cached(&file).await {
    Ok(frame) => return BrowseReply::Encoded(frame),
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
    Err(e) => return unread(&file, &e),
  }
  let _rebuilding = edges.rebuilding.lock().await;
  // a browse that waited for the lock finds the cache the one before it wrote
  if let Ok(frame) = cached(&file).await {
    return BrowseReply::Encoded(frame);
  }
  if let Some(suppressed) = UNCACHED.allow() {
    warn!(suppressed, "Browse response not cached. Rebuilding...");
  }
  if let Err(e) = edges.shares.rebuild_browse_cache(view).await {
    if let Some(suppressed) = UNWRITTEN.allow() {
      error!(error = %e, suppressed, "Error caching browse response");
    }
    return BrowseReply::Response(empty_listing());
  }
  match cached(&file).await {
    Ok(frame) => BrowseReply::Encoded(frame),
    Err(e) => unread(&file, &e),
  }
}

/// The browse cache `file` as a frame read from it while it is sent. The file is replaced
/// whole, never written in place, so the one opened here stays complete.
async fn cached(file: &Path) -> std::io::Result<EncodedFrame> {
  let opened = tokio::fs::File::open(file).await?;
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
  folder(edges, username, directory).await
}

/// The shared directory `directory` and every directory below it, each with its own files; a
/// directory we do not share, or that `username` does not see, comes back empty. `None`, and no
/// answer, when the index cannot be read.
async fn folder(
  edges: &Edges,
  username: &str,
  directory: &RemoteFilename,
) -> Option<Vec<Directory>> {
  let view = edges.shares.view(edges.users.sees(username));
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
    let sent = bytes(browse(&edges, "alice", peer).await).await;
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
}
