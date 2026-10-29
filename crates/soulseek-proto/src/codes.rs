//! The message code catalog of every connection kind. A code being listed here does not mean
//! it has a codec: some peer codes are named by the protocol reference without a layout, and the
//! decoders treat those like unknown codes.

macro_rules! codes {
  ($(#[$meta:meta])* $name:ident: $repr:ty { $($variant:ident = $value:literal,)* }) => {
    $(#[$meta])*
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    #[repr($repr)]
    pub enum $name {
      $($variant = $value,)*
    }

    impl $name {
      #[cfg(test)]
      pub(crate) const ALL: &[Self] = &[$(Self::$variant,)*];

      /// `None` for a code the catalog does not know
      pub fn from_code(code: u32) -> Option<Self> {
        match code {
          $($value => Some(Self::$variant),)*
          _ => None,
        }
      }

      /// the value on the wire
      pub fn code(self) -> u32 {
        self as u32
      }

      /// the variant name, as logs and errors show it
      pub fn name(self) -> &'static str {
        match self {
          $(Self::$variant => stringify!($variant),)*
        }
      }
    }
  };
}

codes! {
  /// Server connection codes (`u32` on the wire). Each code's message struct carries its
  /// description.
  ServerCode: u32 {
    Login = 1,
    SetListenPort = 2,
    GetPeerAddress = 3,
    WatchUser = 5,
    UnwatchUser = 6,
    GetStatus = 7,
    IgnoreUser = 11,
    UnignoreUser = 12,
    SayInChatRoom = 13,
    JoinRoom = 14,
    LeaveRoom = 15,
    UserJoinedRoom = 16,
    UserLeftRoom = 17,
    ConnectToPeer = 18,
    PrivateMessage = 22,
    AcknowledgePrivateMessage = 23,
    FileSearchRoom = 25,
    FileSearch = 26,
    SetOnlineStatus = 28,
    Ping = 32,
    SendConnectToken = 33,
    SendSpeed = 34,
    SharedFoldersAndFiles = 35,
    GetUserStats = 36,
    QueuedDownloads = 40,
    KickedFromServer = 41,
    UserSearch = 42,
    SimilarRecommendations = 50,
    LikedInterestAdd = 51,
    LikedInterestRemove = 52,
    GetPersonalRecommendations = 54,
    MyRecommendations = 55,
    GetGlobalRecommendations = 56,
    GetUserInterests = 57,
    AdminCommand = 58,
    PlaceInLineRequest = 59,
    PlaceInLineResponse = 60,
    RoomAdded = 62,
    RoomRemoved = 63,
    RoomList = 64,
    ExactFileSearch = 65,
    GlobalAdminMessage = 66,
    GlobalUserList = 67,
    TunneledMessage = 68,
    PrivilegedUsers = 69,
    HaveNoParents = 71,
    ParentsIP = 73,
    ParentMinSpeed = 83,
    ParentSpeedRatio = 84,
    ParentInactivityTimeout = 86,
    SearchInactivityTimeout = 87,
    MinimumParentsInCache = 88,
    DistributedAliveInterval = 90,
    AddPrivilegedUser = 91,
    CheckPrivileges = 92,
    EmbeddedMessage = 93,
    AcceptChildren = 100,
    NetInfo = 102,
    WishlistSearch = 103,
    WishlistInterval = 104,
    GetPersonalSimilarUsers = 110,
    GetInterestRecommendations = 111,
    GetInterestSimilarUsers = 112,
    RoomTickers = 113,
    RoomTickerAdd = 114,
    RoomTickerRemove = 115,
    SetRoomTicker = 116,
    HatedInterestAdd = 117,
    HatedInterestRemove = 118,
    RoomSearch = 120,
    SendUploadSpeed = 121,
    UserPrivileges = 122,
    GivePrivileges = 123,
    NotifyPrivileges = 124,
    AcknowledgeNotifyPrivileges = 125,
    BranchLevel = 126,
    BranchRoot = 127,
    ChildDepth = 129,
    DistributedReset = 130,
    PrivateRoomUsers = 133,
    PrivateRoomAddUser = 134,
    PrivateRoomRemoveUser = 135,
    PrivateRoomDropMembership = 136,
    PrivateRoomDropOwnership = 137,
    PrivateRoomUnknown = 138,
    PrivateRoomAdded = 139,
    PrivateRoomRemoved = 140,
    PrivateRoomToggle = 141,
    NewPassword = 142,
    PrivateRoomAddOperator = 143,
    PrivateRoomRemoveOperator = 144,
    PrivateRoomOperatorAdded = 145,
    PrivateRoomOperatorRemoved = 146,
    PrivateRoomOwned = 148,
    MessageUsers = 149,
    AskPublicChat = 150,
    StopPublicChat = 151,
    PublicChat = 152,
    RelatedSearch = 153,
    ExcludedSearchPhrases = 160,
    CannotConnect = 1001,
    CannotJoinRoom = 1003,
  }
}

codes! {
  /// Peer connection codes (`u32` on the wire). Each code's message struct carries its
  /// description; 1, 10, 14, 33, 34, 47, 48 and 49 have no known layout and are decoded as unknown
  /// codes.
  PeerCode: u32 {
    PrivateMessage = 1,
    BrowseRequest = 4,
    BrowseResponse = 5,
    SearchRequest = 8,
    SearchResponse = 9,
    PrivateRoomInvitation = 10,
    CancelledQueuedTransfer = 14,
    InfoRequest = 15,
    InfoResponse = 16,
    SendConnectToken = 33,
    MoveDownloadToTop = 34,
    FolderContentsRequest = 36,
    FolderContentsResponse = 37,
    TransferRequest = 40,
    TransferResponse = 41,
    UploadPlacehold = 42,
    QueueDownload = 43,
    PlaceInQueueResponse = 44,
    UploadFailed = 46,
    ExactFileSearchRequest = 47,
    QueuedDownloads = 48,
    IndirectFileSearchRequest = 49,
    UploadDenied = 50,
    PlaceInQueueRequest = 51,
    UploadQueueNotification = 52,
  }
}

codes! {
  /// Distributed connection codes (`u8` on the wire). Each code's message struct carries its
  /// description.
  DistributedCode: u8 {
    Ping = 0,
    SearchRequest = 3,
    BranchLevel = 4,
    BranchRoot = 5,
    ChildDepth = 7,
    EmbeddedMessage = 93,
  }
}

codes! {
  /// Codes of the first frame on an inbound peer socket (`u8` on the wire). Each code's message
  /// struct carries its description.
  InitCode: u8 {
    PierceFirewall = 0,
    PeerInit = 1,
  }
}
