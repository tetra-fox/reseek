//! The callbacks the application supplies so the client can answer peers (share listings, folder
//! contents, user info, search results, upload queueing) and the caches its lookups go through.
//! They are part of the client options; the peer and search handlers call them.

use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use soulseek_proto::peer::{BrowseResponse, Directory, SearchResponse, UserInfo};
use soulseek_proto::{RemoteFilename, SearchQuery};

use crate::error::ClientError;

/// What a resolver returns: a boxed future of its answer
pub type ResolverFuture<T> = Pin<Box<dyn Future<Output = Result<T, ClientError>> + Send>>;

/// `(username, token, query)` to the response for a search a user sent us, or `None` when we
/// have nothing that matches
pub type SearchResponseResolver =
  Arc<dyn Fn(String, u32, SearchQuery) -> ResolverFuture<Option<SearchResponse>> + Send + Sync>;

/// `(username, endpoint)` to our share listing, for a peer that browses us
pub type BrowseResponseResolver =
  Arc<dyn Fn(String, SocketAddr) -> ResolverFuture<BrowseResponse> + Send + Sync>;

/// `(username, endpoint, token, directory)` to that directory and every directory below it, or
/// `None` to send no reply
pub type DirectoryContentsResolver = Arc<
  dyn Fn(String, SocketAddr, u32, RemoteFilename) -> ResolverFuture<Option<Vec<Directory>>>
    + Send
    + Sync,
>;

/// `(username, endpoint)` to the user info we show that peer
pub type UserInfoResolver =
  Arc<dyn Fn(String, SocketAddr) -> ResolverFuture<UserInfo> + Send + Sync>;

/// `(username, endpoint, filename)`: queues an upload a peer asked for. Failing with
/// [`ClientError::DownloadEnqueue`] refuses it with a reason the peer sees; any other error is
/// logged and the peer gets a generic refusal.
pub type EnqueueDownload =
  Arc<dyn Fn(String, SocketAddr, RemoteFilename) -> ResolverFuture<()> + Send + Sync>;

/// `(username, endpoint, filename)` to the 1-based place of that file in our upload queue, or
/// `None` to send no reply
pub type PlaceInQueueResolver =
  Arc<dyn Fn(String, SocketAddr, RemoteFilename) -> ResolverFuture<Option<u32>> + Send + Sync>;

/// Remembers the addresses the server returned for users, so repeated connects skip the lookup
pub trait UserEndPointCache: Send + Sync {
  /// the remembered address of `username`
  fn get(&self, username: &str) -> Result<Option<SocketAddr>, ClientError>;

  /// remembers the address the server just returned for `username`
  fn insert(&self, username: String, endpoint: SocketAddr) -> Result<(), ClientError>;
}

/// A search response we could not deliver because the searcher was unreachable, kept so it can
/// still be sent if the searcher connects to us later
#[derive(Clone, Debug)]
pub struct CachedSearchResponse {
  pub username: String,
  /// the searcher's token
  pub token: u32,
  /// the query as the searcher sent it
  pub query: String,
  pub response: SearchResponse,
}

/// Holds undelivered search responses under the token of the connection request we sent the
/// searcher
pub trait SearchResponseCache: Send + Sync {
  /// keeps `response` under `response_token`
  fn insert(&self, response_token: u32, response: CachedSearchResponse) -> Result<(), ClientError>;

  /// the response kept under `response_token`, left in place
  fn get(&self, response_token: u32) -> Result<Option<CachedSearchResponse>, ClientError>;

  /// takes the response kept under `response_token` out of the cache
  fn remove(&self, response_token: u32) -> Result<Option<CachedSearchResponse>, ClientError>;
}

/// Every callback and cache the application can supply. An unset resolver falls back to the
/// behaviour each one documents.
#[derive(Clone, Default)]
pub struct Resolvers {
  pub user_end_point_cache: Option<Arc<dyn UserEndPointCache>>,
  /// unset: searches are never answered
  pub search_response_resolver: Option<SearchResponseResolver>,
  /// unset: undeliverable search responses are dropped
  pub search_response_cache: Option<Arc<dyn SearchResponseCache>>,
  /// unset: an empty share listing
  pub browse_response_resolver: Option<BrowseResponseResolver>,
  /// unset: folder requests get no reply
  pub directory_contents_resolver: Option<DirectoryContentsResolver>,
  /// unset: empty user info
  pub user_info_resolver: Option<UserInfoResolver>,
  /// unset: every request is queued
  pub enqueue_download: Option<EnqueueDownload>,
  /// unset: place requests get no reply
  pub place_in_queue_resolver: Option<PlaceInQueueResolver>,
}

impl Resolvers {
  /// takes every callback `patch` sets and keeps the rest
  pub(crate) fn merge(&mut self, patch: Resolvers) {
    let Resolvers {
      user_end_point_cache,
      search_response_resolver,
      search_response_cache,
      browse_response_resolver,
      directory_contents_resolver,
      user_info_resolver,
      enqueue_download,
      place_in_queue_resolver,
    } = patch;
    if user_end_point_cache.is_some() {
      self.user_end_point_cache = user_end_point_cache;
    }
    if search_response_resolver.is_some() {
      self.search_response_resolver = search_response_resolver;
    }
    if search_response_cache.is_some() {
      self.search_response_cache = search_response_cache;
    }
    if browse_response_resolver.is_some() {
      self.browse_response_resolver = browse_response_resolver;
    }
    if directory_contents_resolver.is_some() {
      self.directory_contents_resolver = directory_contents_resolver;
    }
    if user_info_resolver.is_some() {
      self.user_info_resolver = user_info_resolver;
    }
    if enqueue_download.is_some() {
      self.enqueue_download = enqueue_download;
    }
    if place_in_queue_resolver.is_some() {
      self.place_in_queue_resolver = place_in_queue_resolver;
    }
  }
}

/// shows which callbacks are set; the callbacks themselves have nothing to print
impl fmt::Debug for Resolvers {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Resolvers")
      .field("user_end_point_cache", &self.user_end_point_cache.is_some())
      .field(
        "search_response_resolver",
        &self.search_response_resolver.is_some(),
      )
      .field(
        "search_response_cache",
        &self.search_response_cache.is_some(),
      )
      .field(
        "browse_response_resolver",
        &self.browse_response_resolver.is_some(),
      )
      .field(
        "directory_contents_resolver",
        &self.directory_contents_resolver.is_some(),
      )
      .field("user_info_resolver", &self.user_info_resolver.is_some())
      .field("enqueue_download", &self.enqueue_download.is_some())
      .field(
        "place_in_queue_resolver",
        &self.place_in_queue_resolver.is_some(),
      )
      .finish()
  }
}
