//! What the daemon does with each client event: the server connection's state into
//! `State.server` and `State.user`, starting and stopping the watchdog, what follows a login,
//! the distributed network into `State.distributedNetwork` and its metrics, the server's
//! messages to everyone, the presence of watched users, and the log lines of the rest. Events
//! about messages and rooms, the login and the disconnect go on to the messaging engine in the
//! order they came; events whose engine is another part of the daemon are handed on where that
//! engine takes them.

use api_types::activity::{ActivityKind, PrivateRoomChange};
use api_types::events::{Event as BusEvent, EventError, NoFields, SoulseekClientDisconnected};
use api_types::live::LiveFrame;
use api_types::messaging::ServerMessage;
use api_types::state::{DistributedNetworkState, ServerConnectionState};
use api_types::transfers::TransferStates;
use api_types::users::{UserPresence, UserStatus, UserStatusChanged};
use chrono::Utc;
use soulseek_client::{
  ClientError, ClientState, DistributedNetworkInfo, Event, LogLimiter, PrivateMessage, SessionEnd,
  SessionStatus, Transfer, TransferState,
};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, trace, warn};

use super::Intent;
use crate::app::AppState;

static LAGGED: LogLimiter = LogLimiter::new();

/// How far a server session got, in the order its edges come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
  /// begun, not yet at the server
  Attempting,
  /// its TCP connection is up
  Connected,
  /// the server accepted its login
  LoggedIn,
  /// it ended
  Ended,
}

/// The server session the handled events are about and how far it got. Sessions are numbered
/// by the client, so each edge of each session is handled once, whether its event came or it
/// was taken for missed first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Session {
  number: u64,
  phase: Phase,
}

impl Default for Session {
  /// before the first session: nothing to end
  fn default() -> Self {
    Self {
      number: 0,
      phase: Phase::Ended,
    }
  }
}

/// An edge of a session that was missed, to be handled as if its event had come.
#[derive(Debug)]
enum Missed {
  /// the session ended as `end` says; `reached` when it had reached the server
  Ended { end: SessionEnd, reached: bool },
  /// the handled session ended and a newer one is under way; `end` is the latest end before
  /// the newer session, the handled one's or one after it
  Superseded(SessionEnd),
  /// the newest session's TCP connection came up
  Connected,
  /// the newest session logged in as `username`
  LoggedIn { username: String },
}

impl Session {
  /// Moves on to `phase` of session `number`, returning the phase left (`Attempting` for a new
  /// session), or `None` when that edge was handled already.
  fn advance(&mut self, number: u64, phase: Phase) -> Option<Phase> {
    let next = Self { number, phase };
    if next <= *self {
      return None;
    }
    let left = if number == self.number {
      self.phase
    } else {
      Phase::Attempting
    };
    *self = next;
    Some(left)
  }

  /// Moves on to where the client's session stands in `status`, returning the edges skipped on
  /// the way, in order. Several sessions that came and went unseen count as one end.
  fn catch_up(&mut self, status: &SessionStatus) -> Vec<Missed> {
    // the current session's own end, once it has one
    let ended = status
      .last_end
      .as_ref()
      .filter(|end| end.session == status.number);
    let phase = if ended.is_some() {
      Phase::Ended
    } else if status.state.contains(ClientState::LOGGED_IN) {
      Phase::LoggedIn
    } else if status.reached {
      Phase::Connected
    } else {
      Phase::Attempting
    };
    let handled = *self;
    let mut missed = Vec::new();
    if self.advance(status.number, phase).is_none() {
      return missed;
    }
    if let Some(end) = ended {
      missed.push(Missed::Ended {
        end: end.clone(),
        reached: status.reached,
      });
      return missed;
    }
    // a newer session began, so the handled one ended and an end is kept
    if handled.phase != Phase::Ended
      && handled.number < status.number
      && let Some(end) = &status.last_end
    {
      missed.push(Missed::Superseded(end.clone()));
    }
    let from = if handled.number == status.number {
      handled.phase
    } else {
      Phase::Attempting
    };
    if from < Phase::Connected && phase >= Phase::Connected {
      missed.push(Missed::Connected);
    }
    if from < Phase::LoggedIn
      && phase >= Phase::LoggedIn
      && let Some(username) = status.username.clone()
    {
      missed.push(Missed::LoggedIn { username });
    }
    missed
  }
}

