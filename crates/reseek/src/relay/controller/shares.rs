//! An agent's shares reaching the controller. The agent asks on its socket and is handed a
//! token, refused when its index's schema is not this one's, when its last upload is still
//! loading or when its database is too large. It then uploads its share list and database
//! with the token; the call is checked before its body is read, the database is written to a
//! file as it arrives, at most the configured size, and checked and loaded as the agent's
//! host. Only then is the agent ready, and the agent's socket told so.

use std::path::Path;
use std::time::Instant as Clock;

use api_types::events::{Event, RelayAgentSharesUpdated};
use api_types::relay::{RelayErrorCode, RelayMessage};
use api_types::shares::Share;
use axum::extract::Multipart;
use chrono::Utc;
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Controller, Token};
use crate::api::error::ApiError;
use crate::db::remove_database;
use crate::relay::credential::Purpose;
use crate::relay::frames::{self, SHARE_TOKEN_LIFETIME, millis};
use crate::services::AgentCredential;
use crate::shares::{HostError, Shares};
use crate::sync::lock;

/// The largest share list an agent may send.
const SHARE_LIST_MAX: usize = 16 << 20;

/// Takes an agent out of the set of those loading an upload when its upload ends.
struct Loading<'a> {
  controller: &'a Controller,
  agent: String,
}

impl Drop for Loading<'_> {
  fn drop(&mut self) {
    lock(&self.controller.registry).syncing.remove(&self.agent);
  }
}

impl Controller {
  /// The answer to `shares_begin` from the agent `name` on `socket`: a token to upload with,
  /// or why not.
  pub(super) fn begin_shares(
    &self,
    name: &str,
    socket: u64,
    schema_version: u32,
    database_bytes: u64,
  ) -> String {
    let refusal = |code, detail: String| {
      warn!(agent = name, ?code, detail, "agent's share upload refused");
      frames::encode(
        None,
        RelayMessage::Error {
          code,
          detail: Some(detail),
          id: None,
        },
      )
    };
    let ours = Shares::schema_version();
    if schema_version != ours {
      return refusal(
        RelayErrorCode::SchemaMismatch,
        format!("the agent's share index has schema {schema_version}, this controller's {ours}"),
      );
    }
    let limit = self
      .config
      .current()
      .values
      .relay
      .limits
      .share_database_max
      .as_u64();
    if database_bytes > limit {
      return refusal(
        RelayErrorCode::TooLarge,
        format!("the share database has {database_bytes} bytes, at most {limit} are accepted"),
      );
    }
    let token = Uuid::new_v4();
    {
      let mut registry = lock(&self.registry);
      if registry.syncing.contains(name) {
        drop(registry);
        return refusal(
          RelayErrorCode::SyncInProgress,
          "the agent's last share upload is still loading".to_string(),
        );
      }
      let now = Instant::now();
      registry.share_tokens.retain(|_, t| t.expires > now);
      registry.share_tokens.insert(
        token,
        Token {
          agent: name.to_string(),
          socket,
          filename: None,
          expires: now + SHARE_TOKEN_LIFETIME,
        },
      );
    }
    debug!(agent = name, %token, "share upload token handed out");
    frames::encode(
      None,
      RelayMessage::SharesToken {
        token,
        expires_in: millis(SHARE_TOKEN_LIFETIME),
      },
    )
  }

