//! Browsing a peer's shares, and fetching one of its folders. A share listing can be large and
//! slow to come: the browse waits a long while for its first bytes, then follows the connection
//! that brings it until the whole response is in, reporting progress as it arrives. A folder comes
//! back as the folder and every folder below it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use soulseek_proto::peer::{
  BrowseResponse, Directory, FolderContentsRequest, FolderContentsResponse,
};
use soulseek_proto::{PeerCode, PeerMessage, RemoteFilename};
use tokio_util::sync::CancellationToken;

use crate::client::{Client, failed, non_blank};
use crate::error::{ClientError, ConnectionErrorKind};
use crate::events::Event;
use crate::lock;
use crate::peers::{IncomingBrowse, write_peer};
use crate::waiter::WaitKey;

/// how long a folder request waits for its answer unless told otherwise
const FOLDER_CONTENTS_TIMEOUT: Duration = Duration::from_secs(15);

/// How many folder requests of ours wait on each user. A folder's token is inside its
/// compressed payload, so this is what lets a folder nobody asked that user for be skipped from
/// its header.
#[derive(Default)]
pub(crate) struct FolderRequests(Mutex<HashMap<String, usize>>);

impl FolderRequests {
  /// counts a request to `username` until the returned guard is dropped
  fn hold(&self, username: &str) -> FolderRequest<'_> {
    *lock(&self.0).entry(username.to_owned()).or_default() += 1;
    FolderRequest {
      requests: self,
      username: username.to_owned(),
    }
  }

  /// whether a folder request of ours to `username` waits
  pub(crate) fn any(&self, username: &str) -> bool {
    lock(&self.0).contains_key(username)
  }
}

/// one folder request counted in [`FolderRequests`] while it waits
struct FolderRequest<'a> {
  requests: &'a FolderRequests,
  username: String,
}

impl Drop for FolderRequest<'_> {
  fn drop(&mut self) {
    let mut counts = lock(&self.requests.0);
    let count = counts
      .get_mut(&self.username)
      .expect("a held request is counted");
    *count -= 1;
    if *count == 0 {
      counts.remove(&self.username);
    }
  }
}

/// How far a browse response has come
#[derive(Clone, Debug, PartialEq)]
pub struct BrowseProgress {
  pub username: String,
  /// compressed bytes received
  pub bytes_transferred: u64,
  pub bytes_remaining: u64,
  pub percent_complete: f64,
  /// the compressed size of the whole response
  pub size: u64,
}

impl BrowseProgress {
  fn new(username: &str, received: u64, size: u64) -> Self {
    Self {
      username: username.to_owned(),
      bytes_transferred: received,
      bytes_remaining: size.saturating_sub(received),
      // an empty response is complete as soon as it starts
      percent_complete: if size == 0 {
        100.0
      } else {
        received as f64 / size as f64 * 100.0
      },
      size,
    }
  }
}

/// `(progress)` as a browse response arrives, before the client raises the event
pub type BrowseProgressUpdated = Arc<dyn Fn(&BrowseProgress) + Send + Sync>;

/// What shapes one browse
#[derive(Clone)]
pub struct BrowseOptions {
  /// how long to wait for the first bytes of the response; a peer with a large share takes a
  /// while to put it together
  pub response_timeout: Duration,
  pub progress_updated: Option<BrowseProgressUpdated>,
}

impl Default for BrowseOptions {
  fn default() -> Self {
    Self {
      response_timeout: Duration::from_secs(60),
      progress_updated: None,
    }
  }
}

/// shows whether the callback is set; the callback itself has nothing to print
impl std::fmt::Debug for BrowseOptions {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("BrowseOptions")
      .field("response_timeout", &self.response_timeout)
      .field("progress_updated", &self.progress_updated.is_some())
      .finish()
  }
}

impl Client {
  /// Fetches `username`'s whole share listing
  pub async fn browse(
    &self,
    username: &str,
    options: BrowseOptions,
    cancel: &CancellationToken,
  ) -> Result<BrowseResponse, ClientError> {
    non_blank("username", username)?;
    let verb = format!("browse user {username}");
    self.require_logged_in(&verb)?;
    let browsed = self
      .browse_from(username, &options, cancel)
      .await
      .map_err(|error| match error {
        error @ (ClientError::Timeout(_)
        | ClientError::Canceled
        | ClientError::UserOffline(_)
        | ClientError::Connection { .. }
        | ClientError::Message { .. }) => error,
        error => failed(&verb, error),
      });
    match &browsed {
      Ok(response) => tracing::debug!(
        username,
        directories = response.directories.len(),
        "browse completed"
      ),
      Err(error) => tracing::debug!(username, %error, "browse failed"),
    }
    browsed
  }

