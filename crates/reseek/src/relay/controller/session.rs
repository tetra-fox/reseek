//! One agent's socket on the controller, from the challenge to its close. The socket is sent a
//! challenge as it opens and must log in within the challenge's life; logged in, the agent is
//! registered and has two minutes to start sending its shares; an agent whose shares are being
//! scanned renews that by saying so, up to the configured deferral in all. Both sides ping every
//! 15 seconds and the socket closes after 30 seconds without a frame. One task owns the socket:
//! it reads the agent's frames, writes what the controller queues for it, and keeps the timers.

use std::net::IpAddr;
use std::sync::Arc;

use api_types::relay::{
  LoginFailure, RELAY_PROTOCOL, RelayAgentState, RelayErrorCode, RelayMessage, RelayTimeouts,
};
use axum::extract::ws::{CloseFrame, Message};
use chrono::Utc;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};
use tracing::{Instrument, debug, info, info_span, trace, warn};

use super::login::{self, Challenge};
use super::{Controller, OUTBOUND, Outbound};
use crate::crypto::{base62, random_bytes};
use crate::relay::frames::{
  self, LOGIN_WITHIN, MAX_FRAME, PING_EVERY, SILENCE, SYNC_WITHIN, close, millis,
};

/// Where a socket is in its life.
enum Phase {
  /// sent its challenge, which is `None` once a login used it
  Challenged(Option<Challenge>),
  /// logged in as `name`; `sync_by` is when it must have begun sending its shares, `None`
  /// once it has, which a deferral moves up to `deferred_until` at the latest
  LoggedIn {
    name: String,
    sync_by: Option<Instant>,
    deferred_until: Instant,
  },
}

/// The socket an agent is served on: its frames in, frames out.
pub(crate) trait AgentSocket:
  Stream<Item = Result<Message, axum::Error>> + Sink<Message, Error = axum::Error> + Unpin + Send
{
}

impl<T> AgentSocket for T where
  T:
    Stream<Item = Result<Message, axum::Error>> + Sink<Message, Error = axum::Error> + Unpin + Send
{
}

/// When an agent that is deferring its share upload at `now` must have begun it: two minutes on,
/// but no later than the deferral allows.
fn deferred(now: Instant, until: Instant) -> Instant {
  (now + SYNC_WITHIN).min(until)
}

/// How a socket ended.
struct Ended {
  /// the close code sent, `None` when the agent closed or the socket failed
  code: Option<u16>,
  reason: &'static str,
}

impl Controller {
  /// Serves the agent socket `socket`, opened from `address`, until it closes.
  pub(crate) async fn serve_socket(self: Arc<Self>, socket: impl AgentSocket, address: IpAddr) {
    let sessions = self.sessions.clone();
    sessions.track_future(self.serve(socket, address)).await;
  }

  async fn serve(self: Arc<Self>, socket: impl AgentSocket, address: IpAddr) {
    let id = self.next_socket();
    let span = info_span!("relay_agent_socket", socket = id, address = %address);
    async move {
      info!("Agent connection established. Sending authentication challenge");
      let (outbound, queued) = mpsc::channel(OUTBOUND);
      let mut session = Session {
        controller: self.clone(),
        id,
        address,
        outbound,
        phase: Phase::Challenged(None),
      };
      let ended = session.run(socket, queued).await;
      if let Phase::LoggedIn { name, .. } = &session.phase {
        self.deregister(name, id, ended.reason);
      }
      debug!(
        code = ended.code,
        reason = ended.reason,
        "agent socket closed"
      );
    }
    .instrument(span)
    .await;
  }
}

struct Session {
  controller: Arc<Controller>,
  id: u64,
  address: IpAddr,
  /// a sender of the socket's queue, handed to the registration
  outbound: mpsc::Sender<Outbound>,
  phase: Phase,
}

