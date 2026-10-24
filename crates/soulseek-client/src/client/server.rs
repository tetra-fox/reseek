//! What the client does with each message the server sends: complete the wait of the request it
//! answers, raise the event it carries, or hand it to the component that owns it. Frames are
//! handled in order on the server connection's read loop and nothing here blocks; whatever needs
//! a connection or a lookup is handed on through a bounded queue.
//!
//! The waits each reply completes, and the value it completes them with:
//!
//! | reply | key | value |
//! |---|---|---|
//! | `Login` | `Server(Login)` | `LoginResponse` |
//! | `GetPeerAddress` | `ServerUser` | `GetPeerAddressResponse` |
//! | `WatchUser` | `ServerUser` | `WatchUserResponse` |
//! | `GetStatus` | `ServerUser` | `UserStatus` |
//! | `JoinRoom` | `ServerRoom` | `RoomData` (failed by `CannotJoinRoom`) |
//! | `LeaveRoom` | `ServerRoom` | `()` |
//! | `Ping` | `Server(Ping)` | `()` |
//! | `GetUserStats` | `ServerUser` | `UserStatistics` |
//! | `GetPersonalRecommendations`, `GetGlobalRecommendations` | `Server` | `Recommendations` |
//! | `GetUserInterests` | `ServerUser` | `UserInterests` |
//! | `RoomList` | `Server(RoomList)` | `Arc<RoomList>` |
//! | `CheckPrivileges` | `Server(CheckPrivileges)` | `u32`, seconds |
//! | `GetPersonalSimilarUsers` | `Server` | `GetPersonalSimilarUsersResponse` |
//! | `GetInterestRecommendations` | `ServerInterest` | `GetInterestRecommendationsResponse` |
//! | `GetInterestSimilarUsers` | `ServerInterest` | `GetInterestSimilarUsersResponse` |
//! | `UserPrivileges` | `ServerUser` | `bool` |
//! | `PrivateRoomAddUser`, `PrivateRoomRemoveUser`, `PrivateRoomAddOperator`, `PrivateRoomRemoveOperator` | `ServerRoomUser` | `()` |
//! | `PrivateRoomRemoved`, `PrivateRoomOperatorRemoved` | `ServerRoom` | `()` |
//! | `PrivateRoomToggle` | `Server(PrivateRoomToggle)` | `bool` |
//! | `NewPassword` | `Server(NewPassword)` | `Secret`, the password echoed back |

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use soulseek_proto::distributed::Embedded;
use soulseek_proto::server::{
  AcknowledgeNotifyPrivileges, AcknowledgePrivateMessage, CannotConnectResponse,
  ConnectToPeerResponse, FileSearchResponse, NetInfo,
};
use soulseek_proto::{DecodeError, ServerCode, ServerRequest, ServerResponse};

use super::Client;
use super::disconnect::Cause;
use crate::error::{ClientError, RoomErrorKind};
use crate::events::{Event, PrivateMessage, RoomMembers};
use crate::log::LogLimiter;
use crate::state::ServerInfo;
use crate::waiter::WaitKey;

/// codes the server no longer sends; seeing one is worth telling apart from a new code
const OBSOLETE: [ServerCode; 13] = [
  ServerCode::IgnoreUser,
  ServerCode::UnignoreUser,
  ServerCode::FileSearchRoom,
  ServerCode::SendConnectToken,
  ServerCode::SimilarRecommendations,
  ServerCode::MyRecommendations,
  ServerCode::AdminCommand,
  ServerCode::PlaceInLineRequest,
  ServerCode::PlaceInLineResponse,
  ServerCode::RoomAdded,
  ServerCode::RoomRemoved,
  ServerCode::GlobalUserList,
  ServerCode::TunneledMessage,
];

static HANDOFF_FULL: LogLimiter = LogLimiter::new();