  async fn browse_from(
    &self,
    username: &str,
    options: &BrowseOptions,
    cancel: &CancellationToken,
  ) -> Result<BrowseResponse, ClientError> {
    // both in place before the request, so a fast answer is not missed; the wait for the first
    // bytes counts from the request, not from the connection made for it
    let mut result = Box::pin(self.waiter().wait_indefinitely::<Arc<BrowseResponse>>(
      WaitKey::Peer {
        code: PeerCode::BrowseResponse,
        username: username.to_owned(),
      },
      cancel.clone(),
    ));
    let first_bytes = self.waiter().wait_from_first_poll::<IncomingBrowse>(
      WaitKey::BrowseResponseConnection {
        username: username.to_owned(),
      },
      Some(options.response_timeout),
      cancel.clone(),
    );
    let endpoint = self.get_user_endpoint(username).await?;
    let conn = self
      .message_connection(username, Some(endpoint), cancel)
      .await?;
    write_peer(&conn, PeerMessage::BrowseRequest).await?;
    let IncomingBrowse {
      length,
      connection,
      mut received,
    } = first_bytes.await?;
    // the payload after the code; a response too large to follow failed the wait above
    let size = u64::from(length).saturating_sub(4);
    tracing::debug!(username, size, "browse response arriving");
    let progress = |received: u64| {
      let progress = BrowseProgress::new(username, received, size);
      if let Some(callback) = &options.progress_updated {
        callback(&progress);
      }
      self.emit(Event::BrowseProgressUpdated(progress));
    };
    // the first chunk was read with the header, so its own progress is not seen
    progress(0);
    let mut reported = 0;
    let handle = connection.handle().clone();
    let response = loop {
      tokio::select! {
        biased;
        response = &mut result => break response?,
        reason = handle.disconnected() => {
          // a response that arrived whole is decoded and delivered all the same
          if (*received.borrow() as u64) < size {
            return Err(ClientError::connection(
              ConnectionErrorKind::Other,
              format!("Peer connection disconnected unexpectedly: {}", reason.message),
            ));
          }
          break result.as_mut().await?;
        }
        changed = received.changed() => {
          if changed.is_err() {
            // the read loop moved on to the next frame; the response is complete or failed
            break result.as_mut().await?;
          }
          reported = *received.borrow_and_update() as u64;
          progress(reported);
        }
      }
    };
    if reported < size {
      progress(size);
    }
    Ok(Arc::unwrap_or_clone(response))
  }

  /// Fetches `directory` from `username`'s shares, with every directory below it as an entry of
  /// its own. Waits `timeout`, 15 seconds unless given, for the answer.
  pub async fn get_directory_contents(
    &self,
    username: &str,
    directory: impl Into<RemoteFilename>,
    token: Option<u32>,
    timeout: Option<Duration>,
    cancel: &CancellationToken,
  ) -> Result<Vec<Directory>, ClientError> {
    let directory = directory.into();
    non_blank("username", username)?;
    non_blank("directory", directory.as_str())?;
    let verb = format!("get the contents of {directory} from {username}");
    self.require_logged_in(&verb)?;
    let token = token.unwrap_or_else(|| self.next_token());
    let _asking = self.folder_requests().hold(username);
    let answer = self
      .waiter()
      .wait_from_first_poll::<FolderContentsResponse>(
        WaitKey::PeerToken {
          code: PeerCode::FolderContentsResponse,
          username: username.to_owned(),
          token,
        },
        Some(timeout.unwrap_or(FOLDER_CONTENTS_TIMEOUT)),
        cancel.clone(),
      );
    let ask = async {
      let endpoint = self.get_user_endpoint(username).await?;
      let conn = self
        .message_connection(username, Some(endpoint), cancel)
        .await?;
      let request = PeerMessage::FolderContentsRequest(FolderContentsRequest {
        token,
        directory_name: directory,
      });
      write_peer(&conn, request).await?;
      Ok(answer.await?.directories)
    };
    ask.await.map_err(|error| failed(&verb, error))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn browse_progress_counts_an_empty_response_as_complete() {
    assert_eq!(BrowseProgress::new("bob", 0, 0).percent_complete, 100.0);
    let half = BrowseProgress::new("bob", 50, 100);
    assert_eq!(half.percent_complete, 50.0);
    assert_eq!(half.bytes_remaining, 50);
  }
}