impl Session {
  async fn run(
    &mut self,
    mut socket: impl AgentSocket,
    mut queued: mpsc::Receiver<Outbound>,
  ) -> Ended {
    let token = base62(&random_bytes(32));
    let opened = Instant::now();
    let challenge = frames::encode(
      None,
      RelayMessage::Challenge {
        token: token.clone(),
        expires_in: millis(LOGIN_WITHIN),
        protocol: RELAY_PROTOCOL,
      },
    );
    self.phase = Phase::Challenged(Some(Challenge {
      token,
      issued: opened,
    }));
    if socket.send(Message::Text(challenge.into())).await.is_err() {
      return Ended {
        code: None,
        reason: "closed",
      };
    }
    let mut heard = opened;
    let mut ping = tokio::time::interval_at(opened + PING_EVERY, PING_EVERY);
    loop {
      let (deadline, late) = self.deadline(opened, heard);
      tokio::select! {
        () = self.controller.shutdown.cancelled() => {
          return closing(&mut socket, close::SHUTTING_DOWN, "shutting down").await;
        }
        () = sleep_until(deadline) => {
          warn!(reason = late, "agent socket timed out");
          return closing(&mut socket, close::TIMEOUT, late).await;
        }
        Some(out) = queued.recv() => match out {
          Outbound::Frame(text) => {
            if let Err(e) = socket.send(Message::Text(text.into())).await {
              debug!(error = %e, "frame not written to the agent's socket");
              return Ended { code: None, reason: "closed" };
            }
          }
          Outbound::Close(code, reason) => return closing(&mut socket, code, reason).await,
        },
        _ = ping.tick() => {
          if socket.send(Message::Ping(Default::default())).await.is_err() {
            return Ended { code: None, reason: "closed" };
          }
        }
        frame = socket.next() => {
          let frame = match frame {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
              debug!(error = %e, "agent socket failed");
              return Ended { code: None, reason: "closed" };
            }
            None => return Ended { code: None, reason: "closed" },
          };
          heard = Instant::now();
          match frame {
            Message::Text(text) if text.len() > MAX_FRAME => {
              warn!(bytes = text.len(), "agent sent a frame larger than allowed");
              return closing(&mut socket, close::TOO_BIG, "frame too big").await;
            }
            Message::Text(text) => {
              if let Some(ended) = self.handle(&mut socket, text.as_str()).await {
                return ended;
              }
            }
            Message::Close(_) => return Ended { code: None, reason: "closed" },
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Binary(_) => debug!("binary frame from an agent ignored"),
          }
        }
      }
    }
  }

  /// When the socket closes unless something arrives, and why it would.
  fn deadline(&self, opened: Instant, heard: Instant) -> (Instant, &'static str) {
    let silent = (heard + SILENCE, "timeout");
    match &self.phase {
      Phase::Challenged(_) => (opened + LOGIN_WITHIN, "login_timeout").min(silent),
      Phase::LoggedIn {
        sync_by: Some(by), ..
      } => (*by, "sync_timeout").min(silent),
      Phase::LoggedIn { sync_by: None, .. } => silent,
    }
  }

  /// Handles a text frame; `Some` when it ended the socket.
  async fn handle(&mut self, socket: &mut impl AgentSocket, text: &str) -> Option<Ended> {
    let envelope = match frames::decode(text) {
      Ok(envelope) => envelope,
      Err(unsupported) => {
        debug!(
          detail = unsupported.detail,
          "unsupported frame from an agent"
        );
        self.reply(unsupported.answer());
        return None;
      }
    };
    trace!(kind = frames::kind(&envelope.message), id = ?envelope.id, "frame from an agent");
    let id = envelope.id;
    match (&mut self.phase, envelope.message) {
      (_, RelayMessage::Ping {}) => self.reply(frames::encode(id, RelayMessage::Pong {})),
      (_, RelayMessage::Pong {}) => {}
      (
        Phase::Challenged(_),
        RelayMessage::Login {
          agent,
          credential,
          version,
          ..
        },
      ) => return self.login(socket, agent, &credential, version).await,
      (
        Phase::LoggedIn {
          name,
          sync_by: Some(by),
          deferred_until,
        },
        RelayMessage::SyncDeferred { reason },
      ) => {
        *by = deferred(Instant::now(), *deferred_until);
        debug!(
          agent = name.as_str(),
          reason, "agent defers sending its shares"
        );
      }
      (Phase::LoggedIn { name, sync_by, .. }, message) => {
        let name = name.clone();
        if matches!(message, RelayMessage::SharesBegin { .. }) {
          *sync_by = None;
        }
        if let Some(reply) = self.controller.answer(&name, self.id, id, message) {
          self.reply(reply);
        }
      }
      (Phase::Challenged(_), message) => {
        debug!(kind = frames::kind(&message), "frame before login ignored");
        self.reply(frames::encode(
          None,
          RelayMessage::Error {
            code: RelayErrorCode::Unsupported,
            detail: Some("log in first".to_string()),
            id,
          },
        ));
      }
    }
    None
  }

  /// Checks a login; registers the agent and answers `login_ok`, or answers `login_failed`
  /// and closes the socket. The challenge is used up either way.
  async fn login(
    &mut self,
    socket: &mut impl AgentSocket,
    name: String,
    credential: &str,
    version: String,
  ) -> Option<Ended> {
    let Phase::Challenged(challenge) = &mut self.phase else {
      return None;
    };
    let challenge = challenge.take();
    let options = self.controller.config.current();
    let configured = Controller::configured(&options.values, &name);
    let checked = login::check(
      configured.as_ref(),
      self.address,
      challenge.as_ref(),
      LOGIN_WITHIN,
      Instant::now(),
      credential,
      |secret| self.controller.key(&name, secret),
    );
    let counted = match checked {
      Ok(()) => "accepted",
      Err(LoginFailure::UnknownAgent) => "unknown_agent",
      Err(LoginFailure::AddressNotAllowed) => "address_not_allowed",
      Err(LoginFailure::ChallengeExpired) => "challenge_expired",
      Err(LoginFailure::BadCredential) => "bad_credential",
    };
    self.controller.count("login", counted);
    if let Err(reason) = checked {
      log_refusal(&name, self.address, reason, configured.map(|a| a.cidr));
      let refusal = frames::encode(None, RelayMessage::LoginFailed { reason });
      if socket.send(Message::Text(refusal.into())).await.is_err() {
        return Some(Ended {
          code: None,
          reason: "closed",
        });
      }
      return Some(closing(socket, close::UNAUTHORIZED, "unauthorized").await);
    }
    let timeouts = &options.values.relay.controller_timeouts;
    let accepted = frames::encode(
      None,
      RelayMessage::LoginOk {
        agent: name.clone(),
        server_version: crate::version::info().current,
        protocol: RELAY_PROTOCOL,
        timeouts: RelayTimeouts {
          file_info: millis(timeouts.file_info),
          file_ack: millis(timeouts.file_ack),
          file_stream: millis(timeouts.file_stream),
        },
      },
    );
    if socket.send(Message::Text(accepted.into())).await.is_err() {
      return Some(Ended {
        code: None,
        reason: "closed",
      });
    }
    let record = RelayAgentState {
      name: name.clone(),
      ip_address: self.address.to_string(),
      connected_at: Utc::now(),
      ready: false,
      shares_updated_at: None,
      version,
    };
    self
      .controller
      .register(self.id, record, self.outbound.clone());
    info!(agent = name, "Agent connection authenticated");
    let now = Instant::now();
    self.phase = Phase::LoggedIn {
      name,
      sync_by: Some(now + SYNC_WITHIN),
      deferred_until: now + SYNC_WITHIN + timeouts.sync_deferral,
    };
    None
  }

  /// Queues `frame` for this socket; a full queue drops it, which the socket's own end
  /// follows.
  fn reply(&self, frame: String) {
    if let Err(e) = self.outbound.try_send(Outbound::Frame(frame)) {
      warn!(error = %e, "reply to an agent not queued");
    }
  }
}

