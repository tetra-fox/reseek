//! The users directory and blocking calls of the API, answered by the [`Directory`]. The calls
//! that ask the network about a user are not here.

use api_types::auth::Principal;
use api_types::blocking::{
  Block, BlockImportResult, BlockRequest, BlockScope, BlockSource, BlockedUsers,
};
use api_types::events::{Event, UnblockReason, UserBlocked, UserUnblocked};
use api_types::users::{
  UserList, UserListQuery, UserProfile, UserSummary, UserTransfers, UserTransfersQuery,
};
use async_trait::async_trait;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::sync::PoisonError;
use tracing::debug;

use super::store::{self, Change};
use super::{Directory, SUMMARY_FOR, blocking, groups, profile, transfers};
use crate::api::error::ApiError;
use crate::db::shifted;
use crate::services::{BlockImport, BlockingService, UserService};

fn internal(error: impl std::fmt::Display) -> ApiError {
  ApiError::internal(error)
}

/// The content type a picture's first bytes say it has.
fn picture_type(bytes: &[u8]) -> &'static str {
  if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
    "image/png"
  } else if bytes.starts_with(b"\xff\xd8\xff") {
    "image/jpeg"
  } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
    "image/gif"
  } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
    "image/webp"
  } else if bytes.starts_with(b"BM") {
    "image/bmp"
  } else {
    "application/octet-stream"
  }
}

impl Directory {
  /// Applies `change` to `username`'s row and pushes the new summary.
  pub(super) async fn change(&self, username: &str, change: Change) -> Result<(), ApiError> {
    let name = username.to_string();
    self
      .db
      .write(move |conn| store::change(conn, &name, &change, Utc::now()))
      .await
      .map_err(internal)?;
    self.changed(username).await;
    Ok(())
  }

  /// The scopes a block without its own gets.
  fn default_scopes(&self) -> Vec<BlockScope> {
    self
      .config
      .current()
      .values
      .blocking
      .default_scopes
      .iter()
      // the configuration is only loaded once every name in it is a scope
      .filter_map(|name| name.parse().ok())
      .collect()
  }

  /// Whether `username` is the account we log in as.
  fn is_us(&self, username: &str) -> bool {
    self.config.current().values.soulseek.username.as_deref() == Some(username)
  }

  /// Stores `block`, raises `UserBlocked` as `by` and pushes the user's summary.
  async fn put_block(&self, block: Block, by: &Principal) -> Result<(), ApiError> {
    let stored = block.clone();
    self
      .db
      .write(move |conn| blocking::store(conn, &stored, Utc::now()))
      .await
      .map_err(internal)?;
    self.blocks.set(&block);
    self.events.raise(Event::UserBlocked(UserBlocked {
      version: 0,
      username: block.username.clone(),
      reason: (!block.reason.is_empty()).then(|| block.reason.clone()),
      scopes: block.scopes.clone(),
      expires_at: block.expires_at,
      by: by.subject.clone(),
    }));
    self.changed(&block.username).await;
    Ok(())
  }

  /// Whether `username` has a block of ours that has not expired.
  async fn blocked_by_us(&self, username: &str) -> Result<bool, ApiError> {
    let name = username.to_string();
    let now = Utc::now();
    let ours = self
      .db
      .read(move |conn| blocking::stored_of(conn, &name))
      .await
      .map_err(internal)?;
    Ok(ours.is_some_and(|b| blocking::stands(b.expires_at, now)))
  }
}

#[async_trait]
impl UserService for Directory {
  async fn list(&self, query: UserListQuery) -> Result<UserList, ApiError> {
    Directory::list(self, query).await
  }

  async fn summary(&self, username: &str) -> Result<UserSummary, ApiError> {
    let cached = self
      .summaries
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .get(username)
      .filter(|(at, _)| at.elapsed() < SUMMARY_FOR)
      .map(|(_, summary)| summary.clone());
    match cached {
      Some(summary) => Ok(summary),
      None => self.summary_of(username).await,
    }
  }

