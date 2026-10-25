//! Answering other users' searches. The application's resolver finds what matches; the answer
//! goes to the searcher over a message connection, dialled directly and requested through the
//! server at once. An answer that cannot be delivered is kept for a few minutes under the token
//! of that connection request, so it still reaches a searcher who acts on the request late. How
//! many searches are worked out at once is bounded; past the bound a request is dropped.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use soulseek_proto::{PeerMessage, SearchQuery};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::cache::BoundedCache;
use crate::client::Client;
use crate::conn::MessageConn;
use crate::events::{Event, SearchDelivery};
use crate::limit::{Limiter, Permit};
use crate::log::LogLimiter;
use crate::resolvers::SearchReply;

/// how long an undelivered answer waits for its searcher to connect
const KEEP_UNDELIVERED: Duration = Duration::from_secs(180);

static RESPONDERS_BUSY: LogLimiter = LogLimiter::new();

/// An answer we could not deliver, kept under the token of the connection request it rode on
#[derive(Clone)]
struct Undelivered {
  username: String,
  /// the searcher's token
  token: u32,
  query: String,
  reply: Arc<SearchReply>,
}

/// What the client keeps for answering searches
pub(crate) struct SearchResponder {
  undelivered: Mutex<BoundedCache<u32, Undelivered>>,
  /// connection requests the searcher refused while we were still trying to reach it, so the
  /// answer they carried is not kept
  refused: Mutex<BoundedCache<u32, ()>>,
  /// searches being answered at once
  responding: Limiter,
}

impl SearchResponder {
  pub(crate) fn new(cache_size: u32, max_concurrent: u32) -> Self {
    Self {
      undelivered: Mutex::new(BoundedCache::new(cache_size as usize, KEEP_UNDELIVERED)),
      refused: Mutex::new(BoundedCache::new(cache_size as usize, KEEP_UNDELIVERED)),
      responding: Limiter::new(max_concurrent),
    }
  }

  fn undelivered(&self) -> std::sync::MutexGuard<'_, BoundedCache<u32, Undelivered>> {
    crate::lock(&self.undelivered)
  }

  /// the peer behind the connection request `token`, still being tried, cannot connect to us
  pub(crate) fn refuse(&self, token: u32) {
    crate::lock(&self.refused).insert(token, ());
  }

  /// the searcher an undelivered answer is kept for under `response_token`
  pub(crate) fn kept_for(&self, response_token: u32) -> Option<String> {
    self
      .undelivered()
      .get(&response_token)
      .map(|kept| kept.username.clone())
  }

  /// follows a change of the cache bound
  pub(crate) fn resize(&self, cache_size: u32) {
    self.undelivered().set_capacity(cache_size as usize);
    crate::lock(&self.refused).set_capacity(cache_size as usize);
  }
}

impl Client {
  /// A permit to answer `username`'s search, or `None` when the bound on searches being answered
  /// at once is reached and the request is to be dropped
  fn responding(&self, username: &str, token: u32) -> Option<Permit> {
    let limit = self.options().limits.max_concurrent_search_responses;
    let permit = self.responder().responding.try_acquire(limit);
    if permit.is_none()
      && let Some(suppressed) = RESPONDERS_BUSY.allow()
    {
      tracing::debug!(
        username,
        token,
        suppressed,
        "too many searches being answered, request dropped"
      );
    }
    permit
  }

  /// Answers another user's search on a task of its own, unless the bound on searches being
  /// answered at once is reached, when the request is dropped.
  pub(crate) fn respond_to_search(&self, username: String, token: u32, query: String) {
    let Some(permit) = self.responding(&username, token) else {
      return;
    };
    let client = self.clone();
    let span = tracing::debug_span!("search_request", username, token);
    tokio::spawn(
      async move {
        let _permit = permit;
        client.try_respond(&username, token, &query).await;
      }
      .instrument(span),
    );
  }

  /// Resolves an answer to `username`'s search and delivers it; whether it was delivered
  pub(crate) async fn try_respond(&self, username: &str, token: u32, query: &str) -> bool {
    let Some(reply) = self.resolve_search(username, token, query).await else {
      return false;
    };
    let reply = Arc::new(reply);
    if self.username().as_deref() == Some(username) {
      return self.deliver_to_self(username, token, query, reply);
    }
    let endpoint = match self.get_user_endpoint(username).await {
      Ok(endpoint) => endpoint,
      Err(error) => {
        tracing::debug!(query, %error, "Failed to send search response");
        return false;
      }
    };
    let response_token = self.next_token();
    let conn = match self
      .message_connection(
        username,
        Some(endpoint),
        Some(response_token),
        &CancellationToken::new(),
      )
      .await
    {
      Ok(conn) => conn,
      Err(error) => {
        if crate::lock(&self.responder().refused)
          .remove(&response_token)
          .is_some()
        {
          tracing::debug!(response_token, %error, "the searcher cannot connect; search response dropped");
          self.emit(Event::SearchResponseDeliveryFailed(SearchDelivery {
            username: username.to_owned(),
            token,
            query: query.to_owned(),
            search_response: reply,
          }));
          return false;
        }
        self.responder().undelivered().insert(
          response_token,
          Undelivered {
            username: username.to_owned(),
            token,
            query: query.to_owned(),
            reply,
          },
        );
        tracing::debug!(
          response_token,
          %error,
          "Failed to connect to deliver search results; cached response for potential delayed delivery"
        );
        return false;
      }
    };
    self.deliver(&conn, username, token, query, reply).await
  }

