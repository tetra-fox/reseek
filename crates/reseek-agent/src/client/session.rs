//! One socket to the controller, from its challenge to its close. The agent answers the
//! challenge with its credential and must hear back within ten seconds; logged in, it uploads
//! its shares and serves what the controller asks, each request in a task of its own so the
//! socket is never held up by a file. Both sides ping every 15 seconds and the socket closes
//! after 30 seconds without a frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use api_types::relay::{LoginFailure, RelayErrorCode, RelayMessage};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info, trace, warn};

use reseek_relay_proto::credential::Purpose;
use reseek_relay_proto::frames::{
  self, DEFER_EVERY, LOGIN_WITHIN, MAX_FRAME, PING_EVERY, SILENCE, close,
};
use reseek_shares::{Shares, lock};

use super::dial::{Settings, Socket};
use super::{AgentClient, warn_once};

/// How many frames wait to be written to the socket.
const OUTBOUND: usize = 64;

/// The socket now open, as the tasks serving the controller's requests reach it.
pub(super) struct Link {
  /// frames for the socket
  outbound: mpsc::Sender<String>,
  /// closes the socket with the code it is canceled with
  closing: CancellationToken,
  close_code: std::sync::atomic::AtomicU16,
  /// what it connected with
  pub settings: Arc<Settings>,
  logged_in: AtomicBool,
  /// its first share upload succeeded
  ready: AtomicBool,
  /// the share uploads that failed in a row since the last that succeeded
  sync_failures: AtomicU32,
  /// waits for the controller's answer to `shares_begin`
  share_answer: std::sync::Mutex<Option<oneshot::Sender<RelayMessage>>>,
  /// why the controller refused this agent, when it did
  refusal: std::sync::Mutex<Option<String>>,
}

impl Link {
  /// Queues `message` for the socket, with `id` when it answers a request; false when the
  /// socket is gone or its queue is full.
  pub fn send(&self, id: Option<uuid::Uuid>, message: RelayMessage) -> bool {
    let kind = frames::kind(&message);
    match self.outbound.try_send(frames::encode(id, message)) {
      Ok(()) => true,
      Err(e) => {
        debug!(kind, error = %e, "frame for the controller not queued");
        false
      }
    }
  }

  /// Closes the socket with `code`.
  pub fn close(&self, code: u16) {
    self.close_code.store(code, Ordering::Relaxed);
    self.closing.cancel();
  }

  /// Whether the controller accepted the login.
  pub fn logged_in(&self) -> bool {
    self.logged_in.load(Ordering::Acquire)
  }

  /// Whether a share upload succeeded on this socket.
  pub fn ready(&self) -> bool {
    self.ready.load(Ordering::Acquire)
  }

  /// Marks the shares uploaded, the failures before it forgotten.
  pub fn synced(&self) {
    self.ready.store(true, Ordering::Release);
    self.sync_failures.store(0, Ordering::Release);
  }

  /// Counts a share upload that failed after one succeeded; how many failed in a row.
  pub fn sync_failed(&self) -> u32 {
    self.sync_failures.fetch_add(1, Ordering::AcqRel) + 1
  }

  /// How many share uploads failed in a row since the last that succeeded.
  pub fn sync_failures(&self) -> u32 {
    self.sync_failures.load(Ordering::Acquire)
  }

  /// Returns once the socket is closed or closing.
  pub async fn closed(&self) {
    self.closing.cancelled().await;
  }

  /// Records that the controller refused this agent for `reason`, which the reconnect backs
  /// off for.
  pub fn refused(&self, reason: String) {
    *lock(&self.refusal) = Some(reason);
  }

  /// Asks the controller to take the shares with `message` and waits for its answer, `None`
  /// when none comes in `wait`.
  pub async fn ask_shares(
    &self,
    message: RelayMessage,
    wait: std::time::Duration,
  ) -> Option<RelayMessage> {
    let (answer, answered) = oneshot::channel();
    *lock(&self.share_answer) = Some(answer);
    if !self.send(None, message) {
      return None;
    }
    tokio::time::timeout(wait, answered).await.ok()?.ok()
  }
}

