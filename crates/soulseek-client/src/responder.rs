//! Answering other users' searches. The application's resolver finds what matches; the answer
//! goes to the searcher over a message connection, dialed directly and requested through the
//! server at once. An answer that cannot be delivered is kept for a few minutes under the token
//! of that connection request, so it still reaches a searcher who acts on the request late;
//! answers that waited on one attempt are kept together under its request. How many requests
//! wait for an answer at once is the resolver's to bound.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use soulseek_proto::{PeerMessage, SearchQuery};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::cache::BoundedCache;
use crate::client::Client;
use crate::conn::MessageConn;
use crate::events::{Event, SearchDelivery};
use crate::peers::{Answering, Unreached};
use crate::resolvers::SearchReply;

/// how long an undelivered answer waits for its searcher to connect
const KEEP_UNDELIVERED: Duration = Duration::from_secs(180);

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
  /// the answers we could not deliver, by the connection request they rode on
  undelivered: Mutex<BoundedCache<u32, Vec<Undelivered>>>,
  /// connection requests the searcher refused while answers waited on them, so those answers
  /// are not kept; forgotten once the last of them is done
  refused: Mutex<BoundedCache<u32, ()>>,
  /// how many answers wait for a connection to each searcher
  waiting: Mutex<HashMap<String, usize>>,
}

impl SearchResponder {
  pub(crate) fn new(cache_size: u32) -> Self {
    Self {
      undelivered: Mutex::new(BoundedCache::new(cache_size as usize, KEEP_UNDELIVERED)),
      refused: Mutex::new(BoundedCache::new(cache_size as usize, KEEP_UNDELIVERED)),
      waiting: Mutex::new(HashMap::new()),
    }
  }

  fn undelivered(&self) -> std::sync::MutexGuard<'_, BoundedCache<u32, Vec<Undelivered>>> {
    crate::lock(&self.undelivered)
  }

  /// whether an answer of ours waits for a connection to `username`
  pub(crate) fn answers_waiting_for(&self, username: &str) -> bool {
    crate::lock(&self.waiting).contains_key(username)
  }

  /// `username`, whom answers wait to reach, cannot connect to us for the connection request
  /// `token` still being tried
  pub(crate) fn refuse(&self, token: u32) {
    crate::lock(&self.refused).insert(token, ());
  }

  /// counts an answer waiting for a connection to `username` until the returned guard is dropped
  fn wait_for(&self, username: &str) -> Waiting<'_> {
    *crate::lock(&self.waiting)
      .entry(username.to_owned())
      .or_default() += 1;
    Waiting {
      responder: self,
      username: username.to_owned(),
      request: None,
    }
  }

  /// the searcher the undelivered answers kept under `response_token` are for
  pub(crate) fn kept_for(&self, response_token: u32) -> Option<String> {
    self
      .undelivered()
      .get(&response_token)
      .and_then(|kept| kept.first())
      .map(|kept| kept.username.clone())
  }

  /// follows a change of the cache bound
  pub(crate) fn resize(&self, cache_size: u32) {
    self.undelivered().set_capacity(cache_size as usize);
    crate::lock(&self.refused).set_capacity(cache_size as usize);
  }
}

/// One answer waiting for a connection to its searcher. The last one for a searcher to be done
/// forgets the refusal of the request it rode on.
struct Waiting<'a> {
  responder: &'a SearchResponder,
  username: String,
  /// the connection request the answer rode on, once its attempt failed
  request: Option<u32>,
}

impl Drop for Waiting<'_> {
  fn drop(&mut self) {
    let mut waiting = crate::lock(&self.responder.waiting);
    let count = waiting
      .get_mut(&self.username)
      .expect("a waiting answer is counted");
    *count -= 1;
    if *count > 0 {
      return;
    }
    waiting.remove(&self.username);
    if let Some(request) = self.request {
      crate::lock(&self.responder.refused).remove(&request);
    }
  }
}

