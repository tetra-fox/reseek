//! The shares API: the shares of every host, what they hold as a browsing peer sees it, and
//! starting and stopping a scan.

use std::collections::BTreeMap;

use api_types::search::{File, FileAttribute};
use api_types::shares::{Share, SharesByHost};
use api_types::users::Directory;
use async_trait::async_trait;
use soulseek_proto::peer;

use super::{LOCAL_HOST, Shares, repository};
use crate::api::error::ApiError;
use crate::db::DbError;
use crate::services::ShareService;

/// A shared file as the API shows it.
fn file(file: peer::File) -> File {
  let attributes: Vec<FileAttribute> = file.attributes.iter().cloned().map(Into::into).collect();
  File {
    code: file.code,
    bit_rate: file.bit_rate(),
    length: file.length(),
    is_variable_bit_rate: file.is_variable_bit_rate(),
    sample_rate: file.sample_rate(),
    bit_depth: file.bit_depth(),
    attribute_count: u32::try_from(attributes.len()).unwrap_or(u32::MAX),
    filename: file.filename.as_str().to_string(),
    size: file.size,
    extension: file.extension,
    is_locked: false,
    suspect: None,
    attributes,
  }
}

/// A shared directory as the API shows it.
fn directory(directory: peer::Directory) -> Directory {
  let files: Vec<File> = directory.files.into_iter().map(file).collect();
  Directory {
    name: directory.name.as_str().to_string(),
    file_count: u32::try_from(files.len()).unwrap_or(u32::MAX),
    files,
  }
}

fn failed(error: DbError) -> ApiError {
  ApiError::internal(format!("the share index could not be read: {error}"))
}

fn unknown(id: &str) -> ApiError {
  ApiError::not_found(format!("no share has the id {id}"))
}

#[async_trait]
impl ShareService for Shares {
  async fn shares(&self) -> Result<SharesByHost, ApiError> {
    let local = self
      .listed()
      .iter()
      .map(super::share::Share::to_api)
      .collect();
    Ok(SharesByHost(BTreeMap::from([(
      LOCAL_HOST.to_string(),
      local,
    )])))
  }

  async fn share(&self, id: &str) -> Result<Share, ApiError> {
    self
      .listed()
      .iter()
      .find(|share| share.id == id)
      .map(super::share::Share::to_api)
      .ok_or_else(|| unknown(id))
  }

  async fn contents(&self) -> Result<Vec<Directory>, ApiError> {
    let listed = self.browse().await.map_err(failed)?;
    Ok(listed.into_iter().map(directory).collect())
  }

  async fn share_contents(&self, id: &str) -> Result<Vec<Directory>, ApiError> {
    let alias = self
      .listed()
      .iter()
      .find(|share| share.id == id)
      .map(|share| share.alias.clone())
      .ok_or_else(|| unknown(id))?;
    let listed = self
      .db
      .read(move |conn| repository::browse(conn, Some(&alias)))
      .await
      .map_err(failed)?;
    Ok(listed.into_iter().map(directory).collect())
  }

  async fn scan(&self, force: bool) -> Result<(), ApiError> {
    Shares::scan(self, force)
      .map_err(|_| ApiError::conflict("A share scan is already in progress.", None))
  }

  async fn cancel_scan(&self) -> Result<(), ApiError> {
    Shares::cancel_scan(self).map_err(|_| ApiError::conflict("No share scan is in progress.", None))
  }
}