  /// Our answer to a search we sent ourselves on purpose goes straight to that search: a
  /// connection to our own listener would stand for both ends of one user's connection
  fn deliver_to_self(
    &self,
    username: &str,
    token: u32,
    query: &str,
    reply: Arc<SearchReply>,
  ) -> bool {
    let response = match reply.as_ref() {
      SearchReply::Response(response) => response.clone(),
      SearchReply::Encoded(frame) => {
        let limits = self.options().limits.frame_limits();
        match PeerMessage::decode_with(frame, &limits) {
          Ok(PeerMessage::SearchResponse(response)) => response,
          Ok(other) => {
            tracing::warn!(
              code = other.code().code(),
              "encoded search reply is another message"
            );
            return false;
          }
          // the decoder has logged what is wrong with it
          Err(_) => return false,
        }
      }
    };
    self.search_response(response);
    tracing::debug!(query, "search response delivered to our own search");
    self.emit(Event::SearchResponseDelivered(SearchDelivery {
      username: username.to_owned(),
      token,
      query: query.to_owned(),
      search_response: reply,
    }));
    true
  }

  /// Answers a search a peer sent on its message connection, on that connection
  pub(crate) async fn respond_on(
    &self,
    conn: &MessageConn,
    username: &str,
    token: u32,
    query: &str,
  ) {
    let Some(_permit) = self.responding(username, token) else {
      return;
    };
    if let Some(reply) = self.resolve_search(username, token, query).await {
      self
        .deliver(conn, username, token, query, Arc::new(reply))
        .await;
    }
  }

  /// A searcher acted late on a connection request that carried an answer we could not
  /// deliver, and has just connected; the kept answer goes out on that connection
  pub(crate) async fn try_respond_cached(&self, response_token: u32) -> bool {
    let Some(kept) = self.responder().undelivered().remove(&response_token) else {
      return false;
    };
    let delivery = SearchDelivery {
      username: kept.username.clone(),
      token: kept.token,
      query: kept.query.clone(),
      search_response: kept.reply.clone(),
    };
    let Some(conn) = self.peers().cached(&kept.username) else {
      tracing::debug!(
        username = kept.username,
        response_token,
        "no connection to deliver the kept search response on"
      );
      self.emit(Event::SearchResponseDeliveryFailed(delivery));
      return false;
    };
    match conn.write(kept.reply.frame()).await {
      Ok(()) => {
        tracing::debug!(
          username = kept.username,
          token = kept.token,
          "kept search response delivered"
        );
        self.emit(Event::SearchResponseDelivered(delivery));
        true
      }
      Err(error) => {
        tracing::debug!(username = kept.username, %error, "kept search response not delivered");
        self.emit(Event::SearchResponseDeliveryFailed(delivery));
        false
      }
    }
  }

  /// The searcher an answer was kept for cannot connect to us; the answer is given up
  pub(crate) fn try_discard(&self, response_token: u32) -> bool {
    let Some(kept) = self.responder().undelivered().remove(&response_token) else {
      return false;
    };
    tracing::debug!(
      username = kept.username,
      token = kept.token,
      response_token,
      "kept search response discarded"
    );
    self.emit(Event::SearchResponseDeliveryFailed(SearchDelivery {
      username: kept.username,
      token: kept.token,
      query: kept.query,
      search_response: kept.reply,
    }));
    true
  }

  /// Announces the request and asks the application for an answer; `None` when there is none
  /// to send
  async fn resolve_search(&self, username: &str, token: u32, query: &str) -> Option<SearchReply> {
    self.emit(Event::SearchRequestReceived {
      username: username.to_owned(),
      token,
      query: query.to_owned(),
    });
    let resolve = self.options().resolvers.search_response_resolver.clone()?;
    let reply = match resolve(username.to_owned(), token, SearchQuery::parse(query)).await {
      Ok(reply) => reply?,
      Err(error) => {
        tracing::warn!(query, %error, "Error resolving search response");
        return None;
      }
    };
    // an empty answer is never sent; an encoded one always is
    let files = reply.file_count();
    if files == Some(0) {
      return None;
    }
    tracing::debug!(files, query, "Resolved files for search");
    Some(reply)
  }

  async fn deliver(
    &self,
    conn: &MessageConn,
    username: &str,
    token: u32,
    query: &str,
    reply: Arc<SearchReply>,
  ) -> bool {
    match conn.write(reply.frame()).await {
      Ok(()) => {
        tracing::debug!(query, "search response delivered");
        self.emit(Event::SearchResponseDelivered(SearchDelivery {
          username: username.to_owned(),
          token,
          query: query.to_owned(),
          search_response: reply,
        }));
        true
      }
      Err(error) => {
        tracing::debug!(query, %error, "Failed to send search response");
        false
      }
    }
  }
}
