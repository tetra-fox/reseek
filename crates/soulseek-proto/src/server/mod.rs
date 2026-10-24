//! The server connection (spec 01 section 5, 01a section 2): [`ServerRequest`] is what the client
//! sends, [`ServerResponse`] what the server sends, pushed or in reply. A code whose layout
//! differs between the two directions has a `...Request` and a `...Response` struct; a code with
//! one layout both ways has one struct used by both enums. Each message lives in the file of its
//! spec topic; this file holds only the catalogue.

pub(crate) mod distributed;
pub(crate) mod interests;
pub(crate) mod messaging;
pub(crate) mod obsolete;
pub(crate) mod peers;
pub(crate) mod private_rooms;
pub(crate) mod privileges;
pub(crate) mod rooms;
pub(crate) mod search;
pub(crate) mod session;
pub(crate) mod users;

pub use distributed::*;
pub use interests::*;
pub use messaging::*;
pub use obsolete::*;
pub use peers::*;
pub use private_rooms::*;
pub use privileges::*;
pub use rooms::*;
pub use search::*;
pub use session::*;
pub use users::*;

use crate::codec::family;
use crate::codes::ServerCode;
use crate::distributed::Embedded;

family! {
  pub enum ServerRequest {
    code: ServerCode,
    frame: Server,
    label: "server request",
    quiet: [],
    messages: {
      Login(LoginRequest),
      SetListenPort(SetListenPort),
      GetPeerAddress(GetPeerAddressRequest),
      WatchUser(WatchUserRequest),
      UnwatchUser(UnwatchUser),
      GetStatus(GetStatusRequest),
      /// obsolete
      IgnoreUser(IgnoreUser),
      /// obsolete
      UnignoreUser(UnignoreUser),
      SayInChatRoom(SayInChatRoomRequest),
      JoinRoom(JoinRoomRequest),
      LeaveRoom(LeaveRoom),
      ConnectToPeer(ConnectToPeerRequest),
      PrivateMessage(PrivateMessageRequest),
      AcknowledgePrivateMessage(AcknowledgePrivateMessage),
      /// obsolete
      FileSearchRoom(FileSearchRoom),
      FileSearch(FileSearchRequest),
      SetOnlineStatus(SetOnlineStatus),
      /// a write-liveness probe; the server never answers
      Ping,
      /// obsolete
      SendConnectToken(SendConnectToken),
      /// obsolete `SendDownloadSpeed`
      SendSpeed(SendSpeed),
      SharedFoldersAndFiles(SharedFoldersAndFiles),
      GetUserStats(GetUserStatsRequest),
      /// obsolete `UploadSlotsFull`
      QueuedDownloads(QueuedDownloadsRequest),
      UserSearch(UserSearch),
      /// obsolete
      SimilarRecommendations(SimilarRecommendationsRequest),
      LikedInterestAdd(LikedInterestAdd),
      LikedInterestRemove(LikedInterestRemove),
      GetPersonalRecommendations,
      /// obsolete
      MyRecommendations,
      GetGlobalRecommendations,
      GetUserInterests(GetUserInterestsRequest),
      /// obsolete
      AdminCommand(AdminCommand),
      /// obsolete
      PlaceInLineRequest(PlaceInLineRequest),
      /// obsolete
      PlaceInLineResponse(PlaceInLineResponse),
      RoomList,
      /// obsolete
      ExactFileSearch(ExactFileSearchRequest),
      /// obsolete
      GlobalUserList,
      /// obsolete
      TunneledMessage(TunneledMessageRequest),
      HaveNoParents(HaveNoParents),
      /// deprecated, never sent
      ParentsIP(ParentsIP),
      CheckPrivileges,
      AcceptChildren(AcceptChildren),
      WishlistSearch(WishlistSearch),
      GetPersonalSimilarUsers,
      GetInterestRecommendations(GetInterestRecommendationsRequest),
      GetInterestSimilarUsers(GetInterestSimilarUsersRequest),
      SetRoomTicker(SetRoomTicker),
      HatedInterestAdd(HatedInterestAdd),
      HatedInterestRemove(HatedInterestRemove),
      RoomSearch(RoomSearch),
      SendUploadSpeed(SendUploadSpeed),
      /// deprecated
      UserPrivileges(UserPrivilegesRequest),
      GivePrivileges(GivePrivileges),
      /// deprecated
      AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges),
      BranchLevel(BranchLevel),
      BranchRoot(BranchRoot),
      /// obsolete, never sent
      ChildDepth(ChildDepth),
      PrivateRoomAddUser(PrivateRoomAddUser),
      PrivateRoomRemoveUser(PrivateRoomRemoveUser),
      PrivateRoomDropMembership(PrivateRoomDropMembership),
      PrivateRoomDropOwnership(PrivateRoomDropOwnership),
      /// obsolete
      PrivateRoomUnknown(PrivateRoomUnknown),
      PrivateRoomToggle(PrivateRoomToggle),
      NewPassword(NewPassword),
      PrivateRoomAddOperator(PrivateRoomAddOperator),
      PrivateRoomRemoveOperator(PrivateRoomRemoveOperator),
      /// codec only
      MessageUsers(MessageUsers),
      AskPublicChat,
      StopPublicChat,
      /// obsolete
      RelatedSearch(RelatedSearchRequest),
      CannotConnect(CannotConnectRequest),
    }
  }
}

