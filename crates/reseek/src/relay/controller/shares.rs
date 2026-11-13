//! An agent's shares reaching the controller. The agent asks on its socket and is handed a
//! token, refused when its index's schema is not this one's, when its last upload is still
//! loading or when its database is too large. It then uploads its share list and database
//! with the token; the call is checked before its body is read, the database is written to a
//! file in the cache directory as it arrives, at most the configured size, and checked and
//! loaded as the agent's host. Only then is the agent ready, and the agent's socket told so.
//! An upload whose socket closes before its database arrived stops there.

use std::path::{Path, PathBuf};
use std::time::Instant as Clock;

use api_types::events::{Event, RelayAgentSharesUpdated};
use api_types::relay::{RelayErrorCode, RelayMessage};
use api_types::shares::Share;
use axum::extract::Multipart;
use chrono::Utc;
use reseek_shares::lock;
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Controller, ShareToken};
use crate::api::error::ApiError;
use crate::db::remove_database;
use crate::services::AgentCredential;
use crate::shares::{HostError, Shares};
use reseek_relay_proto::credential::Purpose;
use reseek_relay_proto::frames::{self, SHARE_TOKEN_LIFETIME, millis};

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

/// The file a share upload is written to. The call removes it once the upload is loaded or
/// refused; a call dropped before then, when its agent goes away mid-upload, removes it on the
/// blocking pool.
struct UploadFile(Option<PathBuf>);

impl UploadFile {
  fn path(&self) -> &Path {
    self
      .0
      .as_deref()
      .expect("the file is there until it is removed")
  }

  /// Removes the file, unless it was renamed into place as the host's index.
  async fn remove(mut self) {
    let file = self.0.take().expect("the file is removed once");
    let removed = file.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || remove_database(&removed)).await {
      warn!(file = %file.display(), error = %e, "uploaded share database not deleted");
    }
  }
}

impl Drop for UploadFile {
  fn drop(&mut self) {
    if let Some(file) = self.0.take() {
      // a call is dropped from the runtime, which must not wait for the disk
      tokio::task::spawn_blocking(move || remove_database(&file));
    }
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
    // `against` is what the agent was held to, the size limit or its own schema, so a refusal
    // for a limit since changed is warned about again but one for an index that grew is not
    let refusal = |code, against: Option<u64>, detail: String| {
      let repeated = lock(&self.registry)
        .refusals_warned
        .insert(name.to_string(), (code, against))
        == Some((code, against));
      if repeated {
        debug!(agent = name, ?code, detail, "agent's share upload refused");
      } else {
        warn!(agent = name, ?code, detail, "agent's share upload refused");
      }
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
        Some(u64::from(schema_version)),
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
        Some(limit),
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
          None,
          "the agent's last share upload is still loading".to_string(),
        );
      }
      registry.refusals_warned.remove(name);
      let now = Instant::now();
      registry.share_tokens.retain(|_, t| t.expires > now);
      registry.share_tokens.insert(
        token,
        ShareToken {
          agent: name.to_string(),
          socket,
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
        return Err(
          ApiError::unauthorized("The token is unknown, expired or already used").logged(),
        );
      }
    };
    self.authenticate(&caller, Purpose::Shares, token)?;
    let left = {
      let mut registry = lock(&self.registry);
      let Some(left) = registry
        .agents
        .get(&name)
        .filter(|r| r.socket == socket)
        .map(|r| r.left.clone())
      else {
        drop(registry);
        warn!(agent = name, "share upload for a socket that closed");
        return Err(ApiError::unauthorized(format!("Agent {name} is not registered")).logged());
      };
      if !registry.syncing.insert(name.clone()) {
        return Err(ApiError::conflict("sync_in_progress", None));
      }
      left
    };
    let _loading = Loading {
      controller: self,
      agent: name.clone(),
    };
    let file = UploadFile(Some(self.shares.upload_file(&name)));
    // the upload ends with the socket it was asked on, so the agent's next socket can send one
    let received = tokio::select! {
      () = left.cancelled() => {
        info!(agent = name, "share upload from agent stopped: its socket closed");
        Err(ApiError::conflict(
          "agent_disconnected",
          Some(format!("Agent {name} disconnected while it sent its shares")),
        ))
      }
      received = self.read_upload(&name, &mut form, file.path()) => received,
    };
    let loaded = match received {
      Ok(shares) => self.load(&name, socket, shares, file.path()).await,
      Err(e) => Err(e),
    };
    self.count("shares", if loaded.is_ok() { "loaded" } else { "refused" });
    file.remove().await;
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
              return Err(ApiError::too_large("The share list is too large"));
            }
            text.extend_from_slice(&chunk);
          }
          let list: Vec<Share> = serde_json::from_slice(&text)
            .map_err(|e| ApiError::invalid("The share list is not valid JSON", e.to_string()))?;
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
                "The share database is larger than {limit} bytes"
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
      (None, _) => Err(ApiError::bad_request("The shares part is missing")),
      (_, false) => Err(ApiError::bad_request("The database part is missing")),
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