/// Handles the client's events in order until `drained` is canceled and every event raised
/// before has been handled.
pub(super) async fn follow(
  app: AppState,
  mut events: broadcast::Receiver<Event>,
  drained: CancellationToken,
) {
  let mut session = Session::default();
  loop {
    let event = tokio::select! {
      biased;
      event = events.recv() => event,
      () = drained.cancelled() => return,
    };
    match event {
      Ok(event) => handle(&app, event, &mut session).await,
      Err(RecvError::Lagged(missed)) => caught_up(&app, missed, &mut session).await,
      Err(RecvError::Closed) => return,
    }
  }
}

/// Events were missed: the client's state is read again, and each edge of its session that
/// nothing handled is handled now, so a lost connection is reconnected and what follows a
/// login is not skipped.
async fn caught_up(app: &AppState, missed: u64, session: &mut Session) {
  if let Some(suppressed) = LAGGED.allow() {
    warn!(
      missed,
      suppressed, "client events missed; the client's state is read again"
    );
  }
  let client = app.network.client();
  let status = client.session();
  server_state(app, status.state);
  distributed_network(app, &client.distributed_network());
  let number = status.number;
  for edge in session.catch_up(&status) {
    match edge {
      Missed::Ended { end, reached } => {
        info!(session = number, "a missed disconnect is handled now");
        ended(app, end, reached).await;
      }
      Missed::Superseded(end) => {
        info!(
          session = end.session,
          "a missed disconnect is handled now; a newer session is under way"
        );
        raise_disconnected(app, &end.message, end.error.as_ref());
        logged_out(app, end).await;
      }
      Missed::Connected => {
        info!(session = number, "a missed connect is handled now");
        connected(app).await;
      }
      Missed::LoggedIn { username } => {
        info!(session = number, username, "a missed login is handled now");
        login(app, number, username).await;
      }
    }
  }
}

/// The login of session `number` as `username`: what follows a login runs, and the messaging
/// engine opens its session.
async fn login(app: &AppState, number: u64, username: String) {
  logged_in(app, &username);
  app.network.mapper.logged_in(true);
  app.network.reachability.logged_in();
  app
    .messaging
    .take(Event::LoggedIn {
      session: number,
      username,
    })
    .await;
}

/// A session ended as `end` says, after reaching the server when `reached`.
async fn ended(app: &AppState, end: SessionEnd, reached: bool) {
  disconnected(app, &end.message, end.error.as_ref(), reached);
  logged_out(app, end).await;
}

/// What a session's end ends besides the connection: the port mapping and reachability stop
/// counting on a login, and the messaging engine closes its session when it ended.
async fn logged_out(app: &AppState, end: SessionEnd) {
  app.network.mapper.logged_in(false);
  app.network.reachability.logged_out();
  app
    .messaging
    .take(Event::Disconnected {
      session: end.session,
      message: end.message,
      error: end.error,
      at: end.at,
    })
    .await;
}