  async fn profile(&self, username: &str, refresh: bool) -> Result<UserProfile, ApiError> {
    if refresh {
      return Err(ApiError::unavailable(Some(
        "refreshing a profile from the network not implemented yet".to_string(),
      )));
    }
    let (row, block) = self.stored(username).await.map_err(internal)?;
    let figures = self
      .figures
      .get(&self.transfers, username)
      .await
      .map_err(internal)?;
    let options = self.config.current().values.clone();
    let user = profile::known(&options, username, row.as_ref(), block);
    Ok(profile::profile(user, row.as_ref(), figures))
  }

  async fn picture(&self, username: &str) -> Result<Response, ApiError> {
    let name = username.to_string();
    let picture = self
      .db
      .read(move |conn| store::picture(conn, &name))
      .await
      .map_err(internal)?
      .ok_or_else(|| ApiError::not_found(format!("no picture of {username} is kept")))?;
    let digest = Sha256::digest(&picture);
    let tag: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let mut response = picture.clone().into_response();
    let headers = response.headers_mut();
    headers.insert(
      header::CONTENT_TYPE,
      HeaderValue::from_static(picture_type(&picture)),
    );
    headers.insert(
      header::CACHE_CONTROL,
      HeaderValue::from_static("private, max-age=3600"),
    );
    headers.insert(
      header::ETAG,
      HeaderValue::from_str(&format!("\"{tag}\"")).map_err(internal)?,
    );
    Ok(response)
  }

  async fn set_notes(&self, username: &str, notes: String) -> Result<(), ApiError> {
    self.change(username, Change::Notes(notes)).await
  }

  async fn set_tags(&self, username: &str, tags: Vec<String>) -> Result<(), ApiError> {
    let mut unique = Vec::new();
    for tag in tags {
      if !unique.contains(&tag) {
        unique.push(tag);
      }
    }
    self.change(username, Change::Tags(unique)).await
  }

  async fn set_pinned(&self, username: &str, pinned: bool) -> Result<(), ApiError> {
    self.change(username, Change::Pinned(pinned)).await
  }

  async fn set_muted(&self, username: &str, muted: bool) -> Result<(), ApiError> {
    self.change(username, Change::Muted(muted)).await
  }

  async fn set_group(&self, username: &str, group: Option<String>) -> Result<(), ApiError> {
    let options = self.config.current().values.clone();
    if let Some(wanted) = &group
      && let Some((configured, _)) = groups::configured(&options, username)
      && configured != *wanted
    {
      return Err(ApiError::conflict(
        format!("the configuration puts {username} in the group {configured}"),
        Some(format!(
          "change transfers.groups.user_defined.{configured}.members to move the user"
        )),
      ));
    }
    self.change(username, Change::Group(group)).await
  }

  async fn transfers(
    &self,
    username: &str,
    query: UserTransfersQuery,
  ) -> Result<UserTransfers, ApiError> {
    transfers::page(&self.transfers, username, query)
      .await
      .map_err(internal)
  }

  async fn remove(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    let name = username.to_string();
    let deleted = self
      .db
      .write(move |conn| store::delete(conn, &name))
      .await
      .map_err(internal)?;
    if !deleted {
      return Err(ApiError::not_found(format!(
        "{username} is not in the directory"
      )));
    }
    self.blocks.unset(username);
    if purge {
      let removed = transfers::remove_all(&self.transfers, username)
        .await
        .map_err(internal)?;
      debug!(
        username,
        removed, "transfers of a forgotten user removed from the listings"
      );
    }
    self.removed(username);
    Ok(())
  }
}

#[async_trait]
impl BlockingService for Directory {
  async fn blocked(&self, include_config: bool) -> Result<BlockedUsers, ApiError> {
    let now = Utc::now();
    let mut users = self
      .db
      .read(move |conn| blocking::stored(conn, now))
      .await
      .map_err(internal)?;
    if !include_config {
      return Ok(BlockedUsers {
        users,
        rules: Default::default(),
      });
    }
    users.extend(
      self
        .blocks
        .configured_members()
        .iter()
        .map(|name| blocking::configured(name)),
    );
    Ok(BlockedUsers {
      users,
      rules: self.blocks.rules(),
    })
  }

