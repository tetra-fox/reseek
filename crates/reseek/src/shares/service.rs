//! The shares API: the shares of every host, the local one and each relay agent's, what they
//! hold as a browsing peer sees it, and starting and stopping a scan.

use std::collections::BTreeMap;

use api_types::shares::{Share, ShareFolders, SharesByHost};
use api_types::users::Directory;
use async_trait::async_trait;

use super::{FolderError, LOCAL_HOST, Shares};
use crate::api::error::ApiError;
use crate::db::DbError;
use crate::services::ShareService;

fn failed(error: DbError) -> ApiError {
  ApiError::internal(format!("the share index could not be read: {error}"))
}

fn unknown(id: &str) -> ApiError {
  ApiError::not_found(format!("No share has the id {id}"))
}

#[async_trait]
impl ShareService for Shares {
  async fn shares(&self) -> Result<SharesByHost, ApiError> {
    let mut hosts = BTreeMap::from([(LOCAL_HOST.to_string(), self.local_shares())]);
    hosts.extend(self.agent_shares());
    Ok(SharesByHost(hosts))
  }

  async fn share(&self, id: &str) -> Result<Share, ApiError> {
    self
      .local_shares()
      .into_iter()
      .chain(
        self
          .agent_shares()
          .into_iter()
          .flat_map(|(_, shares)| shares),
      )
      .find(|share| share.id == id)
      .ok_or_else(|| unknown(id))
  }

  async fn contents(&self) -> Result<Vec<Directory>, ApiError> {
    let listed = self.browse().await.map_err(failed)?;
    Ok(listed.into_iter().map(Directory::from).collect())
  }

  async fn share_contents(&self, id: &str) -> Result<Vec<Directory>, ApiError> {
    let alias = self.share(id).await?.alias;
    let listed = Shares::share_contents(self, alias).await.map_err(failed)?;
    Ok(listed.into_iter().map(Directory::from).collect())
  }

  async fn folders(&self, path: Option<String>) -> Result<ShareFolders, ApiError> {
    Shares::folders(self, path).await.map_err(|e| match e {
      FolderError::NotListed => ApiError::not_found(e.to_string()),
      FolderError::Relative | FolderError::Traverses => ApiError::bad_request(e.to_string()),
      FolderError::Failed(detail) => ApiError::internal(detail),
    })
  }

  async fn scan(&self, force: bool) -> Result<(), ApiError> {
    Shares::scan(self, force, reseek_shares::ScanOrigin::Asked)
      .map_err(|_| ApiError::conflict("A share scan is already in progress", None))
  }

  async fn cancel_scan(&self) -> Result<(), ApiError> {
    Shares::cancel_scan(self).map_err(|_| ApiError::conflict("No share scan is in progress", None))
  }
}