/// A server message whose handling belongs to the peer, distributed or search components
#[derive(Debug)]
pub enum ServerHandoff {
  /// a peer asks us, through the server, to connect to it
  ConnectToPeer(ConnectToPeerResponse),
  /// another user's search, which the server delivers to us directly
  FileSearch(FileSearchResponse),
  /// a distributed message the server sends us as a branch root
  EmbeddedMessage(Embedded),
  /// candidates for our parent in the distributed tree
  NetInfo(NetInfo),
  /// the server tells every client to rebuild the distributed tree
  DistributedReset,
  /// a peer could not act on one of our connection requests; the event for it, when the
  /// server names the peer, has been raised already
  CannotConnect(CannotConnectResponse),
}

impl ServerHandoff {
  fn name(&self) -> &'static str {
    match self {
      Self::ConnectToPeer(_) => "ConnectToPeer",
      Self::FileSearch(_) => "FileSearch",
      Self::EmbeddedMessage(_) => "EmbeddedMessage",
      Self::NetInfo(_) => "NetInfo",
      Self::DistributedReset => "DistributedReset",
      Self::CannotConnect(_) => "CannotConnect",
    }
  }
}

impl Client {
  /// Handles one complete frame from the server. A frame that does not decode has been logged
  /// by the decoder and is dropped; the connection stays up.
  pub(super) fn dispatch_server_frame(&self, frame: &[u8]) {
    let message = match ServerResponse::decode(frame) {
      Ok(message) => message,
      Err(DecodeError::UnknownCode { code }) => return unhandled(code, frame.len()),
      Err(_) => return,
    };
    if self.inner.options().report_unread_message_data {
      let read = message.encode().len();
      if read < frame.len() {
        tracing::debug!(
          code = message.code().code(),
          name = message.code().name(),
          unread = frame.len() - read,
          "server message has bytes past its fields"
        );
      }
    }
    self.handle_server_message(message, frame.len());
  }