#[cfg(test)]
impl Link {
  /// A socket logged in with `settings`, its frames queued on the receiver, with a share upload
  /// done on it when `ready`.
  pub(super) fn opened(
    settings: Arc<Settings>,
    ready: bool,
  ) -> (Arc<Self>, mpsc::Receiver<String>) {
    let (outbound, queued) = mpsc::channel(OUTBOUND);
    let link = Self {
      outbound,
      closing: CancellationToken::new(),
      close_code: std::sync::atomic::AtomicU16::new(close::NORMAL),
      settings,
      logged_in: AtomicBool::new(true),
      ready: AtomicBool::new(ready),
      sync_failures: AtomicU32::new(0),
      share_answer: std::sync::Mutex::new(None),
      refusal: std::sync::Mutex::new(None),
    };
    (Arc::new(link), queued)
  }

  /// Why the controller refused this agent, when it did.
  pub(super) fn refusal(&self) -> Option<String> {
    lock(&self.refusal).clone()
  }

  /// Whether the socket was closed.
  pub(super) fn is_closed(&self) -> bool {
    self.closing.is_cancelled()
  }
}

/// How a socket ended.
pub(super) struct Ended {
  /// the shares were uploaded on it
  pub ready: bool,
  /// the close code the controller sent
  pub code: Option<u16>,
  /// why the controller refused this agent, when it did
  pub refusal: Option<String>,
}

impl AgentClient {
  /// Serves `socket` until it closes.
  pub(super) async fn serve(
    self: &Arc<Self>,
    mut socket: Socket,
    settings: Arc<Settings>,
  ) -> Ended {
    let (outbound, mut queued) = mpsc::channel(OUTBOUND);
    let link = Arc::new(Link {
      outbound,
      closing: self.shutdown.child_token(),
      close_code: std::sync::atomic::AtomicU16::new(close::NORMAL),
      settings,
      logged_in: AtomicBool::new(false),
      ready: AtomicBool::new(false),
      sync_failures: AtomicU32::new(0),
      share_answer: std::sync::Mutex::new(None),
      refusal: std::sync::Mutex::new(None),
    });
    *lock(&self.link) = Some(link.clone());
    let opened = Instant::now();
    let mut heard = opened;
    let mut ping = tokio::time::interval_at(opened + PING_EVERY, PING_EVERY);
    let mut defer = tokio::time::interval_at(opened + DEFER_EVERY, DEFER_EVERY);
    let mut code = None;
    loop {
      let deadline = if link.logged_in() {
        heard + SILENCE
      } else {
        (opened + LOGIN_WITHIN).min(heard + SILENCE)
      };
      tokio::select! {
        () = link.closing.cancelled() => {
          let sent = link.close_code.load(Ordering::Relaxed);
          send_close(&mut socket, sent).await;
          break;
        }
        () = sleep_until(deadline) => {
          warn!(logged_in = link.logged_in(), "the relay controller went silent");
          send_close(&mut socket, close::TIMEOUT).await;
          break;
        }
        Some(text) = queued.recv() => {
          if let Err(e) = socket.send(Message::Text(text.into())).await {
            debug!(error = %e, "frame not written to the controller");
            break;
          }
        }
        _ = defer.tick() => self.defer_while_scanning(),
        _ = ping.tick() => {
          if socket.send(Message::Ping(Default::default())).await.is_err() {
            break;
          }
        }
        frame = socket.next() => {
          let frame = match frame {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
              debug!(error = %e, "relay socket failed");
              break;
            }
            None => break,
          };
          heard = Instant::now();
          match frame {
            Message::Text(text) if text.len() > MAX_FRAME => {
              warn!(bytes = text.len(), "controller sent a frame larger than allowed");
              send_close(&mut socket, close::TOO_BIG).await;
              break;
            }
            Message::Text(text) => self.handle(&link, text.as_str()),
            Message::Close(frame) => {
              code = frame.map(|f| u16::from(f.code));
              break;
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            Message::Binary(_) => debug!("binary frame from the controller ignored"),
          }
        }
      }
    }
    {
      let mut current = lock(&self.link);
      if current.as_ref().is_some_and(|l| Arc::ptr_eq(l, &link)) {
        *current = None;
      }
    }
    // the tasks still holding the link see it closed
    link.closing.cancel();
    let refusal = lock(&link.refusal).take();
    Ended {
      ready: link.ready(),
      code,
      refusal,
    }
  }