async fn handle(app: &AppState, event: Event, session: &mut Session) {
  match event {
    Event::StateChanged { state, .. } => server_state(app, state),
    Event::Connected { session: number } => {
      if session.advance(number, Phase::Connected).is_some() {
        connected(app).await;
      } else {
        debug!(
          session = number,
          "connect handled already when it was taken for missed"
        );
      }
    }
    Event::LoggedIn {
      session: number,
      username,
    } => {
      if session.advance(number, Phase::LoggedIn).is_some() {
        login(app, number, username).await;
      } else {
        debug!(
          session = number,
          username, "login handled already when it was taken for missed"
        );
      }
    }
    Event::Disconnected {
      session: number,
      message,
      error,
      at,
    } => match session.advance(number, Phase::Ended) {
      Some(left) => {
        let end = SessionEnd {
          session: number,
          message,
          error,
          at,
        };
        ended(app, end, left >= Phase::Connected).await;
      }
      None => debug!(
        session = number,
        "disconnect handled already when it was taken for missed"
      ),
    },
    Event::KickedFromServer => debug!("kicked from the server; the disconnect follows"),
    Event::DistributedNetworkStateChanged(info) => distributed_network(app, &info),
    Event::ExcludedSearchPhrasesReceived(phrases) => {
      debug!(
        count = phrases.len(),
        ?phrases,
        "excluded search phrases received"
      );
      app.network.exclude(phrases);
    }
    Event::PrivateMessageReceived(message) => private_message(app, message).await,
    Event::TransferStateChanged {
      previous_state,
      transfer,
    } => transfer_state(app, previous_state, &transfer),
    Event::DownloadDenied {
      username,
      filename,
      message,
    } => warn!(
      username,
      %filename,
      message,
      "Download was denied by the remote user"
    ),
    Event::PlaceInQueueReceived {
      username,
      filename,
      place,
    } => {
      let downloads = app.downloads.clone();
      tokio::spawn(
        async move {
          downloads
            .place_received(&username, filename.as_str(), place)
            .await;
        }
        .in_current_span(),
      );
    }
    Event::DownloadFailed { username, filename } => warn!(
      username,
      %filename,
      "Download was reported as failed by the remote user"
    ),
    room @ (Event::PrivateRoomMembershipAdded(_)
    | Event::PrivateRoomMembershipRemoved(_)
    | Event::PrivateRoomModerationAdded(_)
    | Event::PrivateRoomModerationRemoved(_)) => {
      private_room(app, &room);
      app.messaging.take(room).await;
    }
    room @ (Event::RoomJoined { .. }
    | Event::RoomLeft { .. }
    | Event::RoomMessageReceived { .. }
    | Event::RoomTickerListReceived { .. }
    | Event::RoomTickerAdded { .. }
    | Event::RoomTickerRemoved { .. }
    | Event::PrivateRoomUserAdded { .. }
    | Event::PrivateRoomUserRemoved { .. }
    | Event::PrivateRoomOperatorAdded { .. }
    | Event::PrivateRoomOperatorRemoved { .. }
    | Event::PrivateRoomUserListReceived(_)
    | Event::PrivateRoomModeratedUserListReceived(_)) => app.messaging.take(room).await,
    Event::GlobalMessageReceived(message) => server_message(app, message),
    Event::PublicChatMessageReceived {
      room_name,
      username,
      message,
    } => app.messaging.public_message(room_name, username, message),
    Event::UserStatusChanged(status) => user_status(app, &status).await,
    Event::UserStatisticsChanged(statistics) => {
      app
        .users
        .statistics_received(&statistics.username, &statistics.stats);
    }
    Event::PrivilegedUserListReceived(usernames) => app.users.privileged_listed(&usernames),
    // the transfer, search, room, user and distributed engines follow these themselves, and the
    // rest change nothing the daemon keeps
    Event::ServerInfoReceived(_)
    | Event::PrivilegeNotificationReceived { .. }
    | Event::UserCannotConnect { .. }
    | Event::RoomListReceived(_)
    | Event::BrowseProgressUpdated(_)
    | Event::DistributedNetworkReset
    | Event::DistributedParentAdopted(_)
    | Event::DistributedParentDisconnected(_)
    | Event::DistributedChildAdded { .. }
    | Event::DistributedChildDisconnected { .. }
    | Event::PromotedToDistributedBranchRoot
    | Event::DemotedFromDistributedBranchRoot
    | Event::SearchRequestReceived { .. }
    | Event::SearchResponseDeliveryFailed(_)
    | Event::TransferProgressUpdated { .. }
    | Event::SearchStateChanged { .. } => {}
    Event::SearchResponseDelivered(delivery) => {
      // the daemon answers with the files themselves, never a frame encoded ahead, so the count
      // is always known
      let results = delivery.search_response.file_count().unwrap_or(0);
      app.activity.record(
        Some(&delivery.username),
        ActivityKind::Search {
          query: delivery.query,
          results: u32::try_from(results).unwrap_or(u32::MAX),
        },
      );
    }
    Event::SearchResponseReceived { search, .. } => {
      app.network.reachability.search_answered(search.token);
    }
  }
}