family! {
  pub enum ServerResponse {
    code: ServerCode,
    frame: Server,
    label: "server response",
    // embedded messages arrive for every network search while we are a branch root
    quiet: [EmbeddedMessage],
    messages: {
      Login(LoginResponse),
      GetPeerAddress(GetPeerAddressResponse),
      WatchUser(WatchUserResponse),
      GetStatus(UserStatus),
      /// obsolete
      IgnoreUser(IgnoreUser),
      /// obsolete
      UnignoreUser(UnignoreUser),
      SayInChatRoom(SayInChatRoomResponse),
      JoinRoom(RoomData),
      LeaveRoom(LeaveRoom),
      UserJoinedRoom(UserJoinedRoom),
      UserLeftRoom(UserLeftRoom),
      ConnectToPeer(ConnectToPeerResponse),
      PrivateMessage(PrivateMessageResponse),
      /// a search for us to answer
      FileSearch(FileSearchResponse),
      /// the live server never sends this; an unsolicited one is dropped
      Ping,
      /// obsolete
      SendConnectToken(SendConnectToken),
      GetUserStats(UserStatistics),
      /// obsolete `UploadSlotsFull`
      QueuedDownloads(QueuedDownloadsResponse),
      /// another session logged in with our credentials
      KickedFromServer,
      /// obsolete
      SimilarRecommendations(SimilarRecommendationsResponse),
      GetPersonalRecommendations(Recommendations),
      /// obsolete
      MyRecommendations(MyRecommendationsResponse),
      GetGlobalRecommendations(Recommendations),
      GetUserInterests(UserInterests),
      /// obsolete
      PlaceInLineRequest(PlaceInLineRequest),
      /// obsolete
      PlaceInLineResponse(PlaceInLineResponse),
      /// obsolete
      RoomAdded(RoomAdded),
      /// obsolete
      RoomRemoved(RoomRemoved),
      RoomList(RoomList),
      /// obsolete
      ExactFileSearch(ExactFileSearchResponse),
      GlobalAdminMessage(GlobalAdminMessage),
      /// obsolete
      GlobalUserList(GlobalUserListResponse),
      /// obsolete
      TunneledMessage(TunneledMessageResponse),
      PrivilegedUsers(PrivilegedUsers),
      ParentMinSpeed(ParentMinSpeed),
      ParentSpeedRatio(ParentSpeedRatio),
      /// obsolete
      ParentInactivityTimeout(ParentInactivityTimeout),
      /// obsolete
      SearchInactivityTimeout(SearchInactivityTimeout),
      /// obsolete
      MinimumParentsInCache(MinimumParentsInCache),
      /// obsolete
      DistributedAliveInterval(DistributedAliveInterval),
      /// obsolete, the parser is kept
      AddPrivilegedUser(AddPrivilegedUser),
      CheckPrivileges(CheckPrivilegesResponse),
      /// a distributed message the server wraps for a branch root
      EmbeddedMessage(Embedded),
      NetInfo(NetInfo),
      WishlistInterval(WishlistInterval),
      GetPersonalSimilarUsers(GetPersonalSimilarUsersResponse),
      GetInterestRecommendations(GetInterestRecommendationsResponse),
      GetInterestSimilarUsers(GetInterestSimilarUsersResponse),
      RoomTickers(RoomTickers),
      RoomTickerAdd(RoomTickerAdd),
      RoomTickerRemove(RoomTickerRemove),
      /// deprecated
      UserPrivileges(UserPrivilegesResponse),
      /// deprecated, acknowledge with `AcknowledgeNotifyPrivileges`
      NotifyPrivileges(NotifyPrivileges),
      DistributedReset,
      PrivateRoomUsers(PrivateRoomUsers),
      PrivateRoomAddUser(PrivateRoomAddUser),
      PrivateRoomRemoveUser(PrivateRoomRemoveUser),
      /// obsolete
      PrivateRoomUnknown(PrivateRoomUnknown),
      PrivateRoomAdded(PrivateRoomAdded),
      PrivateRoomRemoved(PrivateRoomRemoved),
      PrivateRoomToggle(PrivateRoomToggle),
      NewPassword(NewPassword),
      PrivateRoomAddOperator(PrivateRoomAddOperator),
      PrivateRoomRemoveOperator(PrivateRoomRemoveOperator),
      PrivateRoomOperatorAdded(PrivateRoomOperatorAdded),
      PrivateRoomOperatorRemoved(PrivateRoomOperatorRemoved),
      PrivateRoomOwned(PrivateRoomOwned),
      PublicChat(PublicChat),
      /// obsolete
      RelatedSearch(RelatedSearchResponse),
      ExcludedSearchPhrases(ExcludedSearchPhrases),
      CannotConnect(CannotConnectResponse),
      CannotJoinRoom(CannotJoinRoom),
    }
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::error::DecodeError;
  use crate::testing::{decodes, p};

  decodes! {
    unknown_code_response: ServerResponse, p().u32(1).frame(9999) => Err(DecodeError::UnknownCode { code: 9999 });
  }
}