impl Client {
  /// How many answers to other users' searches are kept now because their searcher could not
  /// be reached yet.
  pub fn undelivered_search_responses(&self) -> usize {
    self.responder().undelivered().values().map(Vec::len).sum()
  }

  /// Answers another user's search on a task of its own.
  pub(crate) fn respond_to_search(&self, username: String, token: u32, query: String) {
    let client = self.clone();
    let span = tracing::debug_span!("search_request", username, token);
    tokio::spawn(
      async move {
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
    let mut waiting = self.responder().wait_for(username);
    let conn = match self
      .reach(username, Some(endpoint), &CancellationToken::new())
      .await
    {
      Ok(conn) => conn,
      Err(Unreached { error, request }) => {
        waiting.request = request;
        let delivery = || {
          Event::SearchResponseDeliveryFailed(SearchDelivery {
            username: username.to_owned(),
            token,
            query: query.to_owned(),
            search_response: reply.clone(),
          })
        };
        // an attempt that made no request of its own, such as one answering the searcher's
        // request to us, leaves the searcher nothing to act on late
        let Some(request) = request else {
          tracing::debug!(%error, "the searcher cannot be reached; search response dropped");
          self.emit(delivery());
          return false;
        };
        if crate::lock(&self.responder().refused)
          .get(&request)
          .is_some()
        {
          tracing::debug!(response_token = request, %error, "the searcher cannot connect; search response dropped");
          self.emit(delivery());
          return false;
        }
        let mut undelivered = self.responder().undelivered();
        let mut kept = undelivered.remove(&request).unwrap_or_default();
        kept.push(Undelivered {
          username: username.to_owned(),
          token,
          query: query.to_owned(),
          reply,
        });
        undelivered.insert(request, kept);
        tracing::debug!(
          response_token = request,
          %error,
          "Failed to connect to deliver search results; cached response for potential delayed delivery"
        );
        return false;
      }
    };
    drop(waiting);
    self
      .deliver(&conn, &mut Answering::default(), username, token, query, reply)
      .await
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
    let response = match reply.plain() {
      SearchReply::Response(response) => response.clone(),
      encoded => {
        let limits = self.options().limits.frame_limits();
        match PeerMessage::decode_with(&encoded.frame(), &limits) {
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
    answering: &mut Answering,
    username: &str,
    token: u32,
    query: &str,
  ) {
    if let Some(reply) = self.resolve_search(username, token, query).await {
      self
        .deliver(conn, answering, username, token, query, Arc::new(reply))
        .await;
    }
  }

  /// A searcher acted late on a connection request that carried answers we could not deliver,
  /// and has just connected; the kept answers go out on that connection
  pub(crate) async fn try_respond_cached(&self, response_token: u32) {
    let Some(kept) = self.responder().undelivered().remove(&response_token) else {
      return;
    };
    for kept in kept {
      self.respond_kept(response_token, kept).await;
    }
  }

  /// sends one kept answer on the searcher's connection
  async fn respond_kept(&self, response_token: u32, kept: Undelivered) {
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
      return;
    };
    let remote = conn.handle().remote();
    if !kept.reply.may_reach(remote.ip()) {
      tracing::debug!(
        username = kept.username,
        %remote,
        "kept search response confined to another address; not delivered"
      );
      self.emit(Event::SearchResponseDeliveryFailed(delivery));
      return;
    }
    match conn.write(kept.reply.frame()).await {
      Ok(()) => {
        tracing::debug!(
          username = kept.username,
          token = kept.token,
          "kept search response delivered"
        );
        self.emit(Event::SearchResponseDelivered(delivery));
      }
      Err(error) => {
        tracing::debug!(username = kept.username, %error, "kept search response not delivered");
        self.emit(Event::SearchResponseDeliveryFailed(delivery));
      }
    }
  }

  /// The searcher answers were kept for cannot connect to us; the answers are given up
  pub(crate) fn try_discard(&self, response_token: u32) -> bool {
    let Some(kept) = self.responder().undelivered().remove(&response_token) else {
      return false;
    };
    for kept in kept {
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
    }
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
    answering: &mut Answering,
    username: &str,
    token: u32,
    query: &str,
    reply: Arc<SearchReply>,
  ) -> bool {
    let remote = conn.handle().remote();
    if !reply.may_reach(remote.ip()) {
      tracing::debug!(
        username,
        %remote,
        query,
        "search response confined to another address; not delivered"
      );
      self.emit(Event::SearchResponseDeliveryFailed(SearchDelivery {
        username: username.to_owned(),
        token,
        query: query.to_owned(),
        search_response: reply,
      }));
      return false;
    }
    match answering.send_frame(conn, reply.frame()).await {
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

#[cfg(test)]
mod tests {
  use soulseek_proto::frame::{self, FrameKind};
  use soulseek_proto::server::{CannotConnectResponse, LoginResponse};
  use soulseek_proto::{Limits, Secret, ServerRequest, ServerResponse};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio::net::{TcpListener, TcpStream};

  use super::*;
  use crate::options::ClientOptions;

  /// the next message the client wrote to `server` that `pick` accepts, skipping the others
  async fn request<T>(
    server: &mut TcpStream,
    buffer: &mut Vec<u8>,
    pick: impl Fn(ServerRequest) -> Option<T>,
  ) -> T {
    loop {
      while let Some((_, consumed)) =
        frame::split(FrameKind::Server, buffer, &Limits::DEFAULT).unwrap()
      {
        let frame: Vec<u8> = buffer.drain(..consumed).collect();
        if let Some(picked) = pick(ServerRequest::decode(&frame).unwrap()) {
          return picked;
        }
      }
      let mut chunk = [0; 4096];
      let read = server.read(&mut chunk).await.unwrap();
      assert!(read > 0, "the client closed the connection");
      buffer.extend_from_slice(&chunk[..read]);
    }
  }

  #[tokio::test]
  async fn a_refused_connection_request_that_carried_no_answer_leaves_nothing_behind() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = Client::new(ClientOptions {
      enable_listener: false,
      ..ClientOptions::default()
    })
    .unwrap();
    let cancel = CancellationToken::new();
    let mut buffer = Vec::new();
    let connecting = client.connect("127.0.0.1", port, "alice", "hunter2", &cancel);
    let serving = async {
      let (mut server, _) = listener.accept().await.unwrap();
      request(&mut server, &mut buffer, |request| {
        matches!(request, ServerRequest::Login(_)).then_some(())
      })
      .await;
      let accepted = ServerResponse::Login(LoginResponse::Success {
        greeting: String::new(),
        ip: std::net::Ipv4Addr::LOCALHOST,
        hash: Secret::new("0123456789abcdef0123456789abcdef"),
        is_supporter: false,
      });
      server.write_all(&accepted.encode()).await.unwrap();
      server
    };
    let (connected, mut server) = tokio::join!(connecting, serving);
    connected.unwrap();
    // nothing listens where bob is said to be, and bob refuses the request the server relays
    let nowhere = std::net::TcpListener::bind("127.0.0.1:0")
      .unwrap()
      .local_addr()
      .unwrap();
    let asking = client.get_or_add_message_connection("bob", Some(nowhere), &cancel);
    let refusing = async {
      let token = request(&mut server, &mut buffer, |request| match request {
        ServerRequest::ConnectToPeer(request) => Some(request.token),
        _ => None,
      })
      .await;
      let refusal = ServerResponse::CannotConnect(CannotConnectResponse {
        token,
        username: None,
      });
      server.write_all(&refusal.encode()).await.unwrap();
    };
    let (asked, ()) = tokio::join!(asking, refusing);
    assert!(asked.is_err());
    assert_eq!(crate::lock(&client.responder().refused).len(), 0);
  }
}