  /// Takes the share list and database the agent `caller` uploads with `token` and loads them
  /// as its host.
  pub async fn receive_shares(
    &self,
    token: Uuid,
    caller: AgentCredential,
    mut form: Multipart,
  ) -> Result<(), ApiError> {
    let name = caller.agent.clone();
    info!(agent = name, %token, "Handling share upload from a caller claiming to be agent");
    // the token is used up whether or not the call checks out
    let issued = lock(&self.registry).share_tokens.remove(&token);
    let socket = match issued {
      Some(t) if t.agent == name && t.expires > Instant::now() => t.socket,
      _ => {
        warn!(agent = name, %token, "share upload with an unknown, expired or used token");
        return Err(ApiError::unauthorized("the token is unknown, expired or used").logged());
      }
    };
    self.authenticate(&caller, Purpose::Shares, token)?;
    if !lock(&self.registry).syncing.insert(name.clone()) {
      return Err(ApiError::conflict("sync_in_progress", None));
    }
    let _loading = Loading {
      controller: self,
      agent: name.clone(),
    };
    tokio::fs::create_dir_all(&self.temporary)
      .await
      .map_err(|e| ApiError::internal(format!("{}: {e}", self.temporary.display())))?;
    let file = self
      .temporary
      .join(format!("share_{name}_{:016x}.db", rand::random::<u64>()));
    let received = self.read_upload(&name, &mut form, &file).await;
    let loaded = match received {
      Ok(shares) => self.load(&name, socket, shares, &file).await,
      Err(e) => Err(e),
    };
    self.count("shares", if loaded.is_ok() { "loaded" } else { "refused" });
    let removed = file.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || remove_database(&removed)).await {
      warn!(file = %file.display(), error = %e, "uploaded share database not deleted");
    }
    loaded
  }

  /// Reads the parts of a share upload: the share list and the database, which is written to
  /// `file`.
  async fn read_upload(
    &self,
    name: &str,
    form: &mut Multipart,
    file: &Path,
  ) -> Result<Vec<Share>, ApiError> {
    let limit = self
      .config
      .current()
      .values
      .relay
      .limits
      .share_database_max
      .as_u64();
    let mut shares = None;
    let mut database = false;
    let started = Clock::now();
    while let Some(mut field) = form
      .next_field()
      .await
      .map_err(|e| ApiError::bad_request(e.body_text()))?
    {
      match field.name() {
        Some("shares") => {
          let mut text = Vec::new();
          while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ApiError::bad_request(e.body_text()))?
          {
            if text.len() + chunk.len() > SHARE_LIST_MAX {
              return Err(ApiError::too_large("the share list is too large"));
            }
            text.extend_from_slice(&chunk);
          }
          let list: Vec<Share> = serde_json::from_slice(&text)
            .map_err(|e| ApiError::invalid("the share list is not JSON shares", e.to_string()))?;
          shares = Some(list);
        }
        Some("database") => {
          debug!(agent = name, file = %file.display(), "Agent authenticated for token. Beginning download of shares");
          let mut out = tokio::fs::File::create_new(file)
            .await
            .map_err(|e| ApiError::internal(format!("{}: {e}", file.display())))?;
          let mut written: u64 = 0;
          while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ApiError::bad_request(e.body_text()))?
          {
            written += chunk.len() as u64;
            if written > limit {
              warn!(
                agent = name,
                limit, "agent's share database larger than allowed"
              );
              return Err(ApiError::too_large(format!(
                "the share database is larger than {limit} bytes"
              )));
            }
            out
              .write_all(&chunk)
              .await
              .map_err(|e| ApiError::internal(format!("{}: {e}", file.display())))?;
          }
          out
            .flush()
            .await
            .map_err(|e| ApiError::internal(format!("{}: {e}", file.display())))?;
          info!(
            agent = name,
            bytes = written,
            ms = started.elapsed().as_millis() as u64,
            "Download of shares from agent complete"
          );
          database = true;
        }
        other => debug!(
          agent = name,
          part = other,
          "unknown part of a share upload ignored"
        ),
      }
    }
    match (shares, database) {
      (Some(shares), true) => Ok(shares),
      (None, _) => Err(ApiError::bad_request("the part shares is missing")),
      (_, false) => Err(ApiError::bad_request("the part database is missing")),
    }
  }

  /// Loads the checked upload `file` with `shares` as the host of the agent `name` on
  /// `socket`, then marks it ready and tells it so.
  async fn load(
    &self,
    name: &str,
    socket: u64,
    shares: Vec<Share>,
    file: &Path,
  ) -> Result<(), ApiError> {
    let counted = match self.shares.load_host(name, shares, file).await {
      Ok(counted) => counted,
      Err(e @ HostError::Invalid(_)) => {
        warn!(agent = name, error = %e, "agent's share database refused");
        return Err(ApiError::bad_request(e.to_string()).logged());
      }
      Err(e @ HostError::Failed(_)) => return Err(ApiError::internal(e)),
    };
    let marked = {
      let mut registry = lock(&self.registry);
      match registry.agents.get_mut(name).filter(|r| r.socket == socket) {
        Some(registration) => {
          registration.record.ready = true;
          registration.record.shares_updated_at = Some(Utc::now());
          true
        }
        None => false,
      }
    };
    if !marked {
      // the agent left while its shares loaded, and its leaving unloaded nothing yet
      self.shares.remove_host(name);
      warn!(agent = name, "agent disconnected while its shares loaded");
      return Err(ApiError::unauthorized(format!("Agent {name} is not registered")).logged());
    }
    self.publish();
    self.send(
      name,
      frames::encode(
        None,
        RelayMessage::SharesReady {
          file_count: counted.files,
          directory_count: counted.directories,
        },
      ),
    );
    info!(
      agent = name,
      files = counted.files,
      directories = counted.directories,
      "Shares from agent ready."
    );
    self
      .events
      .raise(Event::RelayAgentSharesUpdated(RelayAgentSharesUpdated {
        version: 0,
        agent: name.to_string(),
        files: counted.files,
        directories: counted.directories,
      }));
    Ok(())
  }
}
