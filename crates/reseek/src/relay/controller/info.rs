//! Asking an agent about a file before a peer's request for it is queued: whether it still
//! exists and how long it is. An agent that is not ready, leaves, or does not answer in the
//! configured time gets no answer, and the request is refused as not shared.

use api_types::relay::RelayMessage;
use reseek_shares::lock;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{debug, warn};
use uuid::Uuid;

use super::Controller;
use reseek_relay_proto::frames;

/// A file info request waiting for its answer.
pub(super) struct FileInfoWait {
  pub agent: String,
  answer: oneshot::Sender<(bool, u64)>,
}

impl Controller {
  /// Asks the agent `agent` whether `filename` exists and how long it is. `None` when the agent
  /// is not ready, leaves, or does not answer within the configured time.
  pub async fn file_info(&self, agent: &str, filename: &str) -> Option<(bool, u64)> {
    let answer = self.ask_file_info(agent, filename).await;
    self.count(
      "file_info",
      if answer.is_some() {
        "answered"
      } else {
        "unanswered"
      },
    );
    answer
  }

  async fn ask_file_info(&self, agent: &str, filename: &str) -> Option<(bool, u64)> {
    if !self.ready(agent) {
      debug!(agent, "Agent is not registered");
      return None;
    }
    let id = Uuid::new_v4();
    let (answer, answered) = oneshot::channel();
    lock(&self.registry).file_infos.insert(
      id,
      FileInfoWait {
        agent: agent.to_string(),
        answer,
      },
    );
    let asked = self.send(
      agent,
      frames::encode(
        Some(id),
        RelayMessage::FileInfoRequest {
          filename: filename.to_string(),
        },
      ),
    );
    debug!(agent, filename, %id, "Requested file information from Agent. Waiting for response.");
    let limit = self
      .config
      .current()
      .values
      .relay
      .controller_timeouts
      .file_info;
    if !asked {
      lock(&self.registry).file_infos.remove(&id);
      return None;
    }
    let answer = timeout(limit, answered).await;
    lock(&self.registry).file_infos.remove(&id);
    match answer {
      Ok(Ok(answer)) => Some(answer),
      Ok(Err(_)) => {
        debug!(
          agent,
          filename, "the agent left before it said whether it has the file"
        );
        None
      }
      Err(_) => {
        warn!(
          agent,
          filename,
          ?limit,
          "Timed out waiting for file information from agent"
        );
        None
      }
    }
  }

  /// Takes the agent `agent`'s answer `id` to a file info request; false when nobody waits
  /// for it.
  pub(super) fn file_info_arrived(&self, agent: &str, id: Uuid, exists: bool, length: u64) -> bool {
    let mut registry = lock(&self.registry);
    if !registry
      .file_infos
      .get(&id)
      .is_some_and(|w| w.agent == agent)
    {
      return false;
    }
    let wait = registry
      .file_infos
      .remove(&id)
      .expect("the wait was just found");
    if wait.answer.send((exists, length)).is_err() {
      debug!(agent, %id, "file info arrived after its asker gave up");
    }
    true
  }
}