/// A user's presence, which the server sends for watched users, for room members and in reply
/// to our own requests. A watched user's row is touched with it, and their statistics are
/// asked for again when the server sent it on its own, since it pushes their presence and not
/// what they share; it is pushed as `users.status` for a user we have a conversation or a
/// transfer with.
async fn user_status(app: &AppState, status: &soulseek_proto::server::UserStatus) {
  let username = &status.username;
  let presence = UserPresence::from(status.presence);
  trace!(username, ?presence, "user status changed");
  app.messaging.member_status(username, presence);
  let asked = app
    .users
    .status_received(username, presence, status.is_privileged);
  if !app.users.is_watched(username) {
    return;
  }
  let touch = crate::users::Touch::Status {
    presence,
    is_privileged: status.is_privileged,
  };
  app.users.touch(username, touch).await;
  if !asked {
    app.users.refetch_statistics(username);
  }
  let client = app.network.client();
  let transferring = client
    .uploads()
    .iter()
    .chain(client.downloads().iter())
    .any(|transfer| transfer.username == *username);
  if transferring || app.messaging.is_open(username) {
    app.live.publish(LiveFrame::UsersStatus(UserStatusChanged {
      username: username.clone(),
      status: UserStatus {
        presence,
        is_privileged: status.is_privileged,
      },
    }));
  }
}

/// `State.server` and our username as the client has them in `state`.
fn server_state(app: &AppState, state: ClientState) {
  let client = app.network.client();
  let disconnected = state.contains(ClientState::DISCONNECTED);
  let address = if disconnected { None } else { client.address() };
  let ip_end_point = if disconnected {
    None
  } else {
    client.ip_endpoint().map(|e| e.to_string())
  };
  let username = client.username();
  app.state.update(|current| {
    let server = &mut current.server;
    server.address = address;
    server.ip_end_point = ip_end_point;
    server.state = ServerConnectionState::from_bits_retain(state.bits().into());
    server.is_connected = state.contains(ClientState::CONNECTED);
    server.is_connecting = state.contains(ClientState::CONNECTING);
    server.is_logged_in = state.contains(ClientState::LOGGED_IN);
    server.is_logging_in = state.contains(ClientState::LOGGING_IN);
    server.is_transitioning = state
      .intersects(ClientState::CONNECTING | ClientState::DISCONNECTING | ClientState::LOGGING_IN);
    current.user.username = username;
  });
}

/// The TCP connection to the server is up, before the login: the watchdog stops without
/// canceling the login, what waited for a reconnect is in effect now, and the users the last
/// session watched are watched no more.
async fn connected(app: &AppState) {
  app.network.watchdog().stop(false);
  app.users.session_ended();
  info!("Connected to the Soulseek server");
  app
    .events
    .raise(BusEvent::SoulseekClientConnected(NoFields { version: 0 }));
  if let Err(e) = app.config.reconnected().await {
    warn!(error = %e, "the options in effect were not marked as reconnected");
  }
  app.network.forget_reconnect();
}

/// The server accepted our login: what needs the session runs in the background.
fn logged_in(app: &AppState, username: &str) {
  info!(username, "Logged in to the Soulseek server");
  app.network.watchdog().logged_in();
  tokio::spawn(after_login(app.clone()).in_current_span());
}

/// Once the shares are initialized: the server is told what we share and what we like and hate,
/// our statistics are fetched, the users with queued uploads and the members of the user-defined
/// groups are watched, and, after the first login, the downloads a stop interrupted are enqueued
/// again. The messaging engine joins the rooms.
async fn after_login(app: AppState) {
  app.shares.initialized().await;
  let client = app.network.client();
  let counts = *app.shares.shared_counts().borrow();
  if let Err(e) = client
    .set_shared_counts(counts.directories, counts.files)
    .await
  {
    warn!(error = %e, "Failed to execute post-login actions: shared counts not sent");
  }
  app.users.send_interests(client).await;
  if let Err(e) = app.network.refresh_user_statistics(true).await {
    warn!(error = %e, "Failed to execute post-login actions: statistics not fetched");
  }
  let uploads = app.uploads.clone();
  tokio::spawn(async move { uploads.logged_in().await }.in_current_span());
  app.downloads.resume().await;
}

/// Tells the server what we share whenever that changes while logged in, and fetches our
/// statistics again, which the server counts from it. The login tells it what we share then.
pub async fn follow_shared_counts(app: AppState) {
  let mut counts = app.shares.shared_counts();
  counts.borrow_and_update();
  loop {
    tokio::select! {
      () = app.shutdown.cancelled() => return,
      changed = counts.changed() => if changed.is_err() { return },
    }
    let current = *counts.borrow_and_update();
    let client = app.network.client();
    if client.state() != ClientState::READY {
      continue;
    }
    if let Err(e) = client
      .set_shared_counts(current.directories, current.files)
      .await
    {
      warn!(error = %e, "shared counts not sent");
      continue;
    }
    if let Err(e) = app.network.refresh_user_statistics(true).await {
      warn!(error = %e, "statistics not fetched after the shares changed");
    }
  }
}