  fn handle_server_message(&self, message: ServerResponse, len: usize) {
    use ServerResponse as R;
    let inner = &self.inner;
    match message {
      R::Login(response) => self.complete(WaitKey::Server(ServerCode::Login), response),
      R::GetPeerAddress(response) => {
        self.complete(
          user_key(ServerCode::GetPeerAddress, &response.username),
          response,
        );
      }
      R::WatchUser(response) => {
        self.complete(
          user_key(ServerCode::WatchUser, &response.username),
          response,
        );
      }
      R::GetStatus(status) => {
        self.complete(
          user_key(ServerCode::GetStatus, &status.username),
          status.clone(),
        );
        inner.emit(Event::UserStatusChanged(status));
      }
      R::SayInChatRoom(said) => inner.emit(Event::RoomMessageReceived {
        room_name: said.room,
        username: said.username,
        message: said.message,
      }),
      R::JoinRoom(room) => self.complete(room_key(ServerCode::JoinRoom, &room.name), room),
      R::LeaveRoom(left) => {
        self.complete(room_key(ServerCode::LeaveRoom, &left.room), ());
        // the server does not announce our own departure, so it is raised here
        match self.username() {
          Some(username) => inner.emit(Event::RoomLeft {
            room_name: left.room,
            username,
          }),
          None => tracing::debug!(room = left.room, "room left after the session ended"),
        }
      }
      R::UserJoinedRoom(joined) => inner.emit(Event::RoomJoined {
        room_name: joined.room,
        username: joined.user.username.clone(),
        user_data: joined.user,
      }),
      R::UserLeftRoom(left) => inner.emit(Event::RoomLeft {
        room_name: left.room,
        username: left.username,
      }),
      R::ConnectToPeer(request) => self.hand_off(ServerHandoff::ConnectToPeer(request)),
      R::PrivateMessage(message) => {
        let id = message.id;
        inner.emit(Event::PrivateMessageReceived(PrivateMessage {
          id,
          timestamp: UNIX_EPOCH + Duration::from_secs(message.timestamp.into()),
          username: message.username,
          message: message.message,
          replayed: !message.is_new,
        }));
        if inner.options().auto_acknowledge_private_messages {
          self.send_server_message(&ServerRequest::AcknowledgePrivateMessage(
            AcknowledgePrivateMessage { id },
          ));
        }
      }
      R::FileSearch(search) => {
        // answering our own searches is up to the search that sent them
        if self.username().as_deref() == Some(search.username.as_str()) {
          tracing::debug!(token = search.token, "search from our own username dropped");
        } else {
          self.hand_off(ServerHandoff::FileSearch(search));
        }
      }
      R::Ping => self.complete(WaitKey::Server(ServerCode::Ping), ()),
      R::GetUserStats(statistics) => {
        self.complete(
          user_key(ServerCode::GetUserStats, &statistics.username),
          statistics.clone(),
        );
        inner.emit(Event::UserStatisticsChanged(statistics));
      }
      R::KickedFromServer => {
        tracing::warn!("Kicked from server.");
        inner.emit(Event::KickedFromServer);
        if let Some(teardown) = inner.begin_disconnect(
          Some("Kicked from server"),
          Some(ClientError::KickedFromServer),
          Cause::Kicked,
        ) {
          let client = self.clone();
          tokio::spawn(async move { teardown.finish(&client.inner).await });
        }
      }
      R::GetPersonalRecommendations(recommendations) => self.complete(
        WaitKey::Server(ServerCode::GetPersonalRecommendations),
        recommendations,
      ),
      R::GetGlobalRecommendations(recommendations) => self.complete(
        WaitKey::Server(ServerCode::GetGlobalRecommendations),
        recommendations,
      ),
      R::GetUserInterests(interests) => {
        self.complete(
          user_key(ServerCode::GetUserInterests, &interests.username),
          interests,
        );
      }
      R::RoomList(list) => {
        let list = Arc::new(list);
        self.complete(WaitKey::Server(ServerCode::RoomList), list.clone());
        inner.emit(Event::RoomListReceived(list));
      }
      R::GlobalAdminMessage(message) => inner.emit(Event::GlobalMessageReceived(message.message)),
      R::PrivilegedUsers(users) => inner.emit(Event::PrivilegedUserListReceived(users.usernames)),
      R::ParentMinSpeed(speed) => {
        inner.merge_server_info(|info| info.parent_min_speed = Some(speed.speed));
      }
      R::ParentSpeedRatio(ratio) => {
        inner.merge_server_info(|info| info.parent_speed_ratio = Some(ratio.ratio));
      }
      R::WishlistInterval(interval) => {
        inner.merge_server_info(|info| info.wishlist_interval = Some(interval.seconds));
      }
      R::ParentInactivityTimeout(value) => {
        self.store_obsolete(ServerCode::ParentInactivityTimeout, |info| {
          info.parent_inactivity_timeout = Some(value.seconds);
        });
      }
      R::SearchInactivityTimeout(value) => {
        self.store_obsolete(ServerCode::SearchInactivityTimeout, |info| {
          info.search_inactivity_timeout = Some(value.seconds);
        });
      }
      R::MinimumParentsInCache(value) => {
        self.store_obsolete(ServerCode::MinimumParentsInCache, |info| {
          info.minimum_parents_in_cache = Some(value.count);
        });
      }
      R::DistributedAliveInterval(value) => {
        self.store_obsolete(ServerCode::DistributedAliveInterval, |info| {
          info.distributed_alive_interval = Some(value.seconds);
        });
      }
      R::AddPrivilegedUser(user) => inner.emit(Event::PrivilegeNotificationReceived {
        username: user.username,
        id: None,
        requires_acknowledgement: false,
      }),
      R::CheckPrivileges(privileges) => self.complete(
        WaitKey::Server(ServerCode::CheckPrivileges),
        privileges.seconds_remaining,
      ),
      R::EmbeddedMessage(embedded) => self.hand_off(ServerHandoff::EmbeddedMessage(embedded)),
      R::NetInfo(candidates) => self.hand_off(ServerHandoff::NetInfo(candidates)),
      R::GetPersonalSimilarUsers(users) => {
        self.complete(WaitKey::Server(ServerCode::GetPersonalSimilarUsers), users);
      }
      R::GetInterestRecommendations(recommendations) => self.complete(
        interest_key(
          ServerCode::GetInterestRecommendations,
          &recommendations.interest,
        ),
        recommendations,
      ),
      R::GetInterestSimilarUsers(users) => self.complete(
        interest_key(ServerCode::GetInterestSimilarUsers, &users.interest),
        users,
      ),
      R::RoomTickers(tickers) => inner.emit(Event::RoomTickerListReceived {
        room_name: tickers.room,
        tickers: tickers.tickers,
      }),
      R::RoomTickerAdd(added) => inner.emit(Event::RoomTickerAdded {
        room_name: added.room,
        ticker: added.ticker,
      }),
      R::RoomTickerRemove(removed) => inner.emit(Event::RoomTickerRemoved {
        room_name: removed.room,
        username: removed.username,
      }),
      R::UserPrivileges(privileges) => self.complete(
        user_key(ServerCode::UserPrivileges, &privileges.username),
        privileges.privileged,
      ),
      R::NotifyPrivileges(notification) => {
        let id = notification.id;
        inner.emit(Event::PrivilegeNotificationReceived {
          username: notification.username,
          id: Some(id),
          requires_acknowledgement: true,
        });
        if inner.options().auto_acknowledge_privilege_notifications {
          self.send_server_message(&ServerRequest::AcknowledgeNotifyPrivileges(
            AcknowledgeNotifyPrivileges { id },
          ));
        }
      }
      R::DistributedReset => {
        tracing::info!("the server reset the distributed network");
        inner.emit(Event::DistributedNetworkReset);
        self.hand_off(ServerHandoff::DistributedReset);
      }
      R::PrivateRoomUsers(members) => {
        inner.emit(Event::PrivateRoomUserListReceived(RoomMembers {
          room_name: members.room,
          users: members.usernames,
        }));
      }
      R::PrivateRoomAddUser(added) => {
        self.complete(
          room_user_key(ServerCode::PrivateRoomAddUser, &added.room, &added.username),
          (),
        );
        inner.emit(Event::PrivateRoomUserAdded {
          room_name: added.room,
          username: added.username,
        });
      }
      R::PrivateRoomRemoveUser(removed) => {
        self.complete(
          room_user_key(
            ServerCode::PrivateRoomRemoveUser,
            &removed.room,
            &removed.username,
          ),
          (),
        );
        inner.emit(Event::PrivateRoomUserRemoved {
          room_name: removed.room,
          username: removed.username,
        });
      }
      R::PrivateRoomAdded(added) => inner.emit(Event::PrivateRoomMembershipAdded(added.room)),
      R::PrivateRoomRemoved(removed) => {
        self.complete(room_key(ServerCode::PrivateRoomRemoved, &removed.room), ());
        inner.emit(Event::PrivateRoomMembershipRemoved(removed.room));
      }
      R::PrivateRoomToggle(toggle) => self.complete(
        WaitKey::Server(ServerCode::PrivateRoomToggle),
        toggle.accept_invitations,
      ),
      R::NewPassword(echo) => {
        self.complete(WaitKey::Server(ServerCode::NewPassword), echo.new_password);
      }
      R::PrivateRoomAddOperator(added) => {
        self.complete(
          room_user_key(
            ServerCode::PrivateRoomAddOperator,
            &added.room,
            &added.username,
          ),
          (),
        );
        inner.emit(Event::PrivateRoomOperatorAdded {
          room_name: added.room,
          username: added.username,
        });
      }
      R::PrivateRoomRemoveOperator(removed) => {
        self.complete(
          room_user_key(
            ServerCode::PrivateRoomRemoveOperator,
            &removed.room,
            &removed.username,
          ),
          (),
        );
        inner.emit(Event::PrivateRoomOperatorRemoved {
          room_name: removed.room,
          username: removed.username,
        });
      }
      R::PrivateRoomOperatorAdded(added) => {
        inner.emit(Event::PrivateRoomModerationAdded(added.room));
      }
      R::PrivateRoomOperatorRemoved(removed) => {
        self.complete(
          room_key(ServerCode::PrivateRoomOperatorRemoved, &removed.room),
          (),
        );
        inner.emit(Event::PrivateRoomModerationRemoved(removed.room));
      }
      R::PrivateRoomOwned(operators) => {
        inner.emit(Event::PrivateRoomModeratedUserListReceived(RoomMembers {
          room_name: operators.room,
          users: operators.operators,
        }));
      }
      R::PublicChat(said) => inner.emit(Event::PublicChatMessageReceived {
        room_name: said.room,
        username: said.username,
        message: said.message,
      }),
      R::ExcludedSearchPhrases(phrases) => {
        inner.emit(Event::ExcludedSearchPhrasesReceived(phrases.phrases));
      }
      R::CannotConnect(failure) => {
        if let Some(username) = &failure.username {
          inner.emit(Event::UserCannotConnect {
            token: failure.token,
            username: username.clone(),
          });
        }
        self.hand_off(ServerHandoff::CannotConnect(failure));
      }
      R::CannotJoinRoom(refused) => {
        let faulted = inner.waiter.fault(
          &room_key(ServerCode::JoinRoom, &refused.room),
          ClientError::Room {
            kind: RoomErrorKind::JoinForbidden,
            message: "A private room with this name already exists".into(),
          },
        );
        if !faulted {
          tracing::debug!(room = refused.room, "refused join nobody waits for");
        }
      }
      R::IgnoreUser(_)
      | R::UnignoreUser(_)
      | R::SendConnectToken(_)
      | R::QueuedDownloads(_)
      | R::SimilarRecommendations(_)
      | R::MyRecommendations(_)
      | R::PlaceInLineRequest(_)
      | R::PlaceInLineResponse(_)
      | R::RoomAdded(_)
      | R::RoomRemoved(_)
      | R::ExactFileSearch(_)
      | R::GlobalUserList(_)
      | R::TunneledMessage(_)
      | R::PrivateRoomUnknown(_)
      | R::RelatedSearch(_) => unhandled(message.code().code(), len),
    }
  }

