//! Fetching a peer's user info: its description, picture, upload slots and queue. A picture
//! larger than we accept makes the answer unreadable, and the request then times out.

use soulseek_proto::peer::UserInfo;
use soulseek_proto::{PeerCode, PeerMessage};
use tokio_util::sync::CancellationToken;

use crate::client::{Client, failed, non_blank};
use crate::error::ClientError;
use crate::peers::write_peer;
use crate::waiter::WaitKey;

/// the largest picture accepted in a peer's user info
const MAX_PICTURE_BYTES: usize = 5 << 20;

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
    let answer = self.waiter().wait::<UserInfo>(
      WaitKey::Peer {
        code: PeerCode::InfoResponse,
        username: username.to_owned(),
      },
      None,
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

  /// A peer's user info arrived: it answers the request waiting for it, unless its picture is
  /// larger than we accept
  pub(crate) fn user_info_received(&self, username: &str, info: UserInfo) {
    let picture = info.picture.as_ref().map_or(0, Vec::len);
    if picture > MAX_PICTURE_BYTES {
      tracing::debug!(
        username,
        picture,
        limit = MAX_PICTURE_BYTES,
        "user info with a picture over the limit, dropped"
      );
      return;
    }
    let key = WaitKey::Peer {
      code: PeerCode::InfoResponse,
      username: username.to_owned(),
    };
    if !self.waiter().complete(&key, info) {
      tracing::debug!(username, "user info nobody waits for, dropped");
    }
  }
}