  async fn block(
    &self,
    by: &Principal,
    username: &str,
    request: BlockRequest,
    expires_in: Option<std::time::Duration>,
  ) -> Result<Block, ApiError> {
    if self.is_us(username) {
      return Err(ApiError::conflict("we can not block ourselves", None));
    }
    let now = Utc::now();
    let expires_at = match (expires_in, request.expires_at) {
      (Some(length), _) => {
        // a length too long for chrono ends at the last time stored, as `shifted` bounds it
        let length = chrono::Duration::from_std(length).unwrap_or(chrono::Duration::MAX);
        Some(shifted(now, length))
      }
      (None, Some(at)) if at <= now => {
        return Err(ApiError::bad_request("expiresAt is in the past"));
      }
      (None, at) => at,
    };
    let block = Block {
      username: username.to_string(),
      reason: request.reason,
      scopes: request.scopes.unwrap_or_else(|| self.default_scopes()),
      created_at: Some(now),
      expires_at,
      created_by: Some(by.name.clone()),
      source: BlockSource::Api,
    };
    self.put_block(block.clone(), by).await?;
    if request.cancel_transfers {
      debug!(
        username,
        "no transfer engine runs, so no transfer of the user is cancelled"
      );
    }
    Ok(block)
  }

  async fn unblock(&self, by: &Principal, username: &str) -> Result<(), ApiError> {
    let name = username.to_string();
    let deleted = self
      .db
      .write(move |conn| blocking::delete(conn, &name))
      .await
      .map_err(internal)?;
    if !deleted {
      if self.blocks.configuration_blocks(username) {
        return Err(ApiError::conflict(
          "blocked by configuration",
          Some("change transfers.groups.blacklisted to unblock the user".to_string()),
        ));
      }
      return Err(ApiError::not_found(format!("{username} is not blocked")));
    }
    self.blocks.unset(username);
    self.events.raise(Event::UserUnblocked(UserUnblocked {
      version: 0,
      username: username.to_string(),
      reason: UnblockReason::Manual,
      by: Some(by.subject.clone()),
    }));
    self.changed(username).await;
    Ok(())
  }

  async fn import(
    &self,
    by: &Principal,
    import: BlockImport,
  ) -> Result<BlockImportResult, ApiError> {
    let now = Utc::now();
    let default_scopes = self.default_scopes();
    let (offered, wanted): (
      usize,
      Vec<(String, String, Vec<BlockScope>, Option<DateTime<Utc>>)>,
    ) = match import {
      BlockImport::Names(names) => (
        names.len(),
        names
          .into_iter()
          .map(|name| (name, String::new(), default_scopes.clone(), None))
          .collect(),
      ),
      BlockImport::Blocks(blocks) => (
        blocks.len(),
        blocks
          .into_iter()
          .filter(|b| b.source == BlockSource::Api && blocking::stands(b.expires_at, now))
          .map(|b| (b.username, b.reason, b.scopes, b.expires_at))
          .collect(),
      ),
    };
    let mut result = BlockImportResult::default();
    for (username, reason, scopes, expires_at) in wanted {
      if username.is_empty() || self.is_us(&username) || self.blocked_by_us(&username).await? {
        continue;
      }
      let block = Block {
        username,
        reason,
        scopes,
        created_at: Some(now),
        expires_at,
        created_by: Some(by.name.clone()),
        source: BlockSource::Api,
      };
      self.put_block(block, by).await?;
      result.added += 1;
    }
    result.skipped = u32::try_from(offered).unwrap_or(u32::MAX) - result.added;
    Ok(result)
  }

  async fn export(&self) -> Result<Vec<Block>, ApiError> {
    let now = Utc::now();
    self
      .db
      .read(move |conn| blocking::stored(conn, now))
      .await
      .map_err(internal)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pictures_are_typed_by_their_first_bytes() {
    assert_eq!(picture_type(b"\x89PNG\r\n\x1a\nrest"), "image/png");
    assert_eq!(picture_type(b"\xff\xd8\xff\xe0"), "image/jpeg");
    assert_eq!(picture_type(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
    assert_eq!(picture_type(b"<svg"), "application/octet-stream");
  }
}