/// The server session ended: why is logged, the watchdog starts again after a session that
/// reached the server unless the daemon asked for the end, the server refused our login or
/// another session took our name, and the event is raised.
fn disconnected(app: &AppState, message: &str, error: Option<&ClientError>, reached: bool) {
  app.network.forget_reconnect();
  let intent = error.and_then(Intent::of);
  match error {
    Some(ClientError::Disposed) => {
      info!("Disconnected from the Soulseek server: the client is shutting down");
    }
    _ if intent == Some(Intent::Shutdown) => {
      info!("Disconnected from the Soulseek server: the client is shutting down");
    }
    _ if intent == Some(Intent::User) => {
      info!(
        message,
        "Disconnected from the Soulseek server: disconnected by the user"
      );
    }
    // a restart or a disconnect asked for canceled the attempt
    Some(ClientError::Canceled) => debug!("connection attempt canceled"),
    Some(ClientError::LoginRejected(reason)) => {
      error!(
        reason,
        "Disconnected from the Soulseek server: invalid username or password"
      );
    }
    Some(ClientError::KickedFromServer) => error!(
      "Disconnected from the Soulseek server: another client logged in using the same username"
    ),
    // the watchdog's own attempt failed before reaching the server; its loop goes on
    _ if !reached => debug!(message, "connection attempt failed"),
    _ => {
      warn!(message, "Disconnected from the Soulseek server");
      app.network.watchdog().start();
    }
  }
  raise_disconnected(app, message, error);
}

/// Raises the disconnect on the event bus.
fn raise_disconnected(app: &AppState, message: &str, error: Option<&ClientError>) {
  app.events.raise(BusEvent::SoulseekClientDisconnected(
    SoulseekClientDisconnected {
      version: 0,
      message: message.to_string(),
      exception: error.map(|error| EventError {
        kind: error_kind(error).to_string(),
        message: error.to_string(),
      }),
    },
  ));
}

/// The short name of an error's kind, as the disconnect event carries it.
fn error_kind(error: &ClientError) -> &'static str {
  if let Some(intent) = Intent::of(error) {
    return match intent {
      Intent::User => "IntentionalDisconnect",
      Intent::Shutdown => "ApplicationShutdown",
    };
  }
  match error {
    ClientError::InvalidArgument(_) => "InvalidArgument",
    ClientError::InvalidOperation(_) => "InvalidOperation",
    ClientError::Timeout(_) => "Timeout",
    ClientError::Canceled => "Cancelled",
    ClientError::Address(_) => "Address",
    ClientError::Listen { .. } => "Listen",
    ClientError::Connect { .. } => "Connect",
    ClientError::LoginRejected(_) => "LoginRejected",
    ClientError::LoginNoResponse(_) => "LoginNoResponse",
    ClientError::KickedFromServer => "KickedFromServer",
    ClientError::Connection { .. } => "Connection",
    ClientError::Message { .. } => "Message",
    ClientError::Disposed => "Disposed",
    _ => "Client",
  }
}

/// Our place in the distributed network into `State.distributedNetwork` and its metrics.
fn distributed_network(app: &AppState, info: &DistributedNetworkInfo) {
  let metrics = &app.telemetry.metrics().distributed;
  metrics.children.set(info.children.len() as f64);
  metrics.child_limit.set(f64::from(info.child_limit));
  metrics
    .has_parent
    .set(if info.has_parent { 1.0 } else { 0.0 });
  metrics.branch_level.set(f64::from(info.branch_level));
  let named = |name: &str| (!name.is_empty()).then(|| name.to_string());
  let network = DistributedNetworkState {
    branch_level: i32::try_from(info.branch_level).unwrap_or(i32::MAX),
    branch_root: named(&info.branch_root),
    can_accept_children: info.can_accept_children,
    child_limit: info.child_limit,
    children: info.children.iter().map(|(name, _)| name.clone()).collect(),
    has_parent: info.has_parent,
    is_branch_root: info.is_branch_root,
    parent: named(&info.parent.0),
  };
  app
    .state
    .update(|state| state.distributed_network = network);
}

