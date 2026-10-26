//! What the daemon does with each client event: the server connection's state into
//! `State.server` and `State.user`, starting and stopping the watchdog, what follows a login,
//! the distributed network into `State.distributedNetwork` and its metrics, and the log lines
//! of the rest. Events whose engine is another part of the daemon are handed on where that
//! engine takes them.

use api_types::events::{Event as BusEvent, EventError, NoFields, SoulseekClientDisconnected};
use api_types::state::{DistributedNetworkState, ServerConnectionState, ServerState};
use soulseek_client::{
  BrowseProgress, ClientError, ClientState, DistributedNetworkInfo, Event, LogLimiter,
  PrivateMessage, Transfer, TransferState,
};
use soulseek_proto::RoomType;
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
    Event::LoggedIn => logged_in(app),
    Event::Disconnected { message, error } => {
      disconnected(app, &message, error.as_ref(), *reached);
      *reached = false;
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
    Event::PrivateMessageReceived(message) => private_message(app, message),
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
    Event::RoomJoined {
      room_name,
      username,
      ..
    } => {
      if app.network.client().username().as_deref() == Some(username.as_str()) {
        app.state.update(|state| {
          if !state.rooms.contains(&room_name) {
            state.rooms.push(room_name);
          }
        });
      }
    }
    Event::RoomLeft {
      room_name,
      username,
    } => {
      if app.network.client().username().as_deref() == Some(username.as_str()) {
        app
          .state
          .update(|state| state.rooms.retain(|room| *room != room_name));
      }
    }
    Event::PrivateRoomMembershipAdded(room) => info!(room, "Added to private room"),
    Event::PrivateRoomMembershipRemoved(room) => info!(room, "Removed from private room"),
    Event::PrivateRoomModerationAdded(room) => {
      info!(room, "Promoted to moderator in private room");
    }
    Event::PrivateRoomModerationRemoved(room) => {
      info!(room, "Demoted from moderator in private room");
    }
    Event::BrowseProgressUpdated(progress) => browse_progress(&progress),
    Event::GlobalMessageReceived(message) => server_message(&message),
    Event::UserStatusChanged(status) => {
      trace!(username = status.username, "user status changed");
      app
        .users
        .status_received(&status.username, status.is_privileged);
      if app.users.is_watched(&status.username) {
        app.users.refetch_statistics(&status.username);
      }
    }
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
    | Event::RoomMessageReceived { .. }
    | Event::PublicChatMessageReceived { .. }
    | Event::RoomTickerListReceived { .. }
    | Event::RoomTickerAdded { .. }
    | Event::RoomTickerRemoved { .. }
    | Event::PrivateRoomUserAdded { .. }
    | Event::PrivateRoomUserRemoved { .. }
    | Event::PrivateRoomOperatorAdded { .. }
    | Event::PrivateRoomOperatorRemoved { .. }
    | Event::PrivateRoomUserListReceived(_)
    | Event::PrivateRoomModeratedUserListReceived(_)
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
    | Event::SearchStateChanged { .. }
    | Event::SearchResponseReceived { .. } => {}
  }
}

/// `State.server` and our username as the client has them in `state`.
fn server_state(app: &AppState, state: ClientState) {
  let client = app.network.client();
  let disconnected = state.contains(ClientState::DISCONNECTED);
  let server = ServerState {
    address: if disconnected { None } else { client.address() },
    ip_end_point: if disconnected {
      None
    } else {
      client.ip_endpoint().map(|e| e.to_string())
    },
    state: ServerConnectionState::from_bits_retain(state.bits().into()),
    is_connected: state.contains(ClientState::CONNECTED),
    is_connecting: state.contains(ClientState::CONNECTING),
    is_logged_in: state.contains(ClientState::LOGGED_IN),
    is_logging_in: state.contains(ClientState::LOGGING_IN),
    is_transitioning: state
      .intersects(ClientState::CONNECTING | ClientState::DISCONNECTING | ClientState::LOGGING_IN),
  };
  let username = client.username();
  app.state.update(|current| {
    current.server = server;
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
fn logged_in(app: &AppState) {
  let username = app.network.client().username();
  info!(username, "Logged in to the Soulseek server");
  app.network.watchdog().logged_in();
  tokio::spawn(after_login(app.clone()).in_current_span());
}

/// Once the shares are initialized: the server is told what we share, our statistics are
/// fetched, the configured rooms are joined, the users with queued uploads and the members of
/// the user-defined groups are watched, and, after the first login, the downloads a stop
/// interrupted are enqueued again.
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
  let rooms = app.config.current().values.rooms.clone();
  for room in rooms {
    match client.join_room(&room, RoomType::Public).await {
      Ok(_) => info!(room, "room joined"),
      Err(e) => warn!(room, error = %e, "room not joined"),
    }
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
  app.state.update(|state| state.rooms.clear());
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
/// stops replaying it, and dropped.
fn private_message(app: &AppState, message: PrivateMessage) {
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
  // unacknowledged, the server replays it at the next login until conversations keep it
  debug!(
    username = message.username,
    replayed = message.replayed,
    "private message not kept; conversations are not kept yet"
  );
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

/// Browse progress, which the browse tracker will push to the UI.
fn browse_progress(progress: &BrowseProgress) {
  trace!(?progress, "browse progress");
}

/// A message from the server's administrators to every user, which the messaging engine will
/// push to the UI.
fn server_message(message: &str) {
  info!(message, "Server message");
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
