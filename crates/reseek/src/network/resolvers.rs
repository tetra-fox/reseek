//! How we answer what other users ask of us: an upload queued, our share listing, a folder's
//! contents, a search, our user info and a file's place in our upload queue. Each answer passes
//! the checks at the network's edge first: a name with a `.` or `..` part is refused, and a user
//! blocked from what they ask for gets what a blocked user gets. What is shared and the upload
//! queue then decide the answer; until they are wired, each answers that nothing is here.

use std::net::SocketAddr;
use std::sync::Arc;

use api_types::blocking::BlockScope;
use soulseek_client::{BrowseReply, ClientError, LogLimiter, Resolvers, SearchReply};
use soulseek_proto::peer::{BrowseResponse, Directory, UserInfo};
use soulseek_proto::{RemoteFilename, SearchQuery};
use tracing::{debug, info, trace, warn};

use crate::safety::traverses;
use crate::shares::Shares;
use crate::users::Directory as Users;

/// The refusal a peer gets for a file we do not share to them.
pub const NOT_SHARED: &str = "File not shared.";

/// Requests with a `.` or `..` part, which a peer could send at any rate.
static TRAVERSING: LogLimiter = LogLimiter::new();
/// Requests from blocked users, likewise.
static BLOCKED: LogLimiter = LogLimiter::new();

/// What the answers are made from.
pub struct Edges {
  /// who is blocked from what
  pub users: Arc<Users>,
  /// what we share
  pub shares: Arc<Shares>,
}

/// The resolvers the client answers peers with.
pub fn resolvers(edges: Arc<Edges>) -> Resolvers {
  let enqueue = edges.clone();
  let browsing = edges.clone();
  let folder = edges.clone();
  let searching = edges.clone();
  let info = edges.clone();
  let place = edges;
  Resolvers {
    enqueue_download: Some(Arc::new(move |username, endpoint, filename| {
      let edges = enqueue.clone();
      Box::pin(async move { enqueue_download(&edges, &username, endpoint, &filename).await })
    })),
    browse_response_resolver: Some(Arc::new(move |username, endpoint| {
      let edges = browsing.clone();
      Box::pin(async move { Ok(browse(&edges, &username, endpoint)) })
    })),
    directory_contents_resolver: Some(Arc::new(move |username, endpoint, _token, directory| {
      let edges = folder.clone();
      Box::pin(async move { Ok(directory_contents(&edges, &username, endpoint, &directory)) })
    })),
    search_response_resolver: Some(Arc::new(move |username, token, query| {
      let edges = searching.clone();
      Box::pin(async move { Ok(search(&edges, &username, token, &query)) })
    })),
    user_info_resolver: Some(Arc::new(move |username, endpoint| {
      let edges = info.clone();
      Box::pin(async move { Ok(user_info(&edges, &username, endpoint).await) })
    })),
    place_in_queue_resolver: Some(Arc::new(move |username, endpoint, filename| {
      let edges = place.clone();
      Box::pin(async move { Ok(place_in_queue(&edges, &username, endpoint, &filename)) })
    })),
  }
}

/// Logs, at most once a minute with a count, a request with a `.` or `..` part.
fn suspicious(username: &str, what: &str, name: &str) {
  if let Some(suppressed) = TRAVERSING.allow() {
    warn!(
      username,
      what, name, suppressed, "Suspicious request from a user containing unsafe path segments"
    );
  }
}

/// Whether `username` from `endpoint` is blocked from `scope`, logged at most once a minute
/// with a count when they are.
fn blocked(edges: &Edges, username: &str, endpoint: Option<SocketAddr>, scope: BlockScope) -> bool {
  let blocked = edges
    .users
    .is_blocked(username, endpoint.map(|e| e.ip()), scope);
  if blocked && let Some(suppressed) = BLOCKED.allow() {
    info!(username, %scope, suppressed, "request from a blocked user refused");
  }
  blocked
}

