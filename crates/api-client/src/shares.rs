//! The share calls.

use api_types::shares::{Share, ShareScanQuery, SharesByHost};
use api_types::users::Directory;

use super::{Client, Result, Route, done, json};

impl Client {
  /// `GET /shares`: every host's shares.
  pub async fn shares(&self) -> Result<SharesByHost> {
    json(self.get(Route::new("shares"))).await
  }

  /// `GET /shares/{id}`: one share.
  pub async fn share(&self, id: &str) -> Result<Share> {
    json(self.get(Route::new("shares").segment(id))).await
  }

  /// `GET /shares/contents`: everything every host shares.
  pub async fn share_contents(&self) -> Result<Vec<Directory>> {
    json(self.get(Route::new("shares/contents"))).await
  }

  /// `GET /shares/{id}/contents`: what one share holds.
  pub async fn contents_of_share(&self, id: &str) -> Result<Vec<Directory>> {
    json(self.get(Route::new("shares").segment(id).then("contents"))).await
  }

  /// `PUT /shares`: starts a scan of this host's shares.
  pub async fn scan_shares(&self, force: bool) -> Result<()> {
    done(self.put(Route::new("shares").query(&ShareScanQuery { force })?)).await
  }

  /// `DELETE /shares`: stops the running scan.
  pub async fn cancel_share_scan(&self) -> Result<()> {
    done(self.delete(Route::new("shares"))).await
  }
}
