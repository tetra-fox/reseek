# soulseek-proto coverage

One row per message code of §01 §4 and §01a §1. Every message is a struct in its family module
(`server`, `peer`, `distributed`, `init`) whose `Wire` impl reads and writes its layout in one
place; "Codec" names that struct and where it is. Encoding goes through the family enum's
`encode`, decoding through its `decode` (`PeerMessage::decode_with` for configured limits).
Server codes have a request codec (`ServerRequest`, client to server) and a response codec
(`ServerResponse`, server to client) where that direction exists.

"Round trip" lists the byte-exact tests and "Also" the compat, rejection and other tests for
that message; every test is in the family module's `tests` and named after the message.
Every family is also covered by the property tests in `src/proptests.rs` and by a fuzz target.
"Limits" names what the decoder enforces beyond the frame length, which is checked on every row.

## Server (§01 §4.1, §01a §1.1)

Frame length <= `LIMIT_SERVER_MESSAGE_BYTES` on every row.

| Code | Name | Request codec | Response codec | Round trip | Also | Limits |
|---|---|---|---|---|---|---|
| 0 | Unknown | n/a: not a wire code | n/a: decodes as `UnknownCode` | n/a | `unknown_code_response` | n/a |
| 1 | Login | `server::LoginRequest` ([src/server.rs:446](src/server.rs#L446)) | `server::LoginResponse` ([src/server.rs:490](src/server.rs#L490)) | `login_request`, `login_response_success`, `login_response_failure`, `login_response_failure_with_detail` | `login_response_success_must_be_exactly_one`, `login_hash_is_md5_of_username_and_password`, `login_request_debug_never_shows_credentials` | strings <= LIMIT_STRING_BYTES; greeting <= LIMIT_DESCRIPTION_BYTES |
| 2 | SetListenPort | `server::SetListenPort` ([src/server.rs:547](src/server.rs#L547)) | - | `set_listen_port_request`, `set_listen_port_request_obfuscated` | `set_listen_port_response_is_not_a_server_message` | fixed-size or empty payload |
| 3 | GetPeerAddress | `server::GetPeerAddressRequest` ([src/server.rs:571](src/server.rs#L571)) | `server::GetPeerAddressResponse` ([src/server.rs:587](src/server.rs#L587)) | `get_peer_address_request`, `get_peer_address_response`, `get_peer_address_response_without_trailer` | `get_peer_address_response_ignores_a_partial_trailer` | strings <= LIMIT_STRING_BYTES |
| 5 | WatchUser | `server::WatchUserRequest` ([src/server.rs:620](src/server.rs#L620)) | `server::WatchUserResponse` ([src/server.rs:642](src/server.rs#L642)) | `watch_user_request`, `watch_user_response`, `watch_user_response_blank_country`, `watch_user_response_offline`, `watch_user_response_missing` | - | strings <= LIMIT_STRING_BYTES |
| 6 | UnwatchUser | `server::UnwatchUser` ([src/server.rs:681](src/server.rs#L681)) | - | `unwatch_user_request` | - | strings <= LIMIT_STRING_BYTES |
| 7 | GetStatus | `server::GetStatusRequest` ([src/server.rs:687](src/server.rs#L687)) | `server::UserStatus` ([src/server.rs:694](src/server.rs#L694)) | `get_status_request`, `get_status_response` | `get_status_response_bool_accepts_any_non_zero`, `get_status_response_rejects_unknown_presence` | strings <= LIMIT_STRING_BYTES |
| 11 | IgnoreUser | `server::IgnoreUser` ([src/server.rs:703](src/server.rs#L703)) | `server::IgnoreUser` ([src/server.rs:703](src/server.rs#L703)) | `ignore_user_request`, `ignore_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 12 | UnignoreUser | `server::UnignoreUser` ([src/server.rs:710](src/server.rs#L710)) | `server::UnignoreUser` ([src/server.rs:710](src/server.rs#L710)) | `unignore_user_request`, `unignore_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 13 | SayInChatRoom | `server::SayInChatRoomRequest` ([src/server.rs:716](src/server.rs#L716)) | `server::SayInChatRoomResponse` ([src/server.rs:723](src/server.rs#L723)) | `say_in_chat_room_request`, `say_in_chat_room_response` | - | strings <= LIMIT_STRING_BYTES |
| 14 | JoinRoom | `server::JoinRoomRequest` ([src/server.rs:731](src/server.rs#L731)) | `server::RoomData` ([src/server.rs:766](src/server.rs#L766)) | `join_room_request`, `join_room_response`, `join_room_response_private` | `join_room_response_rejects_mismatched_lists` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; five parallel counts must be equal (§9 rule 4) |
| 15 | LeaveRoom | `server::LeaveRoom` ([src/server.rs:804](src/server.rs#L804)) | `server::LeaveRoom` ([src/server.rs:804](src/server.rs#L804)) | `leave_room_request`, `leave_room_response` | `leave_room_response_ignores_trailing_fields`, `leave_room_response_rejects_a_string_past_the_payload`, `leave_room_response_rejects_a_cut_frame` | strings <= LIMIT_STRING_BYTES |
| 16 | UserJoinedRoom | - | `server::UserJoinedRoom` ([src/server.rs:817](src/server.rs#L817)) | `user_joined_room_response` | - | strings <= LIMIT_STRING_BYTES |
| 17 | UserLeftRoom | - | `server::UserLeftRoom` ([src/server.rs:850](src/server.rs#L850)) | `user_left_room_response` | - | strings <= LIMIT_STRING_BYTES |
| 18 | ConnectToPeer | `server::ConnectToPeerRequest` ([src/server.rs:857](src/server.rs#L857)) | `server::ConnectToPeerResponse` ([src/server.rs:880](src/server.rs#L880)) | `connect_to_peer_request`, `connect_to_peer_response`, `connect_to_peer_response_without_trailer` | `connect_to_peer_response_rejects_unknown_type` | strings <= LIMIT_STRING_BYTES |
| 22 | PrivateMessage | `server::PrivateMessageRequest` ([src/server.rs:919](src/server.rs#L919)) | `server::PrivateMessageResponse` ([src/server.rs:938](src/server.rs#L938)) | `private_message_request`, `private_message_response`, `private_message_response_replayed` | `private_message_response_is_new_must_be_exactly_one` | strings <= LIMIT_STRING_BYTES |
| 23 | AcknowledgePrivateMessage | `server::AcknowledgePrivateMessage` ([src/server.rs:967](src/server.rs#L967)) | - | `acknowledge_private_message_request` | - | fixed-size or empty payload |
| 25 | FileSearchRoom | `server::FileSearchRoom` ([src/server.rs:974](src/server.rs#L974)) | - | `file_search_room_request` | - | strings <= LIMIT_STRING_BYTES |
| 26 | FileSearch | `server::FileSearchRequest` ([src/server.rs:982](src/server.rs#L982)) | `server::FileSearchResponse` ([src/server.rs:990](src/server.rs#L990)) | `file_search_request`, `file_search_response` | - | strings <= LIMIT_STRING_BYTES |
| 28 | SetOnlineStatus | `server::SetOnlineStatus` ([src/server.rs:998](src/server.rs#L998)) | - | `set_online_status_request` | - | fixed-size or empty payload |
| 32 | Ping | `ServerRequest::Ping`, empty payload | `ServerResponse::Ping`, empty payload | `ping_request`, `ping_response` | `ping_response_rejects_bytes_after_the_frame` | fixed-size or empty payload |
| 33 | SendConnectToken | `server::SendConnectToken` ([src/server.rs:1005](src/server.rs#L1005)) | `server::SendConnectToken` ([src/server.rs:1005](src/server.rs#L1005)) | `send_connect_token_request`, `send_connect_token_response` | - | strings <= LIMIT_STRING_BYTES |
| 34 | SendSpeed | `server::SendSpeed` ([src/server.rs:1013](src/server.rs#L1013)) | - | `send_speed_request` | - | strings <= LIMIT_STRING_BYTES |
| 35 | SharedFoldersAndFiles | `server::SharedFoldersAndFiles` ([src/server.rs:1020](src/server.rs#L1020)) | - | `shared_folders_and_files_request` | - | fixed-size or empty payload |
| 36 | GetUserStats | `server::GetUserStatsRequest` ([src/server.rs:1027](src/server.rs#L1027)) | `server::UserStatistics` ([src/server.rs:1034](src/server.rs#L1034)) | `get_user_stats_request`, `get_user_stats_response` | - | strings <= LIMIT_STRING_BYTES |
| 40 | QueuedDownloads | `server::QueuedDownloadsRequest` ([src/server.rs:1047](src/server.rs#L1047)) | `server::QueuedDownloadsResponse` ([src/server.rs:1068](src/server.rs#L1068)) | `queued_downloads_request`, `queued_downloads_response` | - | strings <= LIMIT_STRING_BYTES |
| 41 | KickedFromServer | - | `ServerResponse::KickedFromServer`, empty payload | `kicked_from_server_response` | - | fixed-size or empty payload |
| 42 | UserSearch | `server::UserSearch` ([src/server.rs:1089](src/server.rs#L1089)) | - | `user_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 50 | SimilarRecommendations | `server::SimilarRecommendationsRequest` ([src/server.rs:1098](src/server.rs#L1098)) | `server::SimilarRecommendationsResponse` ([src/server.rs:1105](src/server.rs#L1105)) | `similar_recommendations_request`, `similar_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 51 | LikedInterestAdd | `server::LikedInterestAdd` ([src/server.rs:1112](src/server.rs#L1112)) | - | `liked_interest_add_request` | - | strings <= LIMIT_STRING_BYTES |
| 52 | LikedInterestRemove | `server::LikedInterestRemove` ([src/server.rs:1118](src/server.rs#L1118)) | - | `liked_interest_remove_request` | - | strings <= LIMIT_STRING_BYTES |
| 54 | GetPersonalRecommendations | `ServerRequest::GetPersonalRecommendations`, empty payload | `server::Recommendations` ([src/server.rs:301](src/server.rs#L301)) | `get_personal_recommendations_request`, `get_personal_recommendations_response` | `get_personal_recommendations_response_without_second_block` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 55 | MyRecommendations | `ServerRequest::MyRecommendations`, empty payload | `server::MyRecommendationsResponse` ([src/server.rs:1125](src/server.rs#L1125)) | `my_recommendations_request`, `my_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 56 | GetGlobalRecommendations | `ServerRequest::GetGlobalRecommendations`, empty payload | `server::Recommendations` ([src/server.rs:301](src/server.rs#L301)) | `get_global_recommendations_request`, `get_global_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 57 | GetUserInterests | `server::GetUserInterestsRequest` ([src/server.rs:1131](src/server.rs#L1131)) | `server::UserInterests` ([src/server.rs:1138](src/server.rs#L1138)) | `get_user_interests_request`, `get_user_interests_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 58 | AdminCommand | `server::AdminCommand` ([src/server.rs:1147](src/server.rs#L1147)) | - | `admin_command_request` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 59 | PlaceInLineRequest | `server::PlaceInLineRequest` ([src/server.rs:1155](src/server.rs#L1155)) | `server::PlaceInLineRequest` ([src/server.rs:1155](src/server.rs#L1155)) | `place_in_line_request_request`, `place_in_line_request_response` | - | strings <= LIMIT_STRING_BYTES |
| 60 | PlaceInLineResponse | `server::PlaceInLineResponse` ([src/server.rs:1163](src/server.rs#L1163)) | `server::PlaceInLineResponse` ([src/server.rs:1163](src/server.rs#L1163)) | `place_in_line_response_request`, `place_in_line_response_response` | - | strings <= LIMIT_STRING_BYTES |
| 62 | RoomAdded | - | `server::RoomAdded` ([src/server.rs:1172](src/server.rs#L1172)) | `room_added_response` | - | strings <= LIMIT_STRING_BYTES |
| 63 | RoomRemoved | - | `server::RoomRemoved` ([src/server.rs:1179](src/server.rs#L1179)) | `room_removed_response` | - | strings <= LIMIT_STRING_BYTES |
| 64 | RoomList | `ServerRequest::RoomList`, empty payload | `server::RoomList` ([src/server.rs:1198](src/server.rs#L1198)) | `room_list_request`, `room_list_response` | `room_list_response_rejects_mismatched_lists`, `room_list_response_rejects_a_count_past_the_payload` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; name and count lists must be equal (§9 rule 4) |
| 65 | ExactFileSearch | `server::ExactFileSearchRequest` ([src/server.rs:1224](src/server.rs#L1224)) | `server::ExactFileSearchResponse` ([src/server.rs:1236](src/server.rs#L1236)) | `exact_file_search_request`, `exact_file_search_response` | - | strings <= LIMIT_STRING_BYTES |
| 66 | GlobalAdminMessage | - | `server::GlobalAdminMessage` ([src/server.rs:1247](src/server.rs#L1247)) | `global_admin_message_response` | - | strings <= LIMIT_STRING_BYTES |
| 67 | GlobalUserList | `ServerRequest::GlobalUserList`, empty payload | `server::GlobalUserListResponse` ([src/server.rs:1259](src/server.rs#L1259)) | `global_user_list_request`, `global_user_list_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; five parallel counts must be equal (§9 rule 4) |
| 68 | TunneledMessage | `server::TunneledMessageRequest` ([src/server.rs:1279](src/server.rs#L1279)) | `server::TunneledMessageResponse` ([src/server.rs:1289](src/server.rs#L1289)) | `tunneled_message_request`, `tunneled_message_response` | - | strings <= LIMIT_STRING_BYTES |
| 69 | PrivilegedUsers | - | `server::PrivilegedUsers` ([src/server.rs:1300](src/server.rs#L1300)) | `privileged_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 71 | HaveNoParents | `server::HaveNoParents` ([src/server.rs:1306](src/server.rs#L1306)) | - | `have_no_parents_request` | - | fixed-size or empty payload |
| 73 | ParentsIP | `server::ParentsIP` ([src/server.rs:1318](src/server.rs#L1318)) | - | `parents_ip_request`, `parents_ip_request_without_parent` | - | fixed-size or empty payload |
| 83 | ParentMinSpeed | - | `server::ParentMinSpeed` ([src/server.rs:1335](src/server.rs#L1335)) | `parent_min_speed_response` | - | fixed-size or empty payload |
| 84 | ParentSpeedRatio | - | `server::ParentSpeedRatio` ([src/server.rs:1341](src/server.rs#L1341)) | `parent_speed_ratio_response` | - | fixed-size or empty payload |
| 86 | ParentInactivityTimeout | - | `server::ParentInactivityTimeout` ([src/server.rs:1348](src/server.rs#L1348)) | `parent_inactivity_timeout_response` | - | fixed-size or empty payload |
| 87 | SearchInactivityTimeout | - | `server::SearchInactivityTimeout` ([src/server.rs:1355](src/server.rs#L1355)) | `search_inactivity_timeout_response` | - | fixed-size or empty payload |
| 88 | MinimumParentsInCache | - | `server::MinimumParentsInCache` ([src/server.rs:1362](src/server.rs#L1362)) | `minimum_parents_in_cache_response` | - | fixed-size or empty payload |
| 90 | DistributedAliveInterval | - | `server::DistributedAliveInterval` ([src/server.rs:1369](src/server.rs#L1369)) | `distributed_alive_interval_response` | - | fixed-size or empty payload |
| 91 | AddPrivilegedUser | - | `server::AddPrivilegedUser` ([src/server.rs:1376](src/server.rs#L1376)) | `add_privileged_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 92 | CheckPrivileges | `ServerRequest::CheckPrivileges`, empty payload | `server::CheckPrivilegesResponse` ([src/server.rs:1382](src/server.rs#L1382)) | `check_privileges_request`, `check_privileges_response` | - | fixed-size or empty payload |
| 93 | EmbeddedMessage | - | `distributed::Embedded` ([src/distributed.rs:177](src/distributed.rs#L177)) | `embedded_message_response` | - | inner payload kept as raw bytes, decoded by DistributedMessage::decode on the inner frame |
| 100 | AcceptChildren | `server::AcceptChildren` ([src/server.rs:1388](src/server.rs#L1388)) | - | `accept_children_request` | - | fixed-size or empty payload |
| 102 | NetInfo | - | `server::NetInfo` ([src/server.rs:1395](src/server.rs#L1395)) | `net_info_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 103 | WishlistSearch | `server::WishlistSearch` ([src/server.rs:1409](src/server.rs#L1409)) | - | `wishlist_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 104 | WishlistInterval | - | `server::WishlistInterval` ([src/server.rs:1416](src/server.rs#L1416)) | `wishlist_interval_response` | - | fixed-size or empty payload |
| 110 | GetPersonalSimilarUsers | `ServerRequest::GetPersonalSimilarUsers`, empty payload | `server::GetPersonalSimilarUsersResponse` ([src/server.rs:1422](src/server.rs#L1422)) | `get_personal_similar_users_request`, `get_personal_similar_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 111 | GetInterestRecommendations | `server::GetInterestRecommendationsRequest` ([src/server.rs:1435](src/server.rs#L1435)) | `server::GetInterestRecommendationsResponse` ([src/server.rs:1441](src/server.rs#L1441)) | `get_interest_recommendations_request`, `get_interest_recommendations_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 112 | GetInterestSimilarUsers | `server::GetInterestSimilarUsersRequest` ([src/server.rs:1448](src/server.rs#L1448)) | `server::GetInterestSimilarUsersResponse` ([src/server.rs:1454](src/server.rs#L1454)) | `get_interest_similar_users_request`, `get_interest_similar_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 113 | RoomTickers | - | `server::RoomTickers` ([src/server.rs:1462](src/server.rs#L1462)) | `room_tickers_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 114 | RoomTickerAdd | - | `server::RoomTickerAdd` ([src/server.rs:1476](src/server.rs#L1476)) | `room_ticker_add_response` | - | strings <= LIMIT_STRING_BYTES |
| 115 | RoomTickerRemove | - | `server::RoomTickerRemove` ([src/server.rs:1483](src/server.rs#L1483)) | `room_ticker_remove_response` | - | strings <= LIMIT_STRING_BYTES |
| 116 | SetRoomTicker | `server::SetRoomTicker` ([src/server.rs:1490](src/server.rs#L1490)) | - | `set_room_ticker_request` | - | strings <= LIMIT_STRING_BYTES |
| 117 | HatedInterestAdd | `server::HatedInterestAdd` ([src/server.rs:1498](src/server.rs#L1498)) | - | `hated_interest_add_request` | - | strings <= LIMIT_STRING_BYTES |
| 118 | HatedInterestRemove | `server::HatedInterestRemove` ([src/server.rs:1504](src/server.rs#L1504)) | - | `hated_interest_remove_request` | - | strings <= LIMIT_STRING_BYTES |
| 120 | RoomSearch | `server::RoomSearch` ([src/server.rs:1510](src/server.rs#L1510)) | - | `room_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 121 | SendUploadSpeed | `server::SendUploadSpeed` ([src/server.rs:1518](src/server.rs#L1518)) | - | `send_upload_speed_request` | - | fixed-size or empty payload |
| 122 | UserPrivileges | `server::UserPrivilegesRequest` ([src/server.rs:1526](src/server.rs#L1526)) | `server::UserPrivilegesResponse` ([src/server.rs:1533](src/server.rs#L1533)) | `user_privileges_request`, `user_privileges_response` | - | strings <= LIMIT_STRING_BYTES |
| 123 | GivePrivileges | `server::GivePrivileges` ([src/server.rs:1540](src/server.rs#L1540)) | - | `give_privileges_request` | - | strings <= LIMIT_STRING_BYTES |
| 124 | NotifyPrivileges | - | `server::NotifyPrivileges` ([src/server.rs:1548](src/server.rs#L1548)) | `notify_privileges_response` | - | strings <= LIMIT_STRING_BYTES |
| 125 | AcknowledgeNotifyPrivileges | `server::AcknowledgeNotifyPrivileges` ([src/server.rs:1556](src/server.rs#L1556)) | - | `acknowledge_notify_privileges_request` | - | fixed-size or empty payload |
| 126 | BranchLevel | `server::BranchLevel` ([src/server.rs:1562](src/server.rs#L1562)) | - | `branch_level_request` | - | fixed-size or empty payload |
| 127 | BranchRoot | `server::BranchRoot` ([src/server.rs:1568](src/server.rs#L1568)) | - | `branch_root_request` | - | strings <= LIMIT_STRING_BYTES |
| 129 | ChildDepth | `server::ChildDepth` ([src/server.rs:1575](src/server.rs#L1575)) | - | `child_depth_request` | - | fixed-size or empty payload |
| 130 | DistributedReset | - | `ServerResponse::DistributedReset`, empty payload | `distributed_reset_response` | - | fixed-size or empty payload |
| 133 | PrivateRoomUsers | - | `server::PrivateRoomUsers` ([src/server.rs:1582](src/server.rs#L1582)) | `private_room_users_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 134 | PrivateRoomAddUser | `server::PrivateRoomAddUser` ([src/server.rs:1589](src/server.rs#L1589)) | `server::PrivateRoomAddUser` ([src/server.rs:1589](src/server.rs#L1589)) | `private_room_add_user_request`, `private_room_add_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 135 | PrivateRoomRemoveUser | `server::PrivateRoomRemoveUser` ([src/server.rs:1596](src/server.rs#L1596)) | `server::PrivateRoomRemoveUser` ([src/server.rs:1596](src/server.rs#L1596)) | `private_room_remove_user_request`, `private_room_remove_user_response` | - | strings <= LIMIT_STRING_BYTES |
| 136 | PrivateRoomDropMembership | `server::PrivateRoomDropMembership` ([src/server.rs:1603](src/server.rs#L1603)) | - | `private_room_drop_membership_request` | - | strings <= LIMIT_STRING_BYTES |
| 137 | PrivateRoomDropOwnership | `server::PrivateRoomDropOwnership` ([src/server.rs:1609](src/server.rs#L1609)) | - | `private_room_drop_ownership_request` | - | strings <= LIMIT_STRING_BYTES |
| 138 | PrivateRoomUnknown | `server::PrivateRoomUnknown` ([src/server.rs:1616](src/server.rs#L1616)) | `server::PrivateRoomUnknown` ([src/server.rs:1616](src/server.rs#L1616)) | `private_room_unknown_request`, `private_room_unknown_response` | - | strings <= LIMIT_STRING_BYTES |
| 139 | PrivateRoomAdded | - | `server::PrivateRoomAdded` ([src/server.rs:1622](src/server.rs#L1622)) | `private_room_added_response` | - | strings <= LIMIT_STRING_BYTES |
| 140 | PrivateRoomRemoved | - | `server::PrivateRoomRemoved` ([src/server.rs:1628](src/server.rs#L1628)) | `private_room_removed_response` | - | strings <= LIMIT_STRING_BYTES |
| 141 | PrivateRoomToggle | `server::PrivateRoomToggle` ([src/server.rs:1634](src/server.rs#L1634)) | `server::PrivateRoomToggle` ([src/server.rs:1634](src/server.rs#L1634)) | `private_room_toggle_request`, `private_room_toggle_response` | - | fixed-size or empty payload |
| 142 | NewPassword | `server::NewPassword` ([src/server.rs:1640](src/server.rs#L1640)) | `server::NewPassword` ([src/server.rs:1640](src/server.rs#L1640)) | `new_password_request`, `new_password_response` | - | strings <= LIMIT_STRING_BYTES |
| 143 | PrivateRoomAddOperator | `server::PrivateRoomAddOperator` ([src/server.rs:1646](src/server.rs#L1646)) | `server::PrivateRoomAddOperator` ([src/server.rs:1646](src/server.rs#L1646)) | `private_room_add_operator_request`, `private_room_add_operator_response` | - | strings <= LIMIT_STRING_BYTES |
| 144 | PrivateRoomRemoveOperator | `server::PrivateRoomRemoveOperator` ([src/server.rs:1653](src/server.rs#L1653)) | `server::PrivateRoomRemoveOperator` ([src/server.rs:1653](src/server.rs#L1653)) | `private_room_remove_operator_request`, `private_room_remove_operator_response` | - | strings <= LIMIT_STRING_BYTES |
| 145 | PrivateRoomOperatorAdded | - | `server::PrivateRoomOperatorAdded` ([src/server.rs:1660](src/server.rs#L1660)) | `private_room_operator_added_response` | - | strings <= LIMIT_STRING_BYTES |
| 146 | PrivateRoomOperatorRemoved | - | `server::PrivateRoomOperatorRemoved` ([src/server.rs:1666](src/server.rs#L1666)) | `private_room_operator_removed_response` | - | strings <= LIMIT_STRING_BYTES |
| 148 | PrivateRoomOwned | - | `server::PrivateRoomOwned` ([src/server.rs:1673](src/server.rs#L1673)) | `private_room_owned_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 149 | MessageUsers | `server::MessageUsers` ([src/server.rs:1681](src/server.rs#L1681)) | - | `message_users_request` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 150 | AskPublicChat | `ServerRequest::AskPublicChat`, empty payload | - | `ask_public_chat_request` | - | fixed-size or empty payload |
| 151 | StopPublicChat | `ServerRequest::StopPublicChat`, empty payload | - | `stop_public_chat_request` | - | fixed-size or empty payload |
| 152 | PublicChat | - | `server::PublicChat` ([src/server.rs:1688](src/server.rs#L1688)) | `public_chat_response` | - | strings <= LIMIT_STRING_BYTES |
| 153 | RelatedSearch | `server::RelatedSearchRequest` ([src/server.rs:1697](src/server.rs#L1697)) | `server::RelatedSearchResponse` ([src/server.rs:1704](src/server.rs#L1704)) | `related_search_request`, `related_search_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 160 | ExcludedSearchPhrases | - | `server::ExcludedSearchPhrases` ([src/server.rs:1718](src/server.rs#L1718)) | `excluded_search_phrases_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 1001 | CannotConnect | `server::CannotConnectRequest` ([src/server.rs:1724](src/server.rs#L1724)) | `server::CannotConnectResponse` ([src/server.rs:1739](src/server.rs#L1739)) | `cannot_connect_request`, `cannot_connect_response`, `cannot_connect_response_with_username` | - | strings <= LIMIT_STRING_BYTES |
| 1003 | CannotJoinRoom | - | `server::CannotJoinRoom` ([src/server.rs:1768](src/server.rs#L1768)) | `cannot_join_room_response` | - | strings <= LIMIT_STRING_BYTES |

## Peer init (§01 §4.4, §01a §1.2)

Frame length <= `LIMIT_INIT_MESSAGE_BYTES` on every row.

| Code | Name | Codec | Round trip | Also | Limits |
|---|---|---|---|---|---|
| 0 | PierceFirewall | `init::PierceFirewall` ([src/init.rs:29](src/init.rs#L29)) | `pierce_firewall` | - | fixed-size or empty payload |
| 1 | PeerInit | `init::PeerInit` ([src/init.rs:46](src/init.rs#L46)) | `peer_init` | `peer_init_keeps_a_non_zero_token`, `peer_init_rejects_an_unknown_type`, `peer_init_rejects_the_reserved_username`, `peer_init_rejects_an_empty_username`, `peer_init_rejects_a_frame_over_the_limit` | strings <= LIMIT_STRING_BYTES; username per §9 rule 6; unknown connection type rejected |

## Peer (§01 §4.2, §01a §1.3)

Frame length <= `LIMIT_PEER_MESSAGE_BYTES` on every row.

| Code | Name | Codec | Round trip | Also | Limits |
|---|---|---|---|---|---|
| 1 | PrivateMessage | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 4 | BrowseRequest | `PeerMessage::BrowseRequest`, empty payload | `browse_request` | - | fixed-size or empty payload |
| 5 | BrowseResponse | `peer::BrowseResponse` ([src/peer.rs:324](src/peer.rs#L324)) | `browse_response`, `browse_response_empty` | `browse_response_without_trailer`, `browse_response_fixes_ns_sign_extended_size`, `browse_response_rejects_a_stream_that_is_not_zlib`, `browse_response_directory_limit_counts_every_list` | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT (file >= 21 B, directory >= 8 B) |
| 8 | SearchRequest | `peer::SearchRequest` ([src/peer.rs:360](src/peer.rs#L360)) | `search_request` | - | strings <= LIMIT_STRING_BYTES |
| 9 | SearchResponse | `peer::SearchResponse` ([src/peer.rs:384](src/peer.rs#L384)) | `search_response` | `search_response_legacy_without_trailer`, `search_response_eight_byte_queue_length`, `search_response_ignores_a_stray_trailing_byte`, `search_response_rejects_too_many_attributes`, `search_response_rejects_a_file_count_past_the_body`, `search_response_peek_stops_before_the_files`, `search_response_honours_configured_limits` | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT (file >= 21 B); trailer read only with >= 4 B |
| 10 | PrivateRoomInvitation | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 14 | CancelledQueuedTransfer | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 15 | InfoRequest | `PeerMessage::InfoRequest`, empty payload | `info_request` | - | fixed-size or empty payload |
| 16 | InfoResponse | `peer::UserInfo` ([src/peer.rs:457](src/peer.rs#L457)) | `info_response`, `info_response_without_picture` | `info_response_ignores_museek_stray_bytes`, `info_response_rejects_a_picture_over_the_limit` | strings <= LIMIT_STRING_BYTES; description <= LIMIT_DESCRIPTION_BYTES, LIMIT_USER_INFO_PICTURE_BYTES |
| 33 | SendConnectToken | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 34 | MoveDownloadToTop | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 36 | FolderContentsRequest | `peer::FolderContentsRequest` ([src/peer.rs:507](src/peer.rs#L507)) | `folder_contents_request` | - | strings <= LIMIT_STRING_BYTES |
| 37 | FolderContentsResponse | `peer::FolderContentsResponse` ([src/peer.rs:529](src/peer.rs#L529)) | `folder_contents_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT |
| 40 | TransferRequest | `peer::TransferRequest` ([src/peer.rs:568](src/peer.rs#L568)) | `transfer_request_download`, `transfer_request_upload` | `transfer_request_download_ignores_a_stray_size`, `transfer_request_upload_without_size` | strings <= LIMIT_STRING_BYTES |
| 41 | TransferResponse | `peer::TransferResponse` ([src/peer.rs:627](src/peer.rs#L627)) | `transfer_response_allowed_with_size`, `transfer_response_allowed`, `transfer_response_denied` | `transfer_response_allowed_must_be_exactly_one`, `transfer_response_denied_needs_a_reason` | strings <= LIMIT_STRING_BYTES |
| 42 | UploadPlacehold | `peer::UploadPlacehold` ([src/peer.rs:674](src/peer.rs#L674)) | `upload_placehold` | - | strings <= LIMIT_STRING_BYTES |
| 43 | QueueDownload | `peer::QueueDownload` ([src/peer.rs:680](src/peer.rs#L680)) | `queue_download`, `queue_download_latin1` | `queue_download_latin1_filename_is_echoed_with_its_bytes` | strings <= LIMIT_STRING_BYTES |
| 44 | PlaceInQueueResponse | `peer::PlaceInQueueResponse` ([src/peer.rs:686](src/peer.rs#L686)) | `place_in_queue_response` | - | strings <= LIMIT_STRING_BYTES |
| 46 | UploadFailed | `peer::UploadFailed` ([src/peer.rs:694](src/peer.rs#L694)) | `upload_failed` | - | strings <= LIMIT_STRING_BYTES |
| 47 | ExactFileSearchRequest | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 48 | QueuedDownloads | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 49 | IndirectFileSearchRequest | n/a: no layout in the reference (§01a §2.16), decodes as `UnknownCode` | n/a | `codes_without_a_layout_decode_as_unknown` | n/a |
| 50 | UploadDenied | `peer::UploadDenied` ([src/peer.rs:700](src/peer.rs#L700)) | `upload_denied` | - | strings <= LIMIT_STRING_BYTES |
| 51 | PlaceInQueueRequest | `peer::PlaceInQueueRequest` ([src/peer.rs:707](src/peer.rs#L707)) | `place_in_queue_request` | - | strings <= LIMIT_STRING_BYTES |
| 52 | UploadQueueNotification | `PeerMessage::UploadQueueNotification`, empty payload | `upload_queue_notification` | - | fixed-size or empty payload |

## Distributed (§01 §4.3, §01a §1.5)

Frame length <= `LIMIT_DISTRIBUTED_MESSAGE_BYTES` on every row.

| Code | Name | Codec | Round trip | Also | Limits |
|---|---|---|---|---|---|
| 0 | Ping | `distributed::Ping` ([src/distributed.rs:43](src/distributed.rs#L43)) | `ping`, `ping_with_token` | - | fixed-size or empty payload |
| 3 | SearchRequest | `distributed::SearchRequest` ([src/distributed.rs:72](src/distributed.rs#L72)) | `search_request` | `search_request_rejects_an_identifier_other_than_49` | strings <= LIMIT_STRING_BYTES; identifier must be 49 |
| 4 | BranchLevel | `distributed::BranchLevel` ([src/distributed.rs:108](src/distributed.rs#L108)) | `branch_level` | `branch_level_rejects_a_negative_level` | negative level rejected |
| 5 | BranchRoot | `distributed::BranchRoot` ([src/distributed.rs:129](src/distributed.rs#L129)) | `branch_root` | `branch_root_rejects_a_non_printable_username` | strings <= LIMIT_STRING_BYTES; username per §9 rule 6 |
| 6 | Unknown | n/a: placeholder of the original library, not a wire code; decodes as `UnknownCode` | n/a | `unknown_placeholder_code` | n/a |
| 7 | ChildDepth | `distributed::ChildDepth` ([src/distributed.rs:152](src/distributed.rs#L152)) | `child_depth` | - | fixed-size or empty payload |
| 93 | EmbeddedMessage | `distributed::EmbeddedMessage` ([src/distributed.rs:203](src/distributed.rs#L203)) | `embedded_message`, `embedded_message_padded` | `embedded_message_with_a_short_zero_prefix_is_not_padded`, `embedded_message_inner_frame_decodes_as_distributed` | inner payload kept as raw bytes, decoded by DistributedMessage::decode on the inner frame |

## File connection (§01 §3.3, §01a §1.4)

| Message | Codec | Round trip | Limits |
|---|---|---|---|
| FileTransferInit | `init::FileTransferInit` ([src/init.rs:78](src/init.rs#L78)) | `file_transfer_init` | exactly 4 bytes |
| FileOffset | `init::FileOffset` ([src/init.rs:98](src/init.rs#L98)) | `file_offset` | exactly 8 bytes; `offset > file size` (§9 rule 5) needs the file size, so the transfer layer refuses it |

## Not covered

- Type-1 ("rotated") obfuscation: the spec gives no algorithm, so there is no codec and no
  `obfuscation` fuzz target. Pinned as low priority in TODO.md under A1. The obfuscation fields
  of `SetListenPort`, `GetPeerAddress` and `ConnectToPeer` are decoded and covered above.
