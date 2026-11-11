//! The users directory calls of the API and its blocking calls, answered by the [`Directory`]
//! from what it stores. The calls that ask the network about a user are the users service's.

use std::sync::Arc;

use api_types::auth::Principal;
use api_types::events::{Event, UnblockReason, UserBlocked, UserUnblocked};
use api_types::users::blocking::{
  Block, BlockImportResult, BlockRequest, BlockScope, BlockSource, BlockedUsers,
};
use api_types::users::{UserProfile, UserSummary, UserTransfers, UserTransfersQuery};
use async_trait::async_trait;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use reseek_shares::lock;
use sha2::{Digest, Sha256};
use tracing::debug;

use super::store::{self, Change};
use super::{Directory, SUMMARY_FOR, blocking, profile, transfers};
use crate::api::error::ApiError;
use crate::db::shifted;
use crate::services::{BlockImport, BlockingService};

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

/// The SHA-256 of `picture` in hex, which changes whenever the picture does.
pub(crate) fn picture_tag(picture: &[u8]) -> String {
  Sha256::digest(picture)
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect()
}

/// `picture` as the whole response: its type from its first bytes, `cache_control`, and its
/// [`picture_tag`] as its ETag.
pub(crate) fn picture_response(picture: Vec<u8>, cache_control: &'static str) -> Response {
  let tag = picture_tag(&picture);
  let content_type = picture_type(&picture);
  let mut response = picture.into_response();
  let headers = response.headers_mut();
  headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
  headers.insert(
    header::CACHE_CONTROL,
    HeaderValue::from_static(cache_control),
  );
  let etag = HeaderValue::from_str(&format!("\"{tag}\""))
    .expect("hex digits in quotes are a valid header value");
  headers.insert(header::ETAG, etag);
  response
}

