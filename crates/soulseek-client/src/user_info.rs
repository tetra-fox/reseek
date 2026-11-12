//! Fetching a peer's user info: its description, picture, upload slots and queue. An answer
//! too large or malformed to read fails the request.

use soulseek_proto::peer::UserInfo;
use soulseek_proto::{PeerCode, PeerMessage};
use tokio_util::sync::CancellationToken;

use crate::client::{Client, failed, non_blank};
use crate::error::ClientError;
use crate::peers::write_peer;
use crate::waiter::WaitKey;

impl Client {
  /// Fetches `username`'s user info
  pub async fn get_user_info(
    &self,
    username: &str,
    cancel: &CancellationToken,
  ) -> Result<UserInfo, ClientError> {
    non_blank("username", username)?;
    let verb = format!("get the user info of {username}");
    self.require_logged_in(&verb)?;
    let answer = self.waiter().wait_from_first_poll::<UserInfo>(
      info_key(username),
      Some(self.waiter().default_timeout()),
      cancel.clone(),
    );
    let ask = async {
      let endpoint = self.get_user_endpoint(username).await?;
      let conn = self
        .message_connection(username, Some(endpoint), None, cancel)
        .await?;
      write_peer(&conn, PeerMessage::InfoRequest).await?;
      answer.await
    };
    ask.await.map_err(|error| failed(&verb, error))
  }

  /// a peer's user info arrived: it answers the request waiting for it
  pub(crate) fn user_info_received(&self, username: &str, info: UserInfo) {
    if !self.waiter().complete(&info_key(username), info) {
      tracing::debug!(username, "user info nobody waits for, dropped");
    }
  }

  /// whether a request of ours waits for `username`'s user info
  pub(crate) fn user_info_awaited(&self, username: &str) -> bool {
    self.waiter().has_wait(&info_key(username))
  }

  /// A peer's user info could not be read, being too long or malformed: the request waiting for
  /// it fails with `error` now rather than timing out
  pub(crate) fn user_info_unreadable(&self, username: &str, error: ClientError) {
    if self.waiter().fault(&info_key(username), error) {
      tracing::debug!(username, "user info request failed by an unreadable answer");
    }
  }
}

fn info_key(username: &str) -> WaitKey {
  WaitKey::Peer {
    code: PeerCode::InfoResponse,
    username: username.to_owned(),
  }
}