/// A peer asks us to queue an upload of `filename`.
pub async fn enqueue_download(
  edges: &Edges,
  username: &str,
  endpoint: SocketAddr,
  filename: &RemoteFilename,
) -> Result<(), ClientError> {
  if traverses(filename.as_str()) {
    suspicious(username, "enqueue", filename.as_str());
    return Err(ClientError::DownloadEnqueue(NOT_SHARED.into()));
  }
  if blocked(edges, username, Some(endpoint), BlockScope::Uploads) {
    return Err(ClientError::DownloadEnqueue(NOT_SHARED.into()));
  }
  queue_upload(username, filename)
}

/// Puts an upload a peer asked for in our queue. Until the upload queue is wired nothing is
/// queued and the peer is told the file is not shared.
fn queue_upload(username: &str, filename: &RemoteFilename) -> Result<(), ClientError> {
  debug!(username, %filename, "upload refused; nothing is queued for upload yet");
  Err(ClientError::DownloadEnqueue(NOT_SHARED.into()))
}

/// A peer browses us: a blocked user sees an empty share.
pub fn browse(edges: &Edges, username: &str, endpoint: SocketAddr) -> BrowseReply {
  if blocked(edges, username, Some(endpoint), BlockScope::Browse) {
    return BrowseReply::Response(empty_listing());
  }
  listing(username)
}

/// Our share listing. Until the share index answers browses it is empty.
fn listing(username: &str) -> BrowseReply {
  debug!(
    username,
    "browse answered with an empty share; browses are not answered yet"
  );
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
pub fn directory_contents(
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
  folder(username, directory)
}

/// The contents of a shared directory. Until the share index answers folder requests none is
/// sent.
fn folder(username: &str, directory: &RemoteFilename) -> Option<Vec<Directory>> {
  debug!(username, %directory, "folder request not answered; folders are not answered yet");
  None
}

/// Another user's search reached us: a user blocked from searches gets nothing.
pub fn search(
  edges: &Edges,
  username: &str,
  token: u32,
  query: &SearchQuery,
) -> Option<SearchReply> {
  if blocked(edges, username, None, BlockScope::Searches) {
    return None;
  }
  results(username, token, query)
}

/// Our response to a search. Until the share index answers searches there are no results.
fn results(username: &str, token: u32, query: &SearchQuery) -> Option<SearchReply> {
  trace!(
    username,
    token,
    query = query.search_text(),
    "search not answered; searches are not answered yet"
  );
  None
}

/// A peer asks for our user info: a blocked user is shown no free slot and the longest queue.
pub async fn user_info(edges: &Edges, username: &str, endpoint: SocketAddr) -> UserInfo {
  if blocked(edges, username, Some(endpoint), BlockScope::Uploads) {
    let local = edges.shares.user_info().await;
    return UserInfo {
      description: local.description,
      picture: local.picture,
      upload_slots: 0,
      queue_length: i32::MAX as u32,
      has_free_upload_slot: false,
      upload_permitted: None,
    };
  }
  profile(username)
}

/// Our user info with our upload slots and this user's place in the queue. Until the share and
/// the upload queue answer it, it is the empty info.
fn profile(username: &str) -> UserInfo {
  debug!(
    username,
    "user info answered empty; user info is not answered yet"
  );
  UserInfo {
    description: String::new(),
    picture: None,
    upload_slots: 0,
    queue_length: 0,
    has_free_upload_slot: false,
    upload_permitted: None,
  }
}

/// A peer asks for its place in our upload queue: a blocked user is not answered.
pub fn place_in_queue(
  edges: &Edges,
  username: &str,
  endpoint: SocketAddr,
  filename: &RemoteFilename,
) -> Option<u32> {
  if blocked(edges, username, Some(endpoint), BlockScope::Uploads) {
    return None;
  }
  queue_place(username, filename)
}

/// The place of a queued upload. Until the upload queue is wired nothing is queued, so there is
/// no place to tell.
fn queue_place(username: &str, filename: &RemoteFilename) -> Option<u32> {
  debug!(username, %filename, "place in queue not answered; nothing is queued for upload yet");
  None
}
