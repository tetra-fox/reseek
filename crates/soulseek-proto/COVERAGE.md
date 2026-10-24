# soulseek-proto coverage

One row per message code of §01 §4 and §01a §1. Every message is a struct whose `Wire` impl
reads and writes its layout in one place, in the file of its spec topic (`src/server/rooms.rs`,
`src/peer/transfers.rs`, ...) or in `src/distributed.rs` and `src/init.rs`; "Codec" names that
struct by its public path and links to where it is. Encoding goes through the family enum's
`encode`, decoding through its `decode` (`PeerMessage::decode_with` for configured limits).
Server codes have a request codec (`ServerRequest`, client to server) and a response codec
(`ServerResponse`, server to client) where that direction exists.

"Round trip" lists the byte-exact tests and "Also" the compat, rejection and other tests for
that message; every test is at the bottom of the file that defines what it tests and is named
after the message.
Every family is also covered by the property tests in `src/proptests.rs` and by a fuzz target.
"Limits" names what the decoder enforces beyond the frame length, which is checked on every row.

## Server (§01 §4.1, §01a §1.1)

Frame length <= `LIMIT_SERVER_MESSAGE_BYTES` on every row.

| Code | Name | Request codec | Response codec | Round trip | Also | Limits |
|---|---|---|---|---|---|---|
| 0 | Unknown | n/a: not a wire code | n/a: decodes as `UnknownCode` | n/a | `unknown_code_response` | n/a |
| 1 | Login | `server::LoginRequest` ([src/server/session.rs:30](src/server/session.rs#L30)) | `server::LoginResponse` ([src/server/session.rs:75](src/server/session.rs#L75)) | `login_request`, `login_response_success`, `login_response_failure`, `login_response_failure_with_detail` | `login_response_success_must_be_exactly_one`, `login_hash_is_md5_of_username_and_password`, `login_request_debug_never_shows_credentials` | strings <= LIMIT_STRING_BYTES; greeting <= LIMIT_DESCRIPTION_BYTES |
| 2 | SetListenPort | `server::SetListenPort` ([src/server/session.rs:133](src/server/session.rs#L133)) | - | `set_listen_port_request`, `set_listen_port_request_obfuscated` | `set_listen_port_response_is_not_a_server_message` | fixed-size or empty payload |
| 3 | GetPeerAddress | `server::GetPeerAddressRequest` ([src/server/users.rs:67](src/server/users.rs#L67)) | `server::GetPeerAddressResponse` ([src/server/users.rs:84](src/server/users.rs#L84)) | `get_peer_address_request`, `get_peer_address_response`, `get_peer_address_response_without_trailer` | `get_peer_address_response_ignores_a_partial_trailer` | strings <= LIMIT_STRING_BYTES |
| 5 | WatchUser | `server::WatchUserRequest` ([src/server/users.rs:118](src/server/users.rs#L118)) | `server::WatchUserResponse` ([src/server/users.rs:132](src/server/users.rs#L132)) | `watch_user_request`, `watch_user_response`, `watch_user_response_blank_country`, `watch_user_response_offline`, `watch_user_response_missing` | - | strings <= LIMIT_STRING_BYTES |
| 6 | UnwatchUser | `server::UnwatchUser` ([src/server/users.rs:181](src/server/users.rs#L181)) | - | `unwatch_user_request` | - | strings <= LIMIT_STRING_BYTES |
| 7 | GetStatus | `server::GetStatusRequest` ([src/server/users.rs:188](src/server/users.rs#L188)) | `server::UserStatus` ([src/server/users.rs:197](src/server/users.rs#L197)) | `get_status_request`, `get_status_response` | `get_status_response_bool_accepts_any_non_zero`, `get_status_response_rejects_unknown_presence` | strings <= LIMIT_STRING_BYTES |
| 11 | IgnoreUser | `server::IgnoreUser` ([src/server/users.rs:206](src/server/users.rs#L206)) | `server::IgnoreUser` ([src/server/users.rs:206](src/server/users.rs#L206)) | `ignore_user_request`, `ignore_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 12 | UnignoreUser | `server::UnignoreUser` ([src/server/users.rs:213](src/server/users.rs#L213)) | `server::UnignoreUser` ([src/server/users.rs:213](src/server/users.rs#L213)) | `unignore_user_request`, `unignore_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 13 | SayInChatRoom | `server::SayInChatRoomRequest` ([src/server/rooms.rs:13](src/server/rooms.rs#L13)) | `server::SayInChatRoomResponse` ([src/server/rooms.rs:21](src/server/rooms.rs#L21)) | `say_in_chat_room_request`, `say_in_chat_room_response` | - | strings <= LIMIT_STRING_BYTES |
| 14 | JoinRoom | `server::JoinRoomRequest` ([src/server/rooms.rs:30](src/server/rooms.rs#L30)) | `server::RoomData` ([src/server/rooms.rs:60](src/server/rooms.rs#L60)) | `join_room_request`, `join_room_response`, `join_room_response_private` | `join_room_response_rejects_mismatched_lists` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; five parallel counts must be equal (§9 rule 4) |
| 15 | LeaveRoom | `server::LeaveRoom` ([src/server/rooms.rs:180](src/server/rooms.rs#L180)) | `server::LeaveRoom` ([src/server/rooms.rs:180](src/server/rooms.rs#L180)) | `leave_room_request`, `leave_room_response` | `leave_room_response_ignores_trailing_fields`, `leave_room_response_rejects_a_string_past_the_payload`, `leave_room_response_rejects_a_cut_frame` | strings <= LIMIT_STRING_BYTES |
| 16 | UserJoinedRoom | - | `server::UserJoinedRoom` ([src/server/rooms.rs:195](src/server/rooms.rs#L195)) | `user_joined_room_response` | - | strings <= LIMIT_STRING_BYTES |
| 17 | UserLeftRoom | - | `server::UserLeftRoom` ([src/server/rooms.rs:230](src/server/rooms.rs#L230)) | `user_left_room_response` | - | strings <= LIMIT_STRING_BYTES |
| 18 | ConnectToPeer | `server::ConnectToPeerRequest` ([src/server/peers.rs:14](src/server/peers.rs#L14)) | `server::ConnectToPeerResponse` ([src/server/peers.rs:39](src/server/peers.rs#L39)) | `connect_to_peer_request`, `connect_to_peer_response`, `connect_to_peer_response_without_trailer` | `connect_to_peer_response_rejects_unknown_type` | strings <= LIMIT_STRING_BYTES |
| 22 | PrivateMessage | `server::PrivateMessageRequest` ([src/server/messaging.rs:11](src/server/messaging.rs#L11)) | `server::PrivateMessageResponse` ([src/server/messaging.rs:31](src/server/messaging.rs#L31)) | `private_message_request`, `private_message_response`, `private_message_response_replayed` | `private_message_response_is_new_must_be_exactly_one` | strings <= LIMIT_STRING_BYTES |
| 23 | AcknowledgePrivateMessage | `server::AcknowledgePrivateMessage` ([src/server/messaging.rs:61](src/server/messaging.rs#L61)) | - | `acknowledge_private_message_request` | - | fixed-size or empty payload |
| 25 | FileSearchRoom | `server::FileSearchRoom` ([src/server/search.rs:9](src/server/search.rs#L9)) | - | `file_search_room_request` | - | strings <= LIMIT_STRING_BYTES |
| 26 | FileSearch | `server::FileSearchRequest` ([src/server/search.rs:18](src/server/search.rs#L18)) | `server::FileSearchResponse` ([src/server/search.rs:28](src/server/search.rs#L28)) | `file_search_request`, `file_search_response` | - | strings <= LIMIT_STRING_BYTES |
| 28 | SetOnlineStatus | `server::SetOnlineStatus` ([src/server/session.rs:158](src/server/session.rs#L158)) | - | `set_online_status_request` | - | fixed-size or empty payload |
| 32 | Ping | `ServerRequest::Ping`, empty payload | `ServerResponse::Ping`, empty payload | `ping_request`, `ping_response` | `ping_response_rejects_bytes_after_the_frame` | fixed-size or empty payload |
| 33 | SendConnectToken | `server::SendConnectToken` ([src/server/peers.rs:79](src/server/peers.rs#L79)) | `server::SendConnectToken` ([src/server/peers.rs:79](src/server/peers.rs#L79)) | `send_connect_token_request`, `send_connect_token_response` | - | strings <= LIMIT_STRING_BYTES |
| 34 | SendSpeed | `server::SendSpeed` ([src/server/obsolete.rs:14](src/server/obsolete.rs#L14)) | - | `send_speed_request` | - | strings <= LIMIT_STRING_BYTES |
| 35 | SharedFoldersAndFiles | `server::SharedFoldersAndFiles` ([src/server/session.rs:165](src/server/session.rs#L165)) | - | `shared_folders_and_files_request` | - | fixed-size or empty payload |
| 36 | GetUserStats | `server::GetUserStatsRequest` ([src/server/users.rs:220](src/server/users.rs#L220)) | `server::UserStatistics` ([src/server/users.rs:229](src/server/users.rs#L229)) | `get_user_stats_request`, `get_user_stats_response` | - | strings <= LIMIT_STRING_BYTES |
| 40 | QueuedDownloads | `server::QueuedDownloadsRequest` ([src/server/obsolete.rs:27](src/server/obsolete.rs#L27)) | `server::QueuedDownloadsResponse` ([src/server/obsolete.rs:49](src/server/obsolete.rs#L49)) | `queued_downloads_request`, `queued_downloads_response` | - | strings <= LIMIT_STRING_BYTES |
| 41 | KickedFromServer | - | `ServerResponse::KickedFromServer`, empty payload | `kicked_from_server_response` | - | fixed-size or empty payload |
| 42 | UserSearch | `server::UserSearch` ([src/server/search.rs:37](src/server/search.rs#L37)) | - | `user_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 50 | SimilarRecommendations | `server::SimilarRecommendationsRequest` ([src/server/interests.rs:51](src/server/interests.rs#L51)) | `server::SimilarRecommendationsResponse` ([src/server/interests.rs:58](src/server/interests.rs#L58)) | `similar_recommendations_request`, `similar_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 51 | LikedInterestAdd | `server::LikedInterestAdd` ([src/server/interests.rs:66](src/server/interests.rs#L66)) | - | `liked_interest_add_request` | - | strings <= LIMIT_STRING_BYTES |
| 52 | LikedInterestRemove | `server::LikedInterestRemove` ([src/server/interests.rs:73](src/server/interests.rs#L73)) | - | `liked_interest_remove_request` | - | strings <= LIMIT_STRING_BYTES |
| 54 | GetPersonalRecommendations | `ServerRequest::GetPersonalRecommendations`, empty payload | `server::Recommendations` ([src/server/interests.rs:27](src/server/interests.rs#L27)) | `get_personal_recommendations_request`, `get_personal_recommendations_response` | `get_personal_recommendations_response_without_second_block` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 55 | MyRecommendations | `ServerRequest::MyRecommendations`, empty payload | `server::MyRecommendationsResponse` ([src/server/interests.rs:80](src/server/interests.rs#L80)) | `my_recommendations_request`, `my_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 56 | GetGlobalRecommendations | `ServerRequest::GetGlobalRecommendations`, empty payload | `server::Recommendations` ([src/server/interests.rs:27](src/server/interests.rs#L27)) | `get_global_recommendations_request`, `get_global_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 57 | GetUserInterests | `server::GetUserInterestsRequest` ([src/server/interests.rs:87](src/server/interests.rs#L87)) | `server::UserInterests` ([src/server/interests.rs:96](src/server/interests.rs#L96)) | `get_user_interests_request`, `get_user_interests_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 58 | AdminCommand | `server::AdminCommand` ([src/server/obsolete.rs:71](src/server/obsolete.rs#L71)) | - | `admin_command_request` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 59 | PlaceInLineRequest | `server::PlaceInLineRequest` ([src/server/obsolete.rs:79](src/server/obsolete.rs#L79)) | `server::PlaceInLineRequest` ([src/server/obsolete.rs:79](src/server/obsolete.rs#L79)) | `place_in_line_request_request`, `place_in_line_request_response` | - | strings <= LIMIT_STRING_BYTES |
| 60 | PlaceInLineResponse | `server::PlaceInLineResponse` ([src/server/obsolete.rs:87](src/server/obsolete.rs#L87)) | `server::PlaceInLineResponse` ([src/server/obsolete.rs:87](src/server/obsolete.rs#L87)) | `place_in_line_response_request`, `place_in_line_response_response` | - | strings <= LIMIT_STRING_BYTES |
| 62 | RoomAdded | - | `server::RoomAdded` ([src/server/rooms.rs:238](src/server/rooms.rs#L238)) | `room_added_response` | - | strings <= LIMIT_STRING_BYTES |
| 63 | RoomRemoved | - | `server::RoomRemoved` ([src/server/rooms.rs:245](src/server/rooms.rs#L245)) | `room_removed_response` | - | strings <= LIMIT_STRING_BYTES |
| 64 | RoomList | `ServerRequest::RoomList`, empty payload | `server::RoomList` ([src/server/rooms.rs:300](src/server/rooms.rs#L300)) | `room_list_request`, `room_list_response` | `room_list_response_rejects_mismatched_lists`, `room_list_response_rejects_a_count_past_the_payload` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; name and count lists must be equal (§9 rule 4) |
| 65 | ExactFileSearch | `server::ExactFileSearchRequest` ([src/server/search.rs:46](src/server/search.rs#L46)) | `server::ExactFileSearchResponse` ([src/server/search.rs:58](src/server/search.rs#L58)) | `exact_file_search_request`, `exact_file_search_response` | - | strings <= LIMIT_STRING_BYTES |
| 66 | GlobalAdminMessage | - | `server::GlobalAdminMessage` ([src/server/messaging.rs:68](src/server/messaging.rs#L68)) | `global_admin_message_response` | - | strings <= LIMIT_STRING_BYTES |
| 67 | GlobalUserList | `ServerRequest::GlobalUserList`, empty payload | `server::GlobalUserListResponse` ([src/server/obsolete.rs:102](src/server/obsolete.rs#L102)) | `global_user_list_request`, `global_user_list_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; five parallel counts must be equal (§9 rule 4) |
| 68 | TunneledMessage | `server::TunneledMessageRequest` ([src/server/obsolete.rs:122](src/server/obsolete.rs#L122)) | `server::TunneledMessageResponse` ([src/server/obsolete.rs:132](src/server/obsolete.rs#L132)) | `tunneled_message_request`, `tunneled_message_response` | - | strings <= LIMIT_STRING_BYTES |
| 69 | PrivilegedUsers | - | `server::PrivilegedUsers` ([src/server/privileges.rs:8](src/server/privileges.rs#L8)) | `privileged_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 71 | HaveNoParents | `server::HaveNoParents` ([src/server/distributed.rs:12](src/server/distributed.rs#L12)) | - | `have_no_parents_request` | - | fixed-size or empty payload |
| 73 | ParentsIP | `server::ParentsIP` ([src/server/distributed.rs:24](src/server/distributed.rs#L24)) | - | `parents_ip_request`, `parents_ip_request_without_parent` | - | fixed-size or empty payload |
| 83 | ParentMinSpeed | - | `server::ParentMinSpeed` ([src/server/distributed.rs:42](src/server/distributed.rs#L42)) | `parent_min_speed_response` | - | fixed-size or empty payload |
| 84 | ParentSpeedRatio | - | `server::ParentSpeedRatio` ([src/server/distributed.rs:49](src/server/distributed.rs#L49)) | `parent_speed_ratio_response` | - | fixed-size or empty payload |
| 86 | ParentInactivityTimeout | - | `server::ParentInactivityTimeout` ([src/server/distributed.rs:56](src/server/distributed.rs#L56)) | `parent_inactivity_timeout_response` | - | fixed-size or empty payload |
| 87 | SearchInactivityTimeout | - | `server::SearchInactivityTimeout` ([src/server/distributed.rs:63](src/server/distributed.rs#L63)) | `search_inactivity_timeout_response` | - | fixed-size or empty payload |
| 88 | MinimumParentsInCache | - | `server::MinimumParentsInCache` ([src/server/distributed.rs:70](src/server/distributed.rs#L70)) | `minimum_parents_in_cache_response` | - | fixed-size or empty payload |
| 90 | DistributedAliveInterval | - | `server::DistributedAliveInterval` ([src/server/distributed.rs:77](src/server/distributed.rs#L77)) | `distributed_alive_interval_response` | - | fixed-size or empty payload |
| 91 | AddPrivilegedUser | - | `server::AddPrivilegedUser` ([src/server/privileges.rs:15](src/server/privileges.rs#L15)) | `add_privileged_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 92 | CheckPrivileges | `ServerRequest::CheckPrivileges`, empty payload | `server::CheckPrivilegesResponse` ([src/server/privileges.rs:22](src/server/privileges.rs#L22)) | `check_privileges_request`, `check_privileges_response` | - | fixed-size or empty payload |
| 93 | EmbeddedMessage | - | `distributed::Embedded` ([src/distributed.rs:187](src/distributed.rs#L187)) | `embedded_message_response` | - | inner payload kept as raw bytes, decoded by DistributedMessage::decode on the inner frame |
| 100 | AcceptChildren | `server::AcceptChildren` ([src/server/distributed.rs:84](src/server/distributed.rs#L84)) | - | `accept_children_request` | - | fixed-size or empty payload |
| 102 | NetInfo | - | `server::NetInfo` ([src/server/distributed.rs:93](src/server/distributed.rs#L93)) | `net_info_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 103 | WishlistSearch | `server::WishlistSearch` ([src/server/search.rs:70](src/server/search.rs#L70)) | - | `wishlist_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 104 | WishlistInterval | - | `server::WishlistInterval` ([src/server/search.rs:78](src/server/search.rs#L78)) | `wishlist_interval_response` | - | fixed-size or empty payload |
| 110 | GetPersonalSimilarUsers | `ServerRequest::GetPersonalSimilarUsers`, empty payload | `server::GetPersonalSimilarUsersResponse` ([src/server/interests.rs:105](src/server/interests.rs#L105)) | `get_personal_similar_users_request`, `get_personal_similar_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 111 | GetInterestRecommendations | `server::GetInterestRecommendationsRequest` ([src/server/interests.rs:119](src/server/interests.rs#L119)) | `server::GetInterestRecommendationsResponse` ([src/server/interests.rs:126](src/server/interests.rs#L126)) | `get_interest_recommendations_request`, `get_interest_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 112 | GetInterestSimilarUsers | `server::GetInterestSimilarUsersRequest` ([src/server/interests.rs:134](src/server/interests.rs#L134)) | `server::GetInterestSimilarUsersResponse` ([src/server/interests.rs:141](src/server/interests.rs#L141)) | `get_interest_similar_users_request`, `get_interest_similar_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 113 | RoomTickers | - | `server::RoomTickers` ([src/server/rooms.rs:328](src/server/rooms.rs#L328)) | `room_tickers_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 114 | RoomTickerAdd | - | `server::RoomTickerAdd` ([src/server/rooms.rs:343](src/server/rooms.rs#L343)) | `room_ticker_add_response` | - | strings <= LIMIT_STRING_BYTES |
| 115 | RoomTickerRemove | - | `server::RoomTickerRemove` ([src/server/rooms.rs:351](src/server/rooms.rs#L351)) | `room_ticker_remove_response` | - | strings <= LIMIT_STRING_BYTES |
| 116 | SetRoomTicker | `server::SetRoomTicker` ([src/server/rooms.rs:359](src/server/rooms.rs#L359)) | - | `set_room_ticker_request` | - | strings <= LIMIT_STRING_BYTES |
| 117 | HatedInterestAdd | `server::HatedInterestAdd` ([src/server/interests.rs:149](src/server/interests.rs#L149)) | - | `hated_interest_add_request` | - | strings <= LIMIT_STRING_BYTES |
| 118 | HatedInterestRemove | `server::HatedInterestRemove` ([src/server/interests.rs:156](src/server/interests.rs#L156)) | - | `hated_interest_remove_request` | - | strings <= LIMIT_STRING_BYTES |
| 120 | RoomSearch | `server::RoomSearch` ([src/server/search.rs:85](src/server/search.rs#L85)) | - | `room_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 121 | SendUploadSpeed | `server::SendUploadSpeed` ([src/server/session.rs:173](src/server/session.rs#L173)) | - | `send_upload_speed_request` | - | fixed-size or empty payload |
| 122 | UserPrivileges | `server::UserPrivilegesRequest` ([src/server/privileges.rs:29](src/server/privileges.rs#L29)) | `server::UserPrivilegesResponse` ([src/server/privileges.rs:36](src/server/privileges.rs#L36)) | `user_privileges_request`, `user_privileges_response` | - | strings <= LIMIT_STRING_BYTES |
| 123 | GivePrivileges | `server::GivePrivileges` ([src/server/privileges.rs:44](src/server/privileges.rs#L44)) | - | `give_privileges_request` | - | strings <= LIMIT_STRING_BYTES |
| 124 | NotifyPrivileges | - | `server::NotifyPrivileges` ([src/server/privileges.rs:52](src/server/privileges.rs#L52)) | `notify_privileges_response` | - | strings <= LIMIT_STRING_BYTES |
| 125 | AcknowledgeNotifyPrivileges | `server::AcknowledgeNotifyPrivileges` ([src/server/privileges.rs:60](src/server/privileges.rs#L60)) | - | `acknowledge_notify_privileges_request` | - | fixed-size or empty payload |
| 126 | BranchLevel | `server::BranchLevel` ([src/server/distributed.rs:108](src/server/distributed.rs#L108)) | - | `branch_level_request` | - | fixed-size or empty payload |
| 127 | BranchRoot | `server::BranchRoot` ([src/server/distributed.rs:115](src/server/distributed.rs#L115)) | - | `branch_root_request` | - | strings <= LIMIT_STRING_BYTES |
| 129 | ChildDepth | `server::ChildDepth` ([src/server/distributed.rs:122](src/server/distributed.rs#L122)) | - | `child_depth_request` | - | fixed-size or empty payload |
| 130 | DistributedReset | - | `ServerResponse::DistributedReset`, empty payload | `distributed_reset_response` | - | fixed-size or empty payload |
| 133 | PrivateRoomUsers | - | `server::PrivateRoomUsers` ([src/server/private_rooms.rs:9](src/server/private_rooms.rs#L9)) | `private_room_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 134 | PrivateRoomAddUser | `server::PrivateRoomAddUser` ([src/server/private_rooms.rs:17](src/server/private_rooms.rs#L17)) | `server::PrivateRoomAddUser` ([src/server/private_rooms.rs:17](src/server/private_rooms.rs#L17)) | `private_room_add_user_request`, `private_room_add_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 135 | PrivateRoomRemoveUser | `server::PrivateRoomRemoveUser` ([src/server/private_rooms.rs:25](src/server/private_rooms.rs#L25)) | `server::PrivateRoomRemoveUser` ([src/server/private_rooms.rs:25](src/server/private_rooms.rs#L25)) | `private_room_remove_user_request`, `private_room_remove_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 136 | PrivateRoomDropMembership | `server::PrivateRoomDropMembership` ([src/server/private_rooms.rs:33](src/server/private_rooms.rs#L33)) | - | `private_room_drop_membership_request` | - | strings <= LIMIT_STRING_BYTES |
| 137 | PrivateRoomDropOwnership | `server::PrivateRoomDropOwnership` ([src/server/private_rooms.rs:40](src/server/private_rooms.rs#L40)) | - | `private_room_drop_ownership_request` | - | strings <= LIMIT_STRING_BYTES |
| 138 | PrivateRoomUnknown | `server::PrivateRoomUnknown` ([src/server/private_rooms.rs:47](src/server/private_rooms.rs#L47)) | `server::PrivateRoomUnknown` ([src/server/private_rooms.rs:47](src/server/private_rooms.rs#L47)) | `private_room_unknown_request`, `private_room_unknown_response` | - | strings <= LIMIT_STRING_BYTES |
| 139 | PrivateRoomAdded | - | `server::PrivateRoomAdded` ([src/server/private_rooms.rs:54](src/server/private_rooms.rs#L54)) | `private_room_added_response` | - | strings <= LIMIT_STRING_BYTES |
| 140 | PrivateRoomRemoved | - | `server::PrivateRoomRemoved` ([src/server/private_rooms.rs:61](src/server/private_rooms.rs#L61)) | `private_room_removed_response` | - | strings <= LIMIT_STRING_BYTES |
| 141 | PrivateRoomToggle | `server::PrivateRoomToggle` ([src/server/private_rooms.rs:68](src/server/private_rooms.rs#L68)) | `server::PrivateRoomToggle` ([src/server/private_rooms.rs:68](src/server/private_rooms.rs#L68)) | `private_room_toggle_request`, `private_room_toggle_response` | - | fixed-size or empty payload |
| 142 | NewPassword | `server::NewPassword` ([src/server/session.rs:181](src/server/session.rs#L181)) | `server::NewPassword` ([src/server/session.rs:181](src/server/session.rs#L181)) | `new_password_request`, `new_password_response` | - | strings <= LIMIT_STRING_BYTES |
| 143 | PrivateRoomAddOperator | `server::PrivateRoomAddOperator` ([src/server/private_rooms.rs:75](src/server/private_rooms.rs#L75)) | `server::PrivateRoomAddOperator` ([src/server/private_rooms.rs:75](src/server/private_rooms.rs#L75)) | `private_room_add_operator_request`, `private_room_add_operator_response` | - | strings <= LIMIT_STRING_BYTES |
| 144 | PrivateRoomRemoveOperator | `server::PrivateRoomRemoveOperator` ([src/server/private_rooms.rs:83](src/server/private_rooms.rs#L83)) | `server::PrivateRoomRemoveOperator` ([src/server/private_rooms.rs:83](src/server/private_rooms.rs#L83)) | `private_room_remove_operator_request`, `private_room_remove_operator_response` | - | strings <= LIMIT_STRING_BYTES |
| 145 | PrivateRoomOperatorAdded | - | `server::PrivateRoomOperatorAdded` ([src/server/private_rooms.rs:91](src/server/private_rooms.rs#L91)) | `private_room_operator_added_response` | - | strings <= LIMIT_STRING_BYTES |
| 146 | PrivateRoomOperatorRemoved | - | `server::PrivateRoomOperatorRemoved` ([src/server/private_rooms.rs:98](src/server/private_rooms.rs#L98)) | `private_room_operator_removed_response` | - | strings <= LIMIT_STRING_BYTES |
| 148 | PrivateRoomOwned | - | `server::PrivateRoomOwned` ([src/server/private_rooms.rs:107](src/server/private_rooms.rs#L107)) | `private_room_owned_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 149 | MessageUsers | `server::MessageUsers` ([src/server/messaging.rs:75](src/server/messaging.rs#L75)) | - | `message_users_request` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 150 | AskPublicChat | `ServerRequest::AskPublicChat`, empty payload | - | `ask_public_chat_request` | - | fixed-size or empty payload |
| 151 | StopPublicChat | `ServerRequest::StopPublicChat`, empty payload | - | `stop_public_chat_request` | - | fixed-size or empty payload |
| 152 | PublicChat | - | `server::PublicChat` ([src/server/messaging.rs:83](src/server/messaging.rs#L83)) | `public_chat_response` | - | strings <= LIMIT_STRING_BYTES |
| 153 | RelatedSearch | `server::RelatedSearchRequest` ([src/server/search.rs:94](src/server/search.rs#L94)) | `server::RelatedSearchResponse` ([src/server/search.rs:101](src/server/search.rs#L101)) | `related_search_request`, `related_search_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 160 | ExcludedSearchPhrases | - | `server::ExcludedSearchPhrases` ([src/server/search.rs:116](src/server/search.rs#L116)) | `excluded_search_phrases_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 1001 | CannotConnect | `server::CannotConnectRequest` ([src/server/peers.rs:87](src/server/peers.rs#L87)) | `server::CannotConnectResponse` ([src/server/peers.rs:103](src/server/peers.rs#L103)) | `cannot_connect_request`, `cannot_connect_response`, `cannot_connect_response_with_username` | - | strings <= LIMIT_STRING_BYTES |
| 1003 | CannotJoinRoom | - | `server::CannotJoinRoom` ([src/server/rooms.rs:370](src/server/rooms.rs#L370)) | `cannot_join_room_response` | - | strings <= LIMIT_STRING_BYTES |

## Peer init (§01 §4.4, §01a §1.2)

Frame length <= `LIMIT_INIT_MESSAGE_BYTES` on every row.

| Code | Name | Codec | Round trip | Also | Limits |
|---|---|---|---|---|---|
| 0 | PierceFirewall | `init::PierceFirewall` ([src/init.rs:30](src/init.rs#L30)) | `pierce_firewall` | - | fixed-size or empty payload |
| 1 | PeerInit | `init::PeerInit` ([src/init.rs:48](src/init.rs#L48)) | `peer_init` | `peer_init_keeps_a_non_zero_token`, `peer_init_rejects_an_unknown_type`, `peer_init_rejects_the_reserved_username`, `peer_init_rejects_an_empty_username`, `peer_init_rejects_a_frame_over_the_limit` | strings <= LIMIT_STRING_BYTES; username per §9 rule 6; unknown connection type rejected |

## Peer (§01 §4.2, §01a §1.3)

Frame length <= `LIMIT_PEER_MESSAGE_BYTES` on every row.

| Code | Name | Codec | Round trip | Also | Limits |
|---|---|---|---|---|---|
| 1 | PrivateMessage | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 4 | BrowseRequest | `PeerMessage::BrowseRequest`, empty payload | `browse_request` | - | fixed-size or empty payload |
| 5 | BrowseResponse | `peer::BrowseResponse` ([src/peer/browse.rs:189](src/peer/browse.rs#L189)) | `browse_response`, `browse_response_empty` | `browse_response_without_trailer`, `browse_response_fixes_ns_sign_extended_size`, `browse_response_rejects_a_stream_that_is_not_zlib`, `browse_response_directory_limit_counts_every_list` | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT (file >= 21 B, directory >= 8 B) |
| 8 | SearchRequest | `peer::SearchRequest` ([src/peer/search.rs:19](src/peer/search.rs#L19)) | `search_request` | - | strings <= LIMIT_STRING_BYTES |
| 9 | SearchResponse | `peer::SearchResponse` ([src/peer/search.rs:45](src/peer/search.rs#L45)) | `search_response` | `search_response_legacy_without_trailer`, `search_response_eight_byte_queue_length`, `search_response_ignores_a_stray_trailing_byte`, `search_response_rejects_too_many_attributes`, `search_response_rejects_a_file_count_past_the_body`, `search_response_peek_stops_before_the_files`, `search_response_honours_configured_limits` | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT (file >= 21 B); trailer read only with >= 4 B |
| 10 | PrivateRoomInvitation | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 14 | CancelledQueuedTransfer | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 15 | InfoRequest | `PeerMessage::InfoRequest`, empty payload | `info_request` | - | fixed-size or empty payload |
| 16 | InfoResponse | `peer::UserInfo` ([src/peer/info.rs:31](src/peer/info.rs#L31)) | `info_response`, `info_response_without_picture` | `info_response_ignores_museek_stray_bytes`, `info_response_rejects_a_picture_over_the_limit` | strings <= LIMIT_STRING_BYTES; description <= LIMIT_DESCRIPTION_BYTES, LIMIT_USER_INFO_PICTURE_BYTES |
| 33 | SendConnectToken | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 34 | MoveDownloadToTop | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 36 | FolderContentsRequest | `peer::FolderContentsRequest` ([src/peer/browse.rs:225](src/peer/browse.rs#L225)) | `folder_contents_request` | - | strings <= LIMIT_STRING_BYTES |
| 37 | FolderContentsResponse | `peer::FolderContentsResponse` ([src/peer/browse.rs:249](src/peer/browse.rs#L249)) | `folder_contents_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT |
| 40 | TransferRequest | `peer::TransferRequest` ([src/peer/transfers.rs:21](src/peer/transfers.rs#L21)) | `transfer_request_download`, `transfer_request_upload` | `transfer_request_download_ignores_a_stray_size`, `transfer_request_upload_without_size` | strings <= LIMIT_STRING_BYTES |
| 41 | TransferResponse | `peer::TransferResponse` ([src/peer/transfers.rs:81](src/peer/transfers.rs#L81)) | `transfer_response_allowed_with_size`, `transfer_response_allowed`, `transfer_response_denied` | `transfer_response_allowed_must_be_exactly_one`, `transfer_response_denied_needs_a_reason` | strings <= LIMIT_STRING_BYTES |
| 42 | UploadPlacehold | `peer::UploadPlacehold` ([src/peer/transfers.rs:128](src/peer/transfers.rs#L128)) | `upload_placehold` | - | strings <= LIMIT_STRING_BYTES |
| 43 | QueueDownload | `peer::QueueDownload` ([src/peer/transfers.rs:135](src/peer/transfers.rs#L135)) | `queue_download`, `queue_download_latin1` | `queue_download_latin1_filename_is_echoed_with_its_bytes` | strings <= LIMIT_STRING_BYTES |
| 44 | PlaceInQueueResponse | `peer::PlaceInQueueResponse` ([src/peer/transfers.rs:142](src/peer/transfers.rs#L142)) | `place_in_queue_response` | - | strings <= LIMIT_STRING_BYTES |
| 46 | UploadFailed | `peer::UploadFailed` ([src/peer/transfers.rs:151](src/peer/transfers.rs#L151)) | `upload_failed` | - | strings <= LIMIT_STRING_BYTES |
| 47 | ExactFileSearchRequest | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 48 | QueuedDownloads | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 49 | IndirectFileSearchRequest | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 50 | UploadDenied | `peer::UploadDenied` ([src/peer/transfers.rs:158](src/peer/transfers.rs#L158)) | `upload_denied` | - | strings <= LIMIT_STRING_BYTES |
| 51 | PlaceInQueueRequest | `peer::PlaceInQueueRequest` ([src/peer/transfers.rs:166](src/peer/transfers.rs#L166)) | `place_in_queue_request` | - | strings <= LIMIT_STRING_BYTES |
| 52 | UploadQueueNotification | `PeerMessage::UploadQueueNotification`, empty payload | `upload_queue_notification` | - | fixed-size or empty payload |

## Distributed (§01 §4.3, §01a §1.5)

Frame length <= `LIMIT_DISTRIBUTED_MESSAGE_BYTES` on every row.

| Code | Name | Codec | Round trip | Also | Limits |
|---|---|---|---|---|---|
| 0 | Ping | `distributed::Ping` ([src/distributed.rs:45](src/distributed.rs#L45)) | `ping`, `ping_with_token` | - | fixed-size or empty payload |
| 3 | SearchRequest | `distributed::SearchRequest` ([src/distributed.rs:76](src/distributed.rs#L76)) | `search_request` | `search_request_rejects_an_identifier_other_than_49` | strings <= LIMIT_STRING_BYTES; identifier must be 49 |
| 4 | BranchLevel | `distributed::BranchLevel` ([src/distributed.rs:114](src/distributed.rs#L114)) | `branch_level` | `branch_level_rejects_a_negative_level` | negative level rejected |
| 5 | BranchRoot | `distributed::BranchRoot` ([src/distributed.rs:136](src/distributed.rs#L136)) | `branch_root` | `branch_root_rejects_a_non_printable_username` | strings <= LIMIT_STRING_BYTES; username per §9 rule 6 |
| 6 | Unknown | n/a: placeholder of the original library, not a wire code; decodes as `UnknownCode` | n/a | `unknown_placeholder_code` | n/a |
| 7 | ChildDepth | `distributed::ChildDepth` ([src/distributed.rs:160](src/distributed.rs#L160)) | `child_depth` | - | fixed-size or empty payload |
| 93 | EmbeddedMessage | `distributed::EmbeddedMessage` ([src/distributed.rs:214](src/distributed.rs#L214)) | `embedded_message`, `embedded_message_padded` | `embedded_message_with_a_short_zero_prefix_is_not_padded`, `embedded_message_inner_frame_decodes_as_distributed` | inner payload kept as raw bytes, decoded by DistributedMessage::decode on the inner frame |

## File connection (§01 §3.3, §01a §1.4)

| Message | Codec | Round trip | Limits |
|---|---|---|---|
| FileTransferInit | `init::FileTransferInit` ([src/init.rs:81](src/init.rs#L81)) | `file_transfer_init` | exactly 4 bytes |
| FileOffset | `init::FileOffset` ([src/init.rs:101](src/init.rs#L101)) | `file_offset` | exactly 8 bytes; `offset > file size` (§9 rule 5) needs the file size, so the transfer layer refuses it |

## Not covered

- Type-1 ("rotated") obfuscation: the spec gives no algorithm, so there is no codec and no
  `obfuscation` fuzz target. Pinned as low priority in TODO.md under A1. The obfuscation fields
  of `SetListenPort`, `GetPeerAddress` and `ConnectToPeer` are decoded and covered above.
