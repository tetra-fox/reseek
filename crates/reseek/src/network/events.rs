//! What the daemon does with each client event: the server connection's state into
//! `State.server` and `State.user`, starting and stopping the watchdog, what follows a login,
//! the distributed network into `State.distributedNetwork` and its metrics, the server's
//! messages to everyone, the presence of watched users, and the log lines of the rest. Events
//! about messages and rooms, the login and the disconnect go on to the messaging engine in the
//! order they came; events whose engine is another part of the daemon are handed on where that
//! engine takes them.

use api_types::chat::ServerMessage;
use api_types::events::{Event as BusEvent, EventError, NoFields, SoulseekClientDisconnected};
use api_types::live::LiveFrame;
use api_types::state::{DistributedNetworkState, ServerConnectionState};
use api_types::users::{UserPresence, UserStatus, UserStatusChanged};
use chrono::Utc;
use soulseek_client::{
  ClientError, ClientState, DistributedNetworkInfo, Event, LogLimiter, PrivateMessage, Transfer,
  TransferState,
};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, trace, warn};

use super::Intent;
use crate::app::AppState;

static LAGGED: LogLimiter = LogLimiter::new();

/// Handles the client's events in order until `drained` is cancelled and every event raised
/// before has been handled.
pub(super) async fn follow(
  app: AppState,
  mut events: broadcast::Receiver<Event>,
  drained: CancellationToken,
) {
  // whether the session that ends next reached the server, so a failed attempt is not
  // reported as a lost connection
  let mut reached = false;
  loop {
    let event = tokio::select! {
      biased;
      event = events.recv() => event,
      () = drained.cancelled() => return,
    };
    match event {
      Ok(event) => handle(&app, event, &mut reached).await,
      Err(RecvError::Lagged(missed)) => {
        if let Some(suppressed) = LAGGED.allow() {
          warn!(
            missed,
            suppressed, "client events missed; the client's state is read again"
          );
        }
        let client = app.network.client();
        let state = client.state();
        // a missed StateChanged may have been the one that reached the server
        reached |= state.contains(ClientState::CONNECTED);
        server_state(&app, state);
        distributed_network(&app, &client.distributed_network());
      }
      Err(RecvError::Closed) => return,
    }
  }
}

async fn handle(app: &AppState, event: Event, reached: &mut bool) {
  match event {
    Event::StateChanged { state, .. } => {
      if state.contains(ClientState::CONNECTED) {
        *reached = true;
      }
      server_state(app, state);
    }
    Event::Connected => connected(app).await,
    Event::LoggedIn { username } => {
      logged_in(app, &username);
      app.network.mapper.logged_in(true);
      app.network.reachability.logged_in();
      app.messaging.take(Event::LoggedIn { username }).await;
    }
    Event::Disconnected { message, error } => {
      disconnected(app, &message, error.as_ref(), *reached);
      app.network.mapper.logged_in(false);
      app.network.reachability.logged_out();
      *reached = false;
      app
        .messaging
        .take(Event::Disconnected { message, error })
        .await;
    }
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
    | Event::PrivateRoomModeratedUserListReceived(_)
    | Event::PrivateRoomMembershipAdded(_)
    | Event::PrivateRoomMembershipRemoved(_)
    | Event::PrivateRoomModerationAdded(_)
    | Event::PrivateRoomModerationRemoved(_)) => app.messaging.take(room).await,
    Event::GlobalMessageReceived(message) => server_message(app, message),
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
    | Event::PublicChatMessageReceived { .. }
    | Event::BrowseProgressUpdated(_)
    | Event::DistributedNetworkReset
    | Event::DistributedParentAdopted(_)
    | Event::DistributedParentDisconnected(_)
    | Event::DistributedChildAdded { .. }
    | Event::DistributedChildDisconnected { .. }
    | Event::PromotedToDistributedBranchRoot
    | Event::DemotedFromDistributedBranchRoot
    | Event::SearchRequestReceived { .. }
    | Event::SearchResponseDelivered(_)
    | Event::SearchResponseDeliveryFailed(_)
    | Event::TransferProgressUpdated { .. }
    | Event::SearchStateChanged { .. } => {}
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
/// cancelling the login, what waited for a reconnect is in effect now, and the users the last
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

/// Once the shares are initialized: the server is told what we share, our statistics are
/// fetched, the users with queued uploads and the members of the user-defined groups are
/// watched, and, after the first login, the downloads a stop interrupted are enqueued again.
/// The messaging engine joins the rooms.
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
    // a restart or a disconnect asked for cancelled the attempt
    Some(ClientError::Cancelled) => debug!("connection attempt cancelled"),
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
    ClientError::Cancelled => "Cancelled",
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
    api_types::blocking::BlockScope::Messages,
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
