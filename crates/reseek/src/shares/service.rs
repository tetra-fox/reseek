//! The shares API: the shares of every host, what they hold as a browsing peer sees it, and
//! starting and stopping a scan.

use std::collections::BTreeMap;

use api_types::shares::{Share, SharesByHost};
use api_types::users::Directory;
use async_trait::async_trait;

use super::{LOCAL_HOST, Shares, repository};
use crate::api::error::ApiError;
use crate::db::DbError;
use crate::services::ShareService;

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
    Ok(listed.into_iter().map(Directory::from).collect())
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
    Ok(listed.into_iter().map(Directory::from).collect())
  }

  async fn scan(&self, force: bool) -> Result<(), ApiError> {
    Shares::scan(self, force)
      .map_err(|_| ApiError::conflict("A share scan is already in progress.", None))
  }

  async fn cancel_scan(&self) -> Result<(), ApiError> {
    Shares::cancel_scan(self).map_err(|_| ApiError::conflict("No share scan is in progress.", None))
  }
}