  /// completes the oldest wait under `key`; a reply nobody waits for is normal for pushed
  /// messages and is dropped
  fn complete<T: Send + 'static>(&self, key: WaitKey, value: T) {
    if !self.inner.waiter.complete(&key, value) {
      tracing::trace!(?key, "reply nobody waits for");
    }
  }

  fn store_obsolete(&self, code: ServerCode, store: impl FnOnce(&mut ServerInfo)) {
    tracing::debug!(
      code = code.code(),
      name = code.name(),
      "server sent an obsolete tuning value; stored, not used"
    );
    self.inner.merge_server_info(store);
  }

  fn hand_off(&self, handoff: ServerHandoff) {
    let name = handoff.name();
    match self.inner.server_handoffs.try_send(handoff) {
      Ok(()) => tracing::trace!(name, "server message handed on"),
      Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
        if let Some(suppressed) = HANDOFF_FULL.allow() {
          tracing::warn!(
            name,
            suppressed,
            "server hand-off queue is full, message dropped"
          );
        }
      }
      Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
        tracing::debug!(name, "nothing takes server hand-offs, message dropped");
      }
    }
  }
}

/// the debug line for a server message nothing handles, naming the code when it is one the
/// server no longer sends
fn unhandled(code: u32, len: usize) {
  match ServerCode::from_code(code).filter(|code| OBSOLETE.contains(code)) {
    Some(obsolete) => {
      tracing::debug!(code, name = obsolete.name(), len, "obsolete server message");
    }
    None => tracing::debug!(code, len, "unhandled server message"),
  }
}

fn user_key(code: ServerCode, username: &str) -> WaitKey {
  WaitKey::ServerUser {
    code,
    username: username.to_owned(),
  }
}

fn interest_key(code: ServerCode, interest: &str) -> WaitKey {
  WaitKey::ServerInterest {
    code,
    interest: interest.to_owned(),
  }
}

fn room_key(code: ServerCode, room: &str) -> WaitKey {
  WaitKey::ServerRoom {
    code,
    room: room.to_owned(),
  }
}

fn room_user_key(code: ServerCode, room: &str, username: &str) -> WaitKey {
  WaitKey::ServerRoomUser {
    code,
    room: room.to_owned(),
    username: username.to_owned(),
  }
}