/// A private message: one from a user blocked from messages is acknowledged so the server
/// stops replaying it, and dropped; the others go to the messaging engine, which keeps them
/// unacknowledged until they are read.
async fn private_message(app: &AppState, message: PrivateMessage) {
  if app.users.is_blocked(
    &message.username,
    None,
    api_types::users::blocking::BlockScope::Messages,
  ) {
    debug!(
      username = message.username,
      "private message from a blocked user dropped"
    );
    let client = app.network.client().clone();
    tokio::spawn(
      async move {
        if let Err(e) = client.acknowledge_private_message(message.id).await {
          debug!(error = %e, "dropped private message not acknowledged");
        }
      }
      .in_current_span(),
    );
    return;
  }
  app
    .messaging
    .take(Event::PrivateMessageReceived(message))
    .await;
}

/// A transfer moved on: logged, its direction's queued and in-progress gauges counted again, and
/// an upload that succeeded reports its speed to the server, which keeps our average speed from
/// it.
fn transfer_state(app: &AppState, previous: TransferState, transfer: &Transfer) {
  let name = transfer.filename.as_str();
  let basename = name.rsplit(['\\', '/']).next().unwrap_or(name);
  let direction = match transfer.direction {
    soulseek_proto::TransferDirection::Download => "Download",
    soulseek_proto::TransferDirection::Upload => "Upload",
  };
  info!(
    direction,
    username = transfer.username,
    file = basename,
    %previous,
    state = %transfer.state,
    bytes = transfer.bytes_transferred,
    size = transfer.size,
    speed = transfer.average_speed,
    "transfer state changed"
  );
  if transfer.direction == soulseek_proto::TransferDirection::Upload {
    upload_activity(app, previous, transfer);
  }
  let client = app.network.client();
  {
    let family = app.telemetry.metrics().transfers();
    match transfer.direction {
      soulseek_proto::TransferDirection::Download => family.downloads.count(&client.downloads()),
      soulseek_proto::TransferDirection::Upload => family.uploads.count(&client.uploads()),
    }
  }
  let succeeded = TransferState::COMPLETED | TransferState::SUCCEEDED;
  if transfer.direction == soulseek_proto::TransferDirection::Upload
    && transfer.state == succeeded
    && transfer.average_speed > 0.0
  {
    // servers read the speed as a signed 32-bit number
    let speed = transfer.average_speed.ceil().min(f64::from(i32::MAX)) as u32;
    let client = client.clone();
    tokio::spawn(
      async move {
        if let Err(e) = client.send_upload_speed(speed).await {
          warn!(error = %e, speed, "Failed to report upload speed");
        }
      }
      .in_current_span(),
    );
  }
}

/// An upload that started sending, or one that ended after it began, into the activity log. An
/// upload that ends while still queued was never under way, so it is not noted.
fn upload_activity(app: &AppState, previous: TransferState, transfer: &Transfer) {
  let begun = TransferState::INITIALIZING | TransferState::IN_PROGRESS;
  let kind =
    if transfer.state == TransferState::IN_PROGRESS && previous != TransferState::IN_PROGRESS {
      ActivityKind::UploadStarted {
        filename: transfer.filename.to_string(),
        size: transfer.size,
      }
    } else if transfer.state.contains(TransferState::COMPLETED) && previous.intersects(begun) {
      ActivityKind::UploadEnded {
        filename: transfer.filename.to_string(),
        state: TransferStates::from_bits_retain(transfer.state.bits()),
        bytes: transfer.bytes_transferred,
      }
    } else {
      return;
    };
  app.activity.record(Some(&transfer.username), kind);
}

/// A change to our own membership of a private room, into the activity log.
fn private_room(app: &AppState, event: &Event) {
  let (room, change) = match event {
    Event::PrivateRoomMembershipAdded(room) => (room, PrivateRoomChange::Added),
    Event::PrivateRoomMembershipRemoved(room) => (room, PrivateRoomChange::Removed),
    Event::PrivateRoomModerationAdded(room) => (room, PrivateRoomChange::Operator),
    Event::PrivateRoomModerationRemoved(room) => (room, PrivateRoomChange::NoLongerOperator),
    _ => return,
  };
  app.activity.record(
    None,
    ActivityKind::PrivateRoom {
      room: room.clone(),
      change,
    },
  );
}

