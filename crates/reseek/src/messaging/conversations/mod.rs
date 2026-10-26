//! Private conversations: a message received is stored, then pushed and raised; one sent goes to
//! the server, then is stored; reading a conversation acknowledges its messages to the server,
//! which then stops delivering them again at each login. The server's own notices are kept in a
//! conversation of their own, acknowledged at once and never unread.

pub mod store;

use api_types::blocking::BlockScope;
use api_types::chat::{ExportFormat, NotifyLevel, ServerMessage};
use api_types::conversations::{
  Conversation, ConversationListQuery, ConversationMessage, ConversationMessagesQuery,
  ConversationUpdate, PrivateMessage,
};
use api_types::events::{Event, PrivateMessageReceived};
use api_types::live::LiveFrame;
use axum::response::Response;
use chrono::{DateTime, Utc};
use soulseek_client::ClientError;
use tracing::{debug, info, trace, warn};

use super::{Messaging, export, lock};
use crate::api::error::ApiError;
use crate::db::DbError;
use crate::users::Touch;
use store::{Inbound, SERVER};

/// How many messages a conversation is read with.
const WINDOW: u32 = 100;

fn internal(error: DbError) -> ApiError {
  ApiError::internal(error)
}

fn no_conversation(username: &str) -> ApiError {
  ApiError::not_found(format!("there is no conversation with {username}"))
}

/// The answer to a request the server could not take: 409 while not logged in, else 502.
pub(super) fn unsent(error: ClientError) -> ApiError {
  match error {
    ClientError::InvalidOperation(detail) => ApiError::conflict("not logged in", Some(detail)),
    other => ApiError::upstream("the server could not be reached", Some(other.to_string())),
  }
}

impl Messaging {
  /// A private message from a user not blocked from messaging us: stored, then pushed, raised
  /// and counted unread. It stays unacknowledged until it is read, so the server delivers it
  /// again at the next login until then; one the store refused is left to that.
  pub(super) async fn received(&self, message: soulseek_client::PrivateMessage) {
    let inbound = Inbound {
      id: message.id,
      timestamp: DateTime::<Utc>::from(message.timestamp),
      username: message.username,
      message: message.message,
      replayed: message.replayed,
    };
    let username = inbound.username.clone();
    let notice = username == SERVER;
    let now = Utc::now();
    let stored = self
      .db
      .write(move |conn| {
        let tx = conn.transaction()?;
        if notice {
          store::ensure(&tx, SERVER, now)?;
        } else {
          store::activate(&tx, &inbound.username, now)?;
        }
        let stored = store::receive(&tx, &inbound)?;
        tx.commit()?;
        Ok(stored)
      })
      .await;
    let stored = match stored {
      Ok(stored) => stored,
      Err(e) => {
        return warn!(
          username,
          error = %e,
          "private message not stored; the server delivers it again at the next login"
        );
      }
    };
    debug!(
      username,
      id = ?stored.id,
      replayed = stored.was_replayed,
      "private message stored"
    );
    trace!(username, text = stored.message, "private message text");
    if notice {
      info!("server notice received");
      if let Err(e) = self.acknowledge_ids(SERVER, &[message.id]).await {
        debug!(error = ?e, "server notice not acknowledged; it comes again at the next login");
      }
      self
        .live
        .publish(LiveFrame::ApplicationServerMessage(ServerMessage {
          message: stored.message.clone(),
          received_at: now,
        }));
    } else {
      lock(&self.active).insert(username.clone());
      self.users.touch(&username, Touch::MessageIn).await;
      self
        .events
        .raise(Event::PrivateMessageReceived(PrivateMessageReceived {
          version: 0,
          message: stored.clone(),
        }));
      self.watch_peer(&username);
    }
    self
      .live
      .publish(LiveFrame::ConversationsMessage(ConversationMessage {
        username: username.clone(),
        message: stored,
      }));
    if !notice {
      self.conversation_changed(&username).await;
    }
  }