  /// Handles a text frame from the controller.
  pub(super) fn handle(self: &Arc<Self>, link: &Arc<Link>, text: &str) {
    let envelope = match frames::decode(text) {
      Ok(envelope) => envelope,
      Err(unsupported) => {
        debug!(
          detail = unsupported.detail,
          "unsupported frame from the controller"
        );
        if let Err(e) = link.outbound.try_send(unsupported.answer()) {
          debug!(error = %e, "answer to an unsupported frame not queued");
        }
        return;
      }
    };
    let id = envelope.id;
    trace!(
      kind = frames::kind(&envelope.message),
      ?id,
      "frame from the controller"
    );
    match envelope.message {
      RelayMessage::Challenge { token, .. } => {
        info!("Relay controller sent an authentication challenge");
        info!("Logging in...");
        let credential = link.settings.key.credential(Purpose::Login, &token);
        link.send(
          None,
          RelayMessage::Login {
            agent: link.settings.agent.clone(),
            credential,
            version: crate::VERSION.to_string(),
            shares_schema: Some(Shares::schema_version()),
            capabilities: Vec::new(),
          },
        );
      }
      RelayMessage::LoginOk { server_version, .. } => {
        info!(controller_version = server_version, "Login succeeded.");
        link.logged_in.store(true, Ordering::Release);
        self.request_sync();
      }
      RelayMessage::LoginFailed { reason } => {
        let configuration = matches!(
          reason,
          LoginFailure::UnknownAgent
            | LoginFailure::AddressNotAllowed
            | LoginFailure::BadCredential
            | LoginFailure::IncompatibleVersion
            | LoginFailure::SchemaMismatch
        );
        // a refusal of the configuration is warned about as the socket ends, an expired
        // challenge here; each once until a socket is ready
        let refusal = format!("login refused: {reason:?}");
        if configuration {
          debug!(?reason, "the relay controller refused the login");
          link.refused(refusal);
        } else {
          warn_once(
            &self.warned,
            &refusal,
            "Failed to handle authentication challenge: the login was refused",
          );
        }
      }
      message @ (RelayMessage::SharesToken { .. }
      | RelayMessage::Error {
        code:
          RelayErrorCode::SchemaMismatch | RelayErrorCode::SyncInProgress | RelayErrorCode::TooLarge,
        ..
      }) => match lock(&link.share_answer).take() {
        // the upload stopped waiting when its receiver is gone
        Some(answer) => drop(answer.send(message)),
        None => debug!(
          kind = frames::kind(&message),
          "share answer nobody waits for"
        ),
      },
      RelayMessage::SharesReady {
        file_count,
        directory_count,
      } => debug!(
        files = file_count,
        directories = directory_count,
        "controller loaded the shares"
      ),
      RelayMessage::FileInfoRequest { filename } => match id {
        Some(id) => {
          let (agent, link) = (self.clone(), link.clone());
          tokio::spawn(
            async move { agent.answer_file_info(&link, id, filename).await }.in_current_span(),
          );
        }
        None => debug!("file info request without an id ignored"),
      },
      RelayMessage::FileRequest {
        filename,
        start_offset,
        transfer_id,
        token,
      } => {
        let agent = self.clone();
        tokio::spawn(
          async move {
            agent
              .send_file(token, filename, start_offset, transfer_id)
              .await
          }
          .in_current_span(),
        );
      }
      RelayMessage::DownloadAvailable {
        filename, token, ..
      } => {
        let agent = self.clone();
        tokio::spawn(async move { agent.fetch_download(filename, token).await }.in_current_span());
      }
      RelayMessage::Error { code, detail, id } => {
        debug!(?code, detail, ?id, "controller reported an error");
      }
      RelayMessage::Ping {} => {
        link.send(id, RelayMessage::Pong {});
      }
      RelayMessage::Pong {} => {}
      other => {
        debug!(
          kind = frames::kind(&other),
          "unexpected frame from the controller"
        );
        link.send(
          None,
          RelayMessage::Error {
            code: RelayErrorCode::Unsupported,
            detail: Some(format!(
              "{} is not sent by a controller",
              frames::kind(&other)
            )),
            id,
          },
        );
      }
    }
  }
}

/// Sends the close frame with `code`.
async fn send_close(socket: &mut Socket, code: u16) {
  let frame = CloseFrame {
    code: CloseCode::from(code),
    reason: "".into(),
  };
  if let Err(e) = socket.send(Message::Close(Some(frame))).await {
    debug!(error = %e, "close frame not written to the controller");
  }
}
