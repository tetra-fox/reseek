//! What the client tells the application: every state change, every pushed server message the
//! application acts on, and how its transfers, searches and browses go. Events go out on one
//! broadcast channel; a subscriber that falls more than [`EVENT_CAPACITY`] behind gets
//! `RecvError::Lagged(n)` from its receiver and continues with the newest events.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

use soulseek_proto::RemoteFilename;
use soulseek_proto::peer::SearchResponse;
use soulseek_proto::server::{RoomList, RoomTicker, UserData, UserStatistics, UserStatus};

use crate::browse::BrowseProgress;
use crate::error::ClientError;
use crate::resolvers::SearchReply;
use crate::search::{Search, SearchState};
use crate::state::{ClientState, DistributedNetworkInfo, ServerInfo};
use crate::transfers::{Transfer, TransferState};

/// how many events the channel holds for its slowest subscriber
pub const EVENT_CAPACITY: usize = 4096;

/// One thing that happened in the client
#[derive(Clone, Debug)]
pub enum Event {
  /// the client moved from one state to another
  StateChanged {
    previous_state: ClientState,
    state: ClientState,
    message: String,
    error: Option<ClientError>,
  },
  /// the TCP connection to the server is up
  Connected,
  /// the server accepted our login as `username`
  LoggedIn { username: String },
  /// the server session ended; raised once per session, after the last `StateChanged`
  Disconnected {
    message: String,
    error: Option<ClientError>,
  },
  /// another session logged in with our credentials and the server dropped us
  KickedFromServer,
  /// the merged server tuning values, after the login reply or any message that changes one
  ServerInfoReceived(ServerInfo),
  /// a message from the server's administrators to every user
  GlobalMessageReceived(String),
  /// phrases our search responses must not contain
  ExcludedSearchPhrasesReceived(Vec<String>),
  /// a private message to us, live or replayed at login
  PrivateMessageReceived(PrivateMessage),
  /// a user was granted privileges; `id` is set when the server wants it acknowledged
  PrivilegeNotificationReceived {
    username: String,
    id: Option<u32>,
    requires_acknowledgment: bool,
  },
  /// every privileged user, pushed after login
  PrivilegedUserListReceived(Vec<String>),
  /// a user's presence, asked for or pushed for a watched user or a room member
  UserStatusChanged(UserStatus),
  /// a user's statistics, asked for or pushed for a member of a room we are in
  UserStatisticsChanged(UserStatistics),
  /// the server reports that a user could not connect to us for a connection request we made;
  /// the server usually sends only the token, and then this is not raised
  UserCannotConnect { token: u32, username: String },
  /// the rooms on the server, pushed after login or asked for
  RoomListReceived(Arc<RoomList>),
  /// a user, ourselves included, joined a room we are in
  RoomJoined {
    room_name: String,
    username: String,
    user_data: UserData,
  },
  /// a user left a room we are in; raised for ourselves when we leave, which the server does
  /// not announce
  RoomLeft { room_name: String, username: String },
  /// a chat line in a room we are in, our own included
  RoomMessageReceived {
    room_name: String,
    username: String,
    message: String,
  },
  /// a line from any public room, while the public chat feed is on
  PublicChatMessageReceived {
    room_name: String,
    username: String,
    message: String,
  },
  /// every ticker of a room, pushed when we join it
  RoomTickerListReceived {
    room_name: String,
    tickers: Vec<RoomTicker>,
  },
  /// a user set their ticker in a room we are in
  RoomTickerAdded {
    room_name: String,
    ticker: RoomTicker,
  },
  /// a user cleared their ticker in a room we are in
  RoomTickerRemoved { room_name: String, username: String },
  /// we were added to a private room
  PrivateRoomMembershipAdded(String),
  /// we were removed from a private room
  PrivateRoomMembershipRemoved(String),
  /// we were made an operator of a private room
  PrivateRoomModerationAdded(String),
  /// we stopped being an operator of a private room
  PrivateRoomModerationRemoved(String),
  /// someone was added to a private room we are in
  PrivateRoomUserAdded { room_name: String, username: String },
  /// someone was removed from a private room we are in
  PrivateRoomUserRemoved { room_name: String, username: String },
  /// someone was made an operator of a private room we are in
  PrivateRoomOperatorAdded { room_name: String, username: String },
  /// someone stopped being an operator of a private room we are in
  PrivateRoomOperatorRemoved { room_name: String, username: String },
  /// the members of a private room we are in, without its owner
  PrivateRoomUserListReceived(RoomMembers),
  /// the operators of a private room we are in
  PrivateRoomModeratedUserListReceived(RoomMembers),
  /// our place in the distributed search tree changed
  DistributedNetworkStateChanged(DistributedNetworkInfo),
  /// the server told every client to rebuild the distributed search tree
  DistributedNetworkReset,
  /// we took a parent in the distributed search tree
  DistributedParentAdopted(DistributedParent),
  /// our distributed parent's connection closed
  DistributedParentDisconnected(DistributedParent),
  /// a user joined the distributed search tree below us
  DistributedChildAdded {
    username: String,
    ip_endpoint: SocketAddr,
  },
  /// a distributed child's connection closed
  DistributedChildDisconnected {
    username: String,
    ip_endpoint: SocketAddr,
  },
  /// the server made us the root of a branch of the distributed search tree
  PromotedToDistributedBranchRoot,
  /// we stopped being a branch root
  DemotedFromDistributedBranchRoot,
  /// another user's search reached us
  SearchRequestReceived {
    username: String,
    token: u32,
    query: String,
  },
  /// we sent our results for another user's search
  SearchResponseDelivered(SearchDelivery),
  /// results we kept for an unreachable searcher were given up
  SearchResponseDeliveryFailed(SearchDelivery),
  /// a peer refused to queue a file we asked for
  DownloadDenied {
    username: String,
    filename: RemoteFilename,
    message: String,
  },
  /// a peer reported that the upload of a file to us failed
  DownloadFailed {
    username: String,
    filename: RemoteFilename,
  },
  /// a peer told us our place in its queue for a file without our asking, as it does once it
  /// queued the file
  PlaceInQueueReceived {
    username: String,
    filename: RemoteFilename,
    place: u32,
  },
  /// a download or upload moved from one state to another
  TransferStateChanged {
    previous_state: TransferState,
    transfer: Transfer,
  },
  /// bytes of a download or upload moved; raised for every chunk, so a consumer that shows it
  /// should pace itself
  TransferProgressUpdated {
    previous_bytes_transferred: u64,
    transfer: Transfer,
  },
  /// one of our searches moved from one state to another
  SearchStateChanged {
    previous_state: SearchState,
    search: Search,
  },
  /// one of our searches took a response; it may come just after the search completed
  SearchResponseReceived {
    search: Search,
    response: Arc<SearchResponse>,
  },
  /// more of a browse response arrived
  BrowseProgressUpdated(BrowseProgress),
}

/// A private message to us
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateMessage {
  /// acknowledge it with this so the server stops replaying it at login
  pub id: u32,
  pub timestamp: SystemTime,
  pub username: String,
  pub message: String,
  /// the server is replaying it from its store of unacknowledged messages
  pub replayed: bool,
}

/// The users of a private room, as the room membership messages list them
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomMembers {
  pub room_name: String,
  pub users: Vec<String>,
}

/// A distributed parent we adopted or lost
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistributedParent {
  pub username: String,
  pub ip_endpoint: SocketAddr,
  pub branch_level: u32,
  pub branch_root: String,
  pub is_branch_root: bool,
}

/// Our answer to another user's search and whom it was for
#[derive(Clone, Debug)]
pub struct SearchDelivery {
  pub username: String,
  /// the searcher's token
  pub token: u32,
  /// the query as the searcher sent it
  pub query: String,
  pub search_response: Arc<SearchReply>,
}