/// Logs a refused login: an unknown agent, one outside its networks and a bad credential are
/// warnings, never with the credential.
fn log_refusal(name: &str, address: IpAddr, reason: LoginFailure, cidr: Option<String>) {
  match reason {
    LoginFailure::UnknownAgent => warn!(
      agent = name,
      "Unauthorized login attempt from unknown Agent"
    ),
    LoginFailure::AddressNotAllowed => warn!(
      agent = name,
      address = %address,
      cidr,
      "Unauthorized login attempt: the remote IP address is not within the configured range"
    ),
    LoginFailure::ChallengeExpired => {
      warn!(
        agent = name,
        "Unauthorized login attempt: no challenge token cached for the socket"
      )
    }
    LoginFailure::BadCredential => {
      warn!(
        agent = name,
        "Unauthorized login attempt: credential mismatch"
      )
    }
  }
}

/// Sends the close frame with `code` and ends the socket.
async fn closing(socket: &mut impl AgentSocket, code: u16, reason: &'static str) -> Ended {
  let frame = CloseFrame {
    code,
    reason: reason.into(),
  };
  if let Err(e) = socket.send(Message::Close(Some(frame))).await {
    debug!(error = %e, "close frame not written to the agent's socket");
  }
  Ended {
    code: Some(code),
    reason,
  }
}