  /// Pushes the conversation with `username` as it is now, and the unread counts.
  async fn conversation_changed(&self, username: &str) {
    let name = username.to_string();
    match self
      .db
      .read(move |conn| store::conversation(conn, &name))
      .await
    {
      Ok(Some(conversation)) => {
        self
          .live
          .publish(LiveFrame::ConversationsUpdate(ConversationUpdate {
            conversation,
          }));
      }
      Ok(None) => {}
      Err(e) => warn!(username, error = %e, "conversation not read to push it"),
    }
    self.unread.changed();
  }

  /// Watches the peer of an open conversation, so its presence is known, while logged in.
  fn watch_peer(&self, username: &str) {
    if !self.logged_in_now() || self.users.is_watched(username) {
      return;
    }
    let (users, client, username) = (
      self.users.clone(),
      self.client().clone(),
      username.to_string(),
    );
    self
      .tasks
      .spawn(async move { users.watch(&client, &username).await });
  }

  /// After a login: the peers of the open conversations are watched.
  pub(super) fn watch_conversations(&self) {
    let peers: Vec<String> = lock(&self.active).iter().cloned().collect();
    for peer in peers {
      self.watch_peer(&peer);
    }
  }

  /// Tells the server the messages from `username` with `ids` were read, and marks them so;
  /// the first that cannot be told stops it and is the error, the ones told before stay
  /// marked.
  async fn acknowledge_ids(&self, username: &str, ids: &[u32]) -> Result<(), ApiError> {
    let mut told = Vec::new();
    let mut failed = None;
    for &id in ids {
      match self.client().acknowledge_private_message(id).await {
        Ok(()) => told.push(id),
        Err(e) => {
          failed = Some(e);
          break;
        }
      }
    }
    if !told.is_empty() {
      let name = username.to_string();
      self
        .db
        .write(move |conn| store::acknowledged(conn, &name, &told))
        .await
        .map_err(internal)?;
    }
    match failed {
      Some(error) => Err(unsent(error)),
      None => Ok(()),
    }
  }

  pub(super) async fn conversations(
    &self,
    query: ConversationListQuery,
  ) -> Result<Vec<Conversation>, ApiError> {
    self
      .db
      .read(move |conn| store::list(conn, &query))
      .await
      .map_err(internal)
  }

  pub(super) async fn conversation(
    &self,
    username: &str,
    include_messages: bool,
  ) -> Result<Conversation, ApiError> {
    let name = username.to_string();
    let found = self
      .db
      .read(move |conn| {
        let Some(mut conversation) = store::conversation(conn, &name)? else {
          return Ok(None);
        };
        if include_messages {
          conversation.messages = Some(store::messages(conn, &name, None, WINDOW, false)?);
        }
        Ok(Some(conversation))
      })
      .await
      .map_err(internal)?;
    found.ok_or_else(|| no_conversation(username))
  }

  pub(super) async fn conversation_messages(
    &self,
    username: &str,
    query: ConversationMessagesQuery,
  ) -> Result<Vec<PrivateMessage>, ApiError> {
    let name = username.to_string();
    self
      .db
      .read(move |conn| {
        if !store::exists(conn, &name)? {
          return Ok(None);
        }
        let page = store::messages(
          conn,
          &name,
          query.before,
          query.limit,
          query.un_acknowledged_only,
        )?;
        Ok(Some(page))
      })
      .await
      .map_err(internal)?
      .ok_or_else(|| no_conversation(username))
  }