/// A message from the server's administrators to every user: logged and pushed.
fn server_message(app: &AppState, message: String) {
  info!(message, "Server message");
  app
    .live
    .publish(LiveFrame::ApplicationServerMessage(ServerMessage {
      message,
      received_at: Utc::now(),
    }));
}

/// Reads into the metrics what only the client knows: the distributed network's broadcast
/// latency and how many search responses wait for their searcher; run once a minute.
pub fn observe_client(app: &AppState) {
  let client = app.network.client();
  let latency = client
    .distributed_network()
    .average_broadcast_latency
    .unwrap_or(0.0);
  let metrics = app.telemetry.metrics();
  metrics.distributed.broadcast_latency.observe(latency);
  metrics.distributed.broadcast_latency_current.set(latency);
  let kept = client.undelivered_search_responses();
  metrics.search.response_cache_size.set(kept as f64);
  trace!(latency, kept, "client figures observed");
}

/// Counts both directions' queued and in-progress transfers again from the client's, and moves
/// their speed averages; run every five seconds.
pub fn observe_transfers(app: &AppState) {
  let client = app.network.client();
  let (uploads, downloads) = (client.uploads(), client.downloads());
  let family = app.telemetry.metrics().transfers();
  family.uploads.count(&uploads);
  family.uploads.pace(&uploads);
  family.downloads.count(&downloads);
  family.downloads.pace(&downloads);
}

#[cfg(test)]
mod tests {
  use super::*;

  fn end(session: u64) -> SessionEnd {
    SessionEnd {
      session,
      message: "Connection reset".into(),
      error: None,
      at: std::time::SystemTime::now(),
    }
  }

  fn status(number: u64, state: ClientState, reached: bool) -> SessionStatus {
    SessionStatus {
      number,
      state,
      reached,
      username: state.contains(ClientState::LOGGED_IN).then(|| "me".into()),
      last_end: if state.contains(ClientState::DISCONNECTED) {
        Some(end(number))
      } else {
        number.checked_sub(1).filter(|n| *n > 0).map(end)
      },
    }
  }

  fn logged_in(number: u64) -> Session {
    Session {
      number,
      phase: Phase::LoggedIn,
    }
  }

  #[test]
  fn caught_up_disconnect_lost_in_a_lag() {
    let mut session = logged_in(1);
    let missed = session.catch_up(&status(1, ClientState::DISCONNECTED, true));
    assert!(
      matches!(missed.as_slice(), [Missed::Ended { reached: true, .. }]),
      "{missed:?}"
    );
    // the event itself, still queued behind the lag, is not handled twice
    assert_eq!(session.advance(1, Phase::Ended), None);
  }

  #[test]
  fn caught_up_reconnect_and_login_inside_one_lag() {
    let mut session = logged_in(1);
    let missed = session.catch_up(&status(2, ClientState::READY, true));
    assert!(
      matches!(
        missed.as_slice(),
        [
          Missed::Superseded(SessionEnd { session: 1, .. }),
          Missed::Connected,
          Missed::LoggedIn { .. }
        ]
      ),
      "{missed:?}"
    );
    // the old session's queued disconnect and the new one's queued login are handled already
    assert_eq!(session.advance(1, Phase::Ended), None);
    assert_eq!(session.advance(2, Phase::LoggedIn), None);
  }

  #[test]
  fn caught_up_session_that_came_and_went() {
    let mut session = Session::default();
    session.advance(1, Phase::Ended);
    let missed = session.catch_up(&status(2, ClientState::DISCONNECTED, true));
    assert!(
      matches!(missed.as_slice(), [Missed::Ended { reached: true, .. }]),
      "{missed:?}"
    );
  }

  #[test]
  fn caught_up_nothing_missed() {
    let mut session = logged_in(3);
    assert!(
      session
        .catch_up(&status(3, ClientState::READY, true))
        .is_empty()
    );
    // a disconnect under way is not taken for over, and its reach is known when it ends
    let mut session = logged_in(3);
    assert!(
      session
        .catch_up(&status(3, ClientState::DISCONNECTING, true))
        .is_empty()
    );
    assert_eq!(session.advance(3, Phase::Ended), Some(Phase::LoggedIn));
  }
}