impl Directory {
  /// Applies `change` to `username`'s row and pushes the new summary.
  pub(super) async fn change(&self, username: &str, change: Change) -> Result<(), ApiError> {
    let name = username.to_string();
    let buddy = match &change {
      Change::Buddy(buddy) => Some(*buddy),
      _ => None,
    };
    self
      .db
      .write(move |conn| store::change(conn, &name, &change, Utc::now()))
      .await
      .map_err(internal)?;
    if let Some(buddy) = buddy {
      self.regroup([username], || self.buddies.set(username, buddy));
    }
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
  pub(super) fn is_us(&self, username: &str) -> bool {
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

impl Directory {
  /// What is stored about `username`, kept a minute while the options and the speed limits in
  /// force stay the same.
  pub(crate) async fn summary(&self, username: &str) -> Result<UserSummary, ApiError> {
    let config = self.config.current();
    let alternative = self.state.current().alternative_speed_limits;
    let cached = lock(&self.summaries)
      .get(username)
      .filter(|cached| {
        cached.at.elapsed() < SUMMARY_FOR
          && Arc::ptr_eq(&cached.options, &config)
          && cached.alternative == alternative
      })
      .map(|cached| cached.summary.clone());
    match cached {
      Some(summary) => Ok(summary),
      None => self.summary_of(username).await,
    }
  }

  /// `username`'s whole profile from what is stored.
  pub(crate) async fn profile(&self, username: &str) -> Result<UserProfile, ApiError> {
    let (row, block) = self.stored(username).await.map_err(internal)?;
    let figures = self
      .figures
      .get(&self.transfers, &self.messaging, username)
      .await
      .map_err(internal)?;
    let options = self.config.current().values.clone();
    let alternative = self.state.current().alternative_speed_limits;
    let user = profile::known(&options, alternative, username, row.as_ref(), block);
    let user = self.with_network_and_conversation(user).await;
    Ok(profile::profile(user, row.as_ref(), figures))
  }

  /// `username`'s stored picture as the whole response, with its type and caching headers.
  pub(crate) async fn picture(&self, username: &str) -> Result<Response, ApiError> {
    let name = username.to_string();
    let picture = self
      .db
      .read(move |conn| store::picture(conn, &name))
      .await
      .map_err(internal)?
      .ok_or_else(|| ApiError::not_found(format!("No picture of {username} is stored")))?;
    // profiles name it by a URL that changes with it, so a copy kept a day is never stale
    Ok(picture_response(picture, "private, max-age=86400"))
  }

  /// Replaces our notes on `username`.
  pub(crate) async fn set_notes(&self, username: &str, notes: String) -> Result<(), ApiError> {
    self.change(username, Change::Notes(notes)).await
  }

  /// Puts `username` in exactly the groups `wanted`, each once: user-defined groups of the
  /// configuration, and any they are in already that it no longer has.
  pub(crate) async fn set_groups(
    &self,
    username: &str,
    wanted: Vec<String>,
  ) -> Result<(), ApiError> {
    let options = self.config.current().values.clone();
    let held = self
      .groups
      .facts(username)
      .map(|facts| facts.memberships)
      .unwrap_or_default();
    let mut groups: Vec<String> = Vec::new();
    for name in wanted {
      let defined = options.transfers.groups.user_defined.contains_key(&name);
      if !defined && !held.contains(&name) {
        return Err(ApiError::invalid(
          "Unknown group",
          format!("{name} is not a user-defined group"),
        ));
      }
      if !groups.contains(&name) {
        groups.push(name);
      }
    }
    groups.sort();
    self
      .change(username, Change::Groups(groups.clone()))
      .await?;
    self.regroup([username], || self.memberships_changed(username, groups));
    Ok(())
  }

  /// Moves every member of the group `from` into the user-defined group `to`, and says how
  /// many moved.
  pub(crate) async fn move_members(&self, from: &str, to: &str) -> Result<u64, ApiError> {
    if !self
      .config
      .current()
      .values
      .transfers
      .groups
      .user_defined
      .contains_key(to)
    {
      return Err(ApiError::invalid(
        "Unknown group",
        format!("{to} is not a user-defined group"),
      ));
    }
    if from == to {
      return Ok(0);
    }
    let (from_name, to_name) = (from.to_string(), to.to_string());
    let moved = self
      .db
      .write(move |conn| store::move_members(conn, &from_name, &to_name))
      .await
      .map_err(internal)?;
    self.regroup(moved.iter().map(String::as_str), || {
      for username in &moved {
        self.membership_moved(username, from, Some(to));
      }
    });
    for username in &moved {
      self.changed(username).await;
    }
    Ok(moved.len() as u64)
  }

  /// Takes every member out of the group `group`, and says how many there were.
  pub(crate) async fn let_go_members(&self, group: &str) -> Result<u64, ApiError> {
    let name = group.to_string();
    let members = self
      .db
      .write(move |conn| store::let_go(conn, &name))
      .await
      .map_err(internal)?;
    self.regroup(members.iter().map(String::as_str), || {
      for username in &members {
        self.membership_moved(username, group, None);
      }
    });
    for username in &members {
      self.changed(username).await;
    }
    Ok(members.len() as u64)
  }

  /// Mutes or unmutes `username`.
  pub(crate) async fn set_muted(&self, username: &str, muted: bool) -> Result<(), ApiError> {
    self.change(username, Change::Muted(muted)).await
  }

  /// A page of `username`'s transfers.
  pub(crate) async fn transfers(
    &self,
    username: &str,
    query: UserTransfersQuery,
  ) -> Result<UserTransfers, ApiError> {
    transfers::page(&self.transfers, username, query)
      .await
      .map_err(internal)
  }

  /// Forgets `username` and their block; with `purge` their transfers leave the listings.
  pub(crate) async fn remove(&self, username: &str, purge: bool) -> Result<(), ApiError> {
    // a buddy change made meanwhile would leave the map holding a buddy without a row
    let _changing = self.buddies.changing().await;
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
    self.regroup([username], || {
      self.buddies.set(username, None);
      self.memberships_changed(username, Vec::new());
    });
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
    // a member we also block ourselves is listed once, as our block, which is the one that can
    // be lifted here
    let configured: Vec<Block> = self
      .blocks
      .configured_members()
      .iter()
      .filter(|name| !users.iter().any(|ours| &ours.username == *name))
      .map(|name| blocking::configured(name))
      .collect();
    users.extend(configured);
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
      return Err(ApiError::conflict("This account cannot block itself", None));
    }
    let now = Utc::now();
    let expires_at = match (expires_in, request.expires_at) {
      (Some(length), _) => {
        // a length too long for chrono ends at the last time stored, as `shifted` bounds it
        let length = chrono::Duration::from_std(length).unwrap_or(chrono::Duration::MAX);
        Some(shifted(now, length))
      }
      (None, at) => at,
    };
    if !blocking::stands(expires_at, now) {
      return Err(ApiError::bad_request("A block must end in the future"));
    }
    let addresses = blocking::addresses_of(request.addresses).map_err(ApiError::bad_request)?;
    let block = Block {
      username: username.to_string(),
      reason: request.reason,
      scopes: request.scopes.unwrap_or_else(|| self.default_scopes()),
      addresses,
      created_at: Some(now),
      expires_at,
      created_by: Some(by.name.clone()),
      source: BlockSource::Api,
    };
    self.put_block(block.clone(), by).await?;
    Ok(block)
  }

  async fn unblock(&self, by: &Principal, username: &str) -> Result<(), ApiError> {
    let name = username.to_string();
    let deleted = self
      .db
      .write(move |conn| blocking::delete(conn, &name, Utc::now()))
      .await
      .map_err(internal)?;
    if !deleted {
      if self.blocks.configuration_blocks(username) {
        return Err(ApiError::conflict(
          "Blocked by the configuration",
          Some("Change transfers.groups.blacklisted to unblock the user".to_string()),
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
    let (offered, wanted): (usize, Vec<Block>) = match import {
      BlockImport::Names(names) => (
        names.len(),
        names
          .into_iter()
          .map(|username| Block {
            username,
            reason: String::new(),
            scopes: default_scopes.clone(),
            addresses: Vec::new(),
            created_at: None,
            expires_at: None,
            created_by: None,
            source: BlockSource::Api,
          })
          .collect(),
      ),
      BlockImport::Blocks(blocks) => (
        blocks.len(),
        blocks
          .into_iter()
          .filter(|b| {
            b.source == BlockSource::Api
              && blocking::stands(b.expires_at, now)
              && blocking::reason_fits(&b.reason)
          })
          .filter_map(|mut b| {
            match blocking::addresses_of(std::mem::take(&mut b.addresses)) {
              Ok(addresses) => b.addresses = addresses,
              Err(reason) => {
                debug!(username = b.username, reason, "imported block skipped");
                return None;
              }
            }
            Some(b)
          })
          .collect(),
      ),
    };
    let mut result = BlockImportResult::default();
    for wanted in wanted {
      let username = &wanted.username;
      if username.is_empty() || self.is_us(username) || self.blocked_by_us(username).await? {
        continue;
      }
      let block = Block {
        created_at: Some(now),
        created_by: Some(by.name.clone()),
        source: BlockSource::Api,
        ..wanted
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