  /// Sends a private message to `username` and stores it once the server took it; a user
  /// blocked from messages is refused.
  pub(super) async fn send_private(&self, username: &str, message: String) -> Result<(), ApiError> {
    if self.users.is_blocked(username, None, BlockScope::Messages) {
      return Err(ApiError::forbidden("user is blocked"));
    }
    let name = username.to_string();
    let now = Utc::now();
    self
      .db
      .write(move |conn| store::activate(conn, &name, now))
      .await
      .map_err(internal)?;
    lock(&self.active).insert(username.to_string());
    self
      .client()
      .send_private_message(username, &message)
      .await
      .map_err(unsent)?;
    let name = username.to_string();
    let sent = self
      .db
      .write(move |conn| store::sent(conn, &name, &message, Utc::now()))
      .await
      .map_err(|e| {
        warn!(username, error = %e, "private message sent but not stored");
        internal(e)
      })?;
    debug!(username, local_id = sent.local_id, "private message sent");
    self.users.touch(username, Touch::MessageOut).await;
    self
      .live
      .publish(LiveFrame::ConversationsMessage(ConversationMessage {
        username: username.to_string(),
        message: sent,
      }));
    self.conversation_changed(username).await;
    self.watch_peer(username);
    Ok(())
  }

  /// Whether there is a conversation with `username`, else its 404.
  async fn existing(&self, username: &str) -> Result<(), ApiError> {
    let name = username.to_string();
    if self
      .db
      .read(move |conn| store::exists(conn, &name))
      .await
      .map_err(internal)?
    {
      Ok(())
    } else {
      Err(no_conversation(username))
    }
  }

  pub(super) async fn acknowledge_all(&self, username: &str) -> Result<(), ApiError> {
    self.existing(username).await?;
    let name = username.to_string();
    let ids = self
      .db
      .read(move |conn| store::unacknowledged(conn, &name))
      .await
      .map_err(internal)?;
    let told = self.acknowledge_ids(username, &ids).await;
    self.conversation_changed(username).await;
    told
  }

  pub(super) async fn acknowledge(&self, username: &str, id: u32) -> Result<(), ApiError> {
    let name = username.to_string();
    if !self
      .db
      .read(move |conn| store::has_message(conn, &name, id))
      .await
      .map_err(internal)?
    {
      return Err(ApiError::not_found(format!(
        "there is no message {id} from {username}"
      )));
    }
    let told = self.acknowledge_ids(username, &[id]).await;
    self.conversation_changed(username).await;
    told
  }

  /// Acknowledges a conversation and closes it; with `purge` its messages are deleted.
  pub(super) async fn close(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    self.acknowledge_all(username).await?;
    self.closed(username, purge).await
  }

  /// Closes the conversation with `username` and deletes its messages, for a user forgotten:
  /// what can be acknowledged is, and the server delivers the rest again at the next login.
  pub async fn purge(&self, username: &str) -> Result<(), ApiError> {
    if let Err(e) = self.acknowledge_all(username).await {
      if e.status() == axum::http::StatusCode::NOT_FOUND {
        return Ok(());
      }
      debug!(username, error = ?e, "messages of a forgotten user not acknowledged");
    }
    self.closed(username, true).await
  }

  async fn closed(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    let name = username.to_string();
    self
      .db
      .write(move |conn| store::close(conn, &name, purge, Utc::now()))
      .await
      .map_err(internal)?;
    lock(&self.active).remove(username);
    debug!(username, purge, "conversation closed");
    self.conversation_changed(username).await;
    Ok(())
  }

  pub(super) async fn export_conversation(
    &self,
    username: &str,
    format: ExportFormat,
  ) -> Result<Response, ApiError> {
    self.existing(username).await?;
    let name = username.to_string();
    let messages = self
      .db
      .read(move |conn| store::all(conn, &name))
      .await
      .map_err(internal)?;
    let us = self.us().unwrap_or_default();
    export::conversation(username, &us, &messages, format)
  }

  pub(super) async fn set_conversation_notify(
    &self,
    username: &str,
    notify: NotifyLevel,
  ) -> Result<(), ApiError> {
    let name = username.to_string();
    self
      .db
      .write(move |conn| store::set_notify(conn, &name, notify))
      .await
      .map_err(internal)?;
    self.conversation_changed(username).await;
    Ok(())
  }

  pub(super) async fn set_draft(&self, username: &str, draft: String) -> Result<(), ApiError> {
    let name = username.to_string();
    self
      .db
      .write(move |conn| store::set_draft(conn, &name, &draft))
      .await
      .map_err(internal)
  }
}