impl Controller {
  /// Handles a frame from the logged-in agent `name` on `socket` with `id`; the reply, when it
  /// gets one.
  fn answer(
    &self,
    name: &str,
    socket: u64,
    id: Option<uuid::Uuid>,
    message: RelayMessage,
  ) -> Option<String> {
    let unexpected = |detail: &str| {
      Some(frames::encode(
        None,
        RelayMessage::Error {
          code: RelayErrorCode::NoPendingRequest,
          detail: Some(detail.to_string()),
          id,
        },
      ))
    };
    match message {
      RelayMessage::SharesBegin {
        schema_version,
        database_bytes,
        ..
      } => Some(self.begin_shares(name, socket, schema_version, database_bytes)),
      RelayMessage::FileInfo { exists, length } => match id {
        Some(id) if self.file_info_arrived(name, id, exists, length) => None,
        _ => unexpected("A file info response from this agent with this id was not expected"),
      },
      RelayMessage::FileAck {} => match id {
        Some(id) if self.stream_acknowledged(name, id) => None,
        _ => unexpected("A file acknowledgment with this id was not expected"),
      },
      RelayMessage::FileFailed {
        reason,
        status,
        detail,
      } => match id {
        Some(id) if self.stream_failed(name, id, reason, status, detail) => None,
        _ => unexpected("A file failure with this id was not expected"),
      },
      RelayMessage::DownloadDone { outcome, detail } => {
        info!(
          agent = name,
          ?id,
          ?outcome,
          detail,
          "agent's download of a finished file ended"
        );
        None
      }
      RelayMessage::SyncDeferred { reason } => {
        debug!(
          agent = name,
          reason, "agent defers sending its shares after it began"
        );
        None
      }
      RelayMessage::RescanRequested { reason } => {
        info!(
          agent = name,
          reason, "agent asked for a scan of its own shares"
        );
        None
      }
      RelayMessage::Error { code, detail, id } => {
        debug!(agent = name, ?code, detail, ?id, "agent reported an error");
        None
      }
      other => {
        debug!(
          agent = name,
          kind = frames::kind(&other),
          "unexpected frame from an agent"
        );
        Some(frames::encode(
          None,
          RelayMessage::Error {
            code: RelayErrorCode::Unsupported,
            detail: Some(format!("{} is not sent by an agent", frames::kind(&other))),
            id,
          },
        ))
      }
    }
  }
}
