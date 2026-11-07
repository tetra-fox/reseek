//! What the client does with each message the server sends: complete the wait of the request it
//! answers, raise the event it carries, or pass it to the part of the client that owns it (peer
//! connections, the distributed tree, the search responder). Frames are handled in order on the
//! server connection's read loop and nothing here blocks; whatever needs a connection or a
//! lookup runs on a task of its own or goes to the tree task.
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

use soulseek_proto::server::{
  AcknowledgeNotifyPrivileges, AcknowledgePrivateMessage, CannotConnectResponse,
  ConnectToPeerResponse, LoginResponse,
};
use soulseek_proto::{DecodeError, PeerConnectionType, ServerCode, ServerRequest, ServerResponse};

use super::Client;
use super::disconnect::Cause;
use super::request::{interest_key, room_key, room_user_key, user_key};
use crate::distributed::Command;
use crate::error::{ClientError, ConnectionErrorKind, RoomErrorKind};
use crate::events::{Event, PrivateMessage, RoomMembers};
use crate::state::{ClientState, ServerInfo};
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
      R::Login(response) => self.login_replied(response),
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
        self.emit(Event::UserStatusChanged(status));
      }
      R::SayInChatRoom(said) => self.emit(Event::RoomMessageReceived {
        room_name: said.room,
        username: said.username,
        message: said.message,
      }),
      R::JoinRoom(room) => self.complete(room_key(ServerCode::JoinRoom, &room.name), room),
      R::LeaveRoom(left) => {
        self.complete(room_key(ServerCode::LeaveRoom, &left.room), ());
        // the server does not announce our own departure, so it is raised here
        match self.username() {
          Some(username) => self.emit(Event::RoomLeft {
            room_name: left.room,
            username,
          }),
          None => tracing::debug!(room = left.room, "room left after the session ended"),
        }
      }
      R::UserJoinedRoom(joined) => self.emit(Event::RoomJoined {
        room_name: joined.room,
        username: joined.user.username.clone(),
        user_data: joined.user,
      }),
      R::UserLeftRoom(left) => self.emit(Event::RoomLeft {
        room_name: left.room,
        username: left.username,
      }),
      R::ConnectToPeer(request) => self.connect_to_peer(request),
      R::PrivateMessage(message) => {
        let id = message.id;
        self.emit(Event::PrivateMessageReceived(PrivateMessage {
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
        // our own search reaches us too; it is answered only when we asked ourselves on purpose
        let own = self.username();
        if own.as_deref() == Some(search.username.as_str())
          && !own.is_some_and(|own| self.searches().is_self_search(search.token, &own))
        {
          tracing::debug!(token = search.token, "search from our own username dropped");
        } else {
          self.respond_to_search(search.username, search.token, search.query);
        }
      }
      R::Ping => self.complete(WaitKey::Server(ServerCode::Ping), ()),
      R::GetUserStats(statistics) => {
        if self.username().as_deref() == Some(statistics.username.as_str()) {
          // our own speed decides how many distributed children we may take
          self.distributed_command(Command::OwnSpeed(statistics.stats.average_speed));
        }
        self.complete(
          user_key(ServerCode::GetUserStats, &statistics.username),
          statistics.clone(),
        );
        self.emit(Event::UserStatisticsChanged(statistics));
      }
      R::KickedFromServer => {
        tracing::warn!("Kicked from server.");
        self.emit(Event::KickedFromServer);
        if let Some(teardown) = inner.begin_disconnect(
          Some("Kicked from server"),
          Some(ClientError::KickedFromServer),
          Cause::Kicked,
        ) {
          let client = self.clone();
          tokio::spawn(async move { teardown.finish(&client).await });
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
        self.emit(Event::RoomListReceived(list));
      }
      R::GlobalAdminMessage(message) => self.emit(Event::GlobalMessageReceived(message.message)),
      R::PrivilegedUsers(users) => self.emit(Event::PrivilegedUserListReceived(users.usernames)),
      R::ParentMinSpeed(speed) => {
        inner.merge_server_info(|info| info.parent_min_speed = Some(speed.speed));
        self.distributed_command(Command::Changed);
      }
      R::ParentSpeedRatio(ratio) => {
        inner.merge_server_info(|info| info.parent_speed_ratio = Some(ratio.ratio));
        self.distributed_command(Command::Changed);
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
      R::AddPrivilegedUser(user) => self.emit(Event::PrivilegeNotificationReceived {
        username: user.username,
        id: None,
        requires_acknowledgment: false,
      }),
      R::CheckPrivileges(privileges) => self.complete(
        WaitKey::Server(ServerCode::CheckPrivileges),
        privileges.seconds_remaining,
      ),
      R::EmbeddedMessage(embedded) => self.handle_embedded(&embedded),
      R::NetInfo(candidates) => {
        self.distributed_command(Command::NetInfo(candidates.parents));
      }
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
      R::RoomTickers(tickers) => self.emit(Event::RoomTickerListReceived {
        room_name: tickers.room,
        tickers: tickers.tickers,
      }),
      R::RoomTickerAdd(added) => self.emit(Event::RoomTickerAdded {
        room_name: added.room,
        ticker: added.ticker,
      }),
      R::RoomTickerRemove(removed) => self.emit(Event::RoomTickerRemoved {
        room_name: removed.room,
        username: removed.username,
      }),
      R::UserPrivileges(privileges) => self.complete(
        user_key(ServerCode::UserPrivileges, &privileges.username),
        privileges.privileged,
      ),
      R::NotifyPrivileges(notification) => {
        let id = notification.id;
        self.emit(Event::PrivilegeNotificationReceived {
          username: notification.username,
          id: Some(id),
          requires_acknowledgment: true,
        });
        if inner.options().auto_acknowledge_privilege_notifications {
          self.send_server_message(&ServerRequest::AcknowledgeNotifyPrivileges(
            AcknowledgeNotifyPrivileges { id },
          ));
        }
      }
      R::DistributedReset => {
        tracing::info!("the server reset the distributed network");
        self.emit(Event::DistributedNetworkReset);
        self.distributed_command(Command::Reset);
      }
      R::PrivateRoomUsers(members) => {
        self.emit(Event::PrivateRoomUserListReceived(RoomMembers {
          room_name: members.room,
          users: members.usernames,
        }));
      }
      R::PrivateRoomAddUser(added) => {
        self.complete(
          room_user_key(ServerCode::PrivateRoomAddUser, &added.room, &added.username),
          (),
        );
        self.emit(Event::PrivateRoomUserAdded {
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
        self.emit(Event::PrivateRoomUserRemoved {
          room_name: removed.room,
          username: removed.username,
        });
      }
      R::PrivateRoomAdded(added) => self.emit(Event::PrivateRoomMembershipAdded(added.room)),
      R::PrivateRoomRemoved(removed) => {
        self.complete(room_key(ServerCode::PrivateRoomRemoved, &removed.room), ());
        self.emit(Event::PrivateRoomMembershipRemoved(removed.room));
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
        self.emit(Event::PrivateRoomOperatorAdded {
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
        self.emit(Event::PrivateRoomOperatorRemoved {
          room_name: removed.room,
          username: removed.username,
        });
      }
      R::PrivateRoomOperatorAdded(added) => {
        self.emit(Event::PrivateRoomModerationAdded(added.room));
      }
      R::PrivateRoomOperatorRemoved(removed) => {
        self.complete(
          room_key(ServerCode::PrivateRoomOperatorRemoved, &removed.room),
          (),
        );
        self.emit(Event::PrivateRoomModerationRemoved(removed.room));
      }
      R::PrivateRoomOwned(operators) => {
        self.emit(Event::PrivateRoomModeratedUserListReceived(RoomMembers {
          room_name: operators.room,
          users: operators.operators,
        }));
      }
      R::PublicChat(said) => self.emit(Event::PublicChatMessageReceived {
        room_name: said.room,
        username: said.username,
        message: said.message,
      }),
      R::ExcludedSearchPhrases(phrases) => {
        self.emit(Event::ExcludedSearchPhrasesReceived(phrases.phrases));
      }
      R::CannotConnect(failure) => self.cannot_connect(&failure),
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

  /// A successful login takes effect as its reply is read, before the next frame is handled, so
  /// whatever the server sends right after it is raised after `LoggedIn`. The login in progress
  /// then takes the reply from its wait.
  fn login_replied(&self, response: LoginResponse) {
    if let LoginResponse::Success {
      ip, is_supporter, ..
    } = &response
    {
      let mut session = self.inner.session();
      if self.state() == ClientState::CONNECTED | ClientState::LOGGING_IN
        && let Some(username) = session.logging_in_as.take()
      {
        self.inner.merge_server_info(|info| {
          info.is_supporter = Some(*is_supporter);
          info.own_public_ip = Some(*ip);
        });
        session.username = Some(username);
        self
          .inner
          .transition(&mut session, ClientState::READY, "Logged in".into(), None);
      }
    }
    self.complete(WaitKey::Server(ServerCode::Login), response);
  }

  /// A peer asks, through the server, that we connect to it: for messages, for a file it is about
  /// to upload to us, or to become our distributed child. Each is handled away from the read
  /// loop, and a request we cannot honor is reported back so the peer stops waiting.
  fn connect_to_peer(&self, request: ConnectToPeerResponse) {
    tracing::debug!(
      username = request.username,
      connection_type = ?request.connection_type,
      token = request.token,
      "connection request relayed by the server"
    );
    match request.connection_type {
      PeerConnectionType::Peer => self.connect_to_peer_for_messages(request),
      PeerConnectionType::Transfer => self.connect_to_peer_for_transfer(request),
      PeerConnectionType::Distributed => {
        self.distributed_command(Command::ChildRequested(request));
      }
    }
  }

  /// A peer could not act on a connection request we had the server relay. A search answer
  /// kept under that token is given up, and a solicitation still waiting fails at once rather
  /// than at its timeout; a connection answering it afterwards is unknown.
  fn cannot_connect(&self, failure: &CannotConnectResponse) {
    let token = failure.token;
    if let Some(username) = &failure.username {
      self.emit(Event::UserCannotConnect {
        token,
        username: username.clone(),
      });
    }
    let discarded = self.try_discard(token);
    let solicited = self
      .peers()
      .solicitations
      .refused(token)
      .is_some_and(|solicitation| {
        self.waiter().fault(
          &solicitation.key(token),
          ClientError::connection(
            ConnectionErrorKind::Other,
            format!("CannotConnect received for token {token}"),
          ),
        )
      });
    if solicited {
      // a search answer riding on this request is not worth keeping for a late connection
      self.responder().refuse(token);
    }
    tracing::debug!(token, discarded, solicited, "peer cannot connect");
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
