# soulseek-proto coverage

One row per message code of §01 §4 and §01a §1. "Encode" and "Decode" name the functions that
handle the code (server codes are split by direction into `ServerRequest`, client to server, and
`ServerResponse`, server to client). "Round trip" lists the byte-exact tests in
`tests/roundtrip.rs`; "Also" lists the decode-only compat and rejection tests for that code. Every
message family is also covered by the property tests in `src/proptests.rs` (arbitrary bytes never
panic, `decode(encode(m)) == m`, re-encoding a decoded message is stable) and by a fuzz target.
"Limits" names what the decoder enforces beyond the frame length, which is checked for every row.

## Server (§01 §4.1, §01a §1.1)

Frame length <= `LIMIT_SERVER_MESSAGE_BYTES` on every row.

| Code | Name | Encode | Decode | Round trip | Also | Limits |
|---|---|---|---|---|---|---|
| 0 | Unknown | n/a: not a wire code | n/a: decodes as `UnknownCode` | n/a | `unknown_server_code` | n/a |
| 1 | Login | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_login`, `server_response_login_success`, `server_response_login_failure`, `server_response_login_failure_detail` | `login_success_must_be_exactly_one` | strings <= LIMIT_STRING_BYTES; greeting <= LIMIT_DESCRIPTION_BYTES |
| 2 | SetListenPort | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_set_listen_port`, `server_request_set_listen_port_obfuscated` | `request_only_code_is_not_a_response` | fixed-size or empty payload |
| 3 | GetPeerAddress | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_peer_address`, `server_response_get_peer_address`, `server_response_get_peer_address_without_trailer` | `get_peer_address_partial_trailer_is_ignored` | strings <= LIMIT_STRING_BYTES |
| 5 | WatchUser | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_watch_user`, `server_response_watch_user`, `server_response_watch_user_blank_country`, `server_response_watch_user_offline`, `server_response_watch_user_missing` | - | strings <= LIMIT_STRING_BYTES |
| 6 | UnwatchUser | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_unwatch_user` | - | strings <= LIMIT_STRING_BYTES |
| 7 | GetStatus | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_status`, `server_response_get_status` | `bools_accept_any_non_zero`, `user_presence_out_of_range` | strings <= LIMIT_STRING_BYTES |
| 11 | IgnoreUser | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_ignore_user`, `server_response_ignore_user` | - | strings <= LIMIT_STRING_BYTES |
| 12 | UnignoreUser | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_unignore_user`, `server_response_unignore_user` | - | strings <= LIMIT_STRING_BYTES |
| 13 | SayInChatRoom | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_say_in_chat_room`, `server_response_say_in_chat_room` | - | strings <= LIMIT_STRING_BYTES |
| 14 | JoinRoom | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_join_room`, `server_response_join_room`, `server_response_join_room_private` | `join_room_with_mismatched_lists` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; five parallel counts must be equal (§9 rule 4) |
| 15 | LeaveRoom | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_leave_room`, `server_response_leave_room` | `trailing_fields_are_ignored`, `string_longer_than_the_payload`, `frame_cut_short` | strings <= LIMIT_STRING_BYTES |
| 16 | UserJoinedRoom | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_user_joined_room` | - | strings <= LIMIT_STRING_BYTES |
| 17 | UserLeftRoom | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_user_left_room` | - | strings <= LIMIT_STRING_BYTES |
| 18 | ConnectToPeer | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_connect_to_peer`, `server_response_connect_to_peer`, `server_response_connect_to_peer_without_trailer` | `connect_to_peer_unknown_type` | strings <= LIMIT_STRING_BYTES |
| 22 | PrivateMessage | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_message`, `server_response_private_message`, `server_response_private_message_replayed` | `private_message_is_new_must_be_exactly_one` | strings <= LIMIT_STRING_BYTES |
| 23 | AcknowledgePrivateMessage | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_acknowledge_private_message` | - | fixed-size or empty payload |
| 25 | FileSearchRoom | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_file_search_room` | - | strings <= LIMIT_STRING_BYTES |
| 26 | FileSearch | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_file_search`, `server_response_file_search` | - | strings <= LIMIT_STRING_BYTES |
| 28 | SetOnlineStatus | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_set_online_status` | - | fixed-size or empty payload |
| 32 | Ping | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_ping`, `server_response_ping` | `frame_with_bytes_after_it` | fixed-size or empty payload |
| 33 | SendConnectToken | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_send_connect_token`, `server_response_send_connect_token` | - | strings <= LIMIT_STRING_BYTES |
| 34 | SendSpeed | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_send_speed` | - | strings <= LIMIT_STRING_BYTES |
| 35 | SharedFoldersAndFiles | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_shared_folders_and_files` | - | fixed-size or empty payload |
| 36 | GetUserStats | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_user_stats`, `server_response_get_user_stats` | - | strings <= LIMIT_STRING_BYTES |
| 40 | QueuedDownloads | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_queued_downloads`, `server_response_queued_downloads` | - | strings <= LIMIT_STRING_BYTES |
| 41 | KickedFromServer | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_kicked_from_server` | - | fixed-size or empty payload |
| 42 | UserSearch | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_user_search` | - | strings <= LIMIT_STRING_BYTES |
| 50 | SimilarRecommendations | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_similar_recommendations`, `server_response_similar_recommendations` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 51 | LikedInterestAdd | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_liked_interest_add` | - | strings <= LIMIT_STRING_BYTES |
| 52 | LikedInterestRemove | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_liked_interest_remove` | - | strings <= LIMIT_STRING_BYTES |
| 54 | GetPersonalRecommendations | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_personal_recommendations`, `server_response_get_personal_recommendations` | `recommendations_without_second_block` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 55 | MyRecommendations | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_my_recommendations`, `server_response_my_recommendations` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 56 | GetGlobalRecommendations | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_global_recommendations`, `server_response_get_global_recommendations` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 57 | GetUserInterests | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_user_interests`, `server_response_get_user_interests` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 58 | AdminCommand | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_admin_command` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 59 | PlaceInLineRequest | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_place_in_line_request`, `server_response_place_in_line_request` | - | strings <= LIMIT_STRING_BYTES |
| 60 | PlaceInLineResponse | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_place_in_line_response`, `server_response_place_in_line_response` | - | strings <= LIMIT_STRING_BYTES |
| 62 | RoomAdded | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_room_added` | - | strings <= LIMIT_STRING_BYTES |
| 63 | RoomRemoved | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_room_removed` | - | strings <= LIMIT_STRING_BYTES |
| 64 | RoomList | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_room_list`, `server_response_room_list` | `room_list_with_mismatched_lists`, `room_count_beyond_the_payload` | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; name and count lists must be equal (§9 rule 4) |
| 65 | ExactFileSearch | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_exact_file_search`, `server_response_exact_file_search` | - | strings <= LIMIT_STRING_BYTES |
| 66 | GlobalAdminMessage | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_global_admin_message` | - | strings <= LIMIT_STRING_BYTES |
| 67 | GlobalUserList | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_global_user_list`, `server_response_global_user_list` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT; five parallel counts must be equal (§9 rule 4) |
| 68 | TunneledMessage | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_tunneled_message`, `server_response_tunneled_message` | - | strings <= LIMIT_STRING_BYTES |
| 69 | PrivilegedUsers | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_privileged_users` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 71 | HaveNoParents | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_have_no_parents` | - | fixed-size or empty payload |
| 73 | ParentsIP | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_parents_ip`, `server_request_parents_ip_none` | - | fixed-size or empty payload |
| 83 | ParentMinSpeed | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_parent_min_speed` | - | fixed-size or empty payload |
| 84 | ParentSpeedRatio | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_parent_speed_ratio` | - | fixed-size or empty payload |
| 86 | ParentInactivityTimeout | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_parent_inactivity_timeout` | - | fixed-size or empty payload |
| 87 | SearchInactivityTimeout | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_search_inactivity_timeout` | - | fixed-size or empty payload |
| 88 | MinimumParentsInCache | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_minimum_parents_in_cache` | - | fixed-size or empty payload |
| 90 | DistributedAliveInterval | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_distributed_alive_interval` | - | fixed-size or empty payload |
| 91 | AddPrivilegedUser | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_add_privileged_user` | - | strings <= LIMIT_STRING_BYTES |
| 92 | CheckPrivileges | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_check_privileges`, `server_response_check_privileges` | - | fixed-size or empty payload |
| 93 | EmbeddedMessage | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_embedded_message` | - | strings <= LIMIT_STRING_BYTES |
| 100 | AcceptChildren | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_accept_children` | - | fixed-size or empty payload |
| 102 | NetInfo | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_net_info` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 103 | WishlistSearch | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_wishlist_search` | - | strings <= LIMIT_STRING_BYTES |
| 104 | WishlistInterval | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_wishlist_interval` | - | fixed-size or empty payload |
| 110 | GetPersonalSimilarUsers | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_personal_similar_users`, `server_response_get_personal_similar_users` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 111 | GetInterestRecommendations | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_interest_recommendations`, `server_response_get_interest_recommendations` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 112 | GetInterestSimilarUsers | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_get_interest_similar_users`, `server_response_get_interest_similar_users` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 113 | RoomTickers | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_room_tickers` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 114 | RoomTickerAdd | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_room_ticker_add` | - | strings <= LIMIT_STRING_BYTES |
| 115 | RoomTickerRemove | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_room_ticker_remove` | - | strings <= LIMIT_STRING_BYTES |
| 116 | SetRoomTicker | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_set_room_ticker` | - | strings <= LIMIT_STRING_BYTES |
| 117 | HatedInterestAdd | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_hated_interest_add` | - | strings <= LIMIT_STRING_BYTES |
| 118 | HatedInterestRemove | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_hated_interest_remove` | - | strings <= LIMIT_STRING_BYTES |
| 120 | RoomSearch | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_room_search` | - | strings <= LIMIT_STRING_BYTES |
| 121 | SendUploadSpeed | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_send_upload_speed` | - | fixed-size or empty payload |
| 122 | UserPrivileges | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_user_privileges`, `server_response_user_privileges` | - | strings <= LIMIT_STRING_BYTES |
| 123 | GivePrivileges | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_give_privileges` | - | strings <= LIMIT_STRING_BYTES |
| 124 | NotifyPrivileges | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_notify_privileges` | - | strings <= LIMIT_STRING_BYTES |
| 125 | AcknowledgeNotifyPrivileges | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_acknowledge_notify_privileges` | - | fixed-size or empty payload |
| 126 | BranchLevel | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_branch_level` | - | fixed-size or empty payload |
| 127 | BranchRoot | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_branch_root` | - | strings <= LIMIT_STRING_BYTES |
| 129 | ChildDepth | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_child_depth` | - | fixed-size or empty payload |
| 130 | DistributedReset | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_distributed_reset` | - | fixed-size or empty payload |
| 133 | PrivateRoomUsers | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_private_room_users` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 134 | PrivateRoomAddUser | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_room_add_user`, `server_response_private_room_add_user` | - | strings <= LIMIT_STRING_BYTES |
| 135 | PrivateRoomRemoveUser | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_room_remove_user`, `server_response_private_room_remove_user` | - | strings <= LIMIT_STRING_BYTES |
| 136 | PrivateRoomDropMembership | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_private_room_drop_membership` | - | strings <= LIMIT_STRING_BYTES |
| 137 | PrivateRoomDropOwnership | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_private_room_drop_ownership` | - | strings <= LIMIT_STRING_BYTES |
| 138 | PrivateRoomUnknown | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_room_unknown`, `server_response_private_room_unknown` | - | strings <= LIMIT_STRING_BYTES |
| 139 | PrivateRoomAdded | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_private_room_added` | - | strings <= LIMIT_STRING_BYTES |
| 140 | PrivateRoomRemoved | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_private_room_removed` | - | strings <= LIMIT_STRING_BYTES |
| 141 | PrivateRoomToggle | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_room_toggle`, `server_response_private_room_toggle` | - | fixed-size or empty payload |
| 142 | NewPassword | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_new_password`, `server_response_new_password` | - | strings <= LIMIT_STRING_BYTES |
| 143 | PrivateRoomAddOperator | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_room_add_operator`, `server_response_private_room_add_operator` | - | strings <= LIMIT_STRING_BYTES |
| 144 | PrivateRoomRemoveOperator | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_private_room_remove_operator`, `server_response_private_room_remove_operator` | - | strings <= LIMIT_STRING_BYTES |
| 145 | PrivateRoomOperatorAdded | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_private_room_operator_added` | - | strings <= LIMIT_STRING_BYTES |
| 146 | PrivateRoomOperatorRemoved | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_private_room_operator_removed` | - | strings <= LIMIT_STRING_BYTES |
| 148 | PrivateRoomOwned | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_private_room_owned` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 149 | MessageUsers | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_message_users` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 150 | AskPublicChat | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_ask_public_chat` | - | fixed-size or empty payload |
| 151 | StopPublicChat | `ServerRequest::encode` | `ServerRequest::decode` | `server_request_stop_public_chat` | - | fixed-size or empty payload |
| 152 | PublicChat | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_public_chat` | - | strings <= LIMIT_STRING_BYTES |
| 153 | RelatedSearch | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_related_search`, `server_response_related_search` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 160 | ExcludedSearchPhrases | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_excluded_search_phrases` | - | strings <= LIMIT_STRING_BYTES; LIMIT_LIST_COUNT |
| 1001 | CannotConnect | `ServerRequest::encode`, `ServerResponse::encode` | `ServerRequest::decode`, `ServerResponse::decode` | `server_request_cannot_connect`, `server_response_cannot_connect`, `server_response_cannot_connect_with_username` | - | strings <= LIMIT_STRING_BYTES |
| 1003 | CannotJoinRoom | `ServerResponse::encode` | `ServerResponse::decode` | `server_response_cannot_join_room` | - | strings <= LIMIT_STRING_BYTES |

## Peer init (§01 §4.4, §01a §1.2)

Frame length <= `LIMIT_INIT_MESSAGE_BYTES` on every row.

| Code | Name | Encode | Decode | Round trip | Also | Limits |
|---|---|---|---|---|---|---|
| 0 | PierceFirewall | `InitMessage::encode` | `InitMessage::decode` | `init_pierce_firewall` | - | fixed-size or empty payload |
| 1 | PeerInit | `InitMessage::encode` | `InitMessage::decode` | `init_peer_init` | `peer_init_token_is_ignored_but_kept`, `peer_init_unknown_type`, `peer_init_reserved_username`, `peer_init_empty_username`, `init_frame_over_limit` | strings <= LIMIT_STRING_BYTES; username per §9 rule 6; unknown connection type rejected |

## Peer (§01 §4.2, §01a §1.3)

Frame length <= `LIMIT_PEER_MESSAGE_BYTES` on every row.

| Code | Name | Encode | Decode | Round trip | Also | Limits |
|---|---|---|---|---|---|---|
| 1 | PrivateMessage | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 4 | BrowseRequest | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_browse_request` | - | fixed-size or empty payload |
| 5 | BrowseResponse | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_browse_response`, `peer_browse_response_empty` | `browse_response_without_trailer`, `browse_response_ns_sign_extended_size`, `compressed_payload_not_zlib` | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT (file >= 21 B, directory >= 8 B) |
| 8 | SearchRequest | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_search_request` | - | strings <= LIMIT_STRING_BYTES |
| 9 | SearchResponse | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_search_response` | `search_response_legacy_without_trailer`, `search_response_eight_byte_queue_length`, `search_response_stray_trailing_byte`, `file_with_too_many_attributes`, `search_response_file_count_beyond_the_body` | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT (file >= 21 B); trailer read only with >= 4 B |
| 10 | PrivateRoomInvitation | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 14 | CancelledQueuedTransfer | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 15 | InfoRequest | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_info_request` | - | fixed-size or empty payload |
| 16 | InfoResponse | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_info_response`, `peer_info_response_without_picture` | `info_response_museek_stray_bytes`, `info_response_picture_over_limit` | strings <= LIMIT_STRING_BYTES; description <= LIMIT_DESCRIPTION_BYTES, LIMIT_USER_INFO_PICTURE_BYTES |
| 33 | SendConnectToken | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 34 | MoveDownloadToTop | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 36 | FolderContentsRequest | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_folder_contents_request` | - | strings <= LIMIT_STRING_BYTES |
| 37 | FolderContentsResponse | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_folder_contents_response` | - | strings <= LIMIT_STRING_BYTES; LIMIT_INFLATED_BYTES, LIMIT_FILES_PER_RESPONSE, LIMIT_DIRECTORIES_PER_RESPONSE, LIMIT_ATTRIBUTES_PER_FILE, LIMIT_LIST_COUNT |
| 40 | TransferRequest | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_transfer_request_download`, `peer_transfer_request_upload` | `transfer_request_download_ignores_stray_size`, `transfer_request_upload_without_size` | strings <= LIMIT_STRING_BYTES |
| 41 | TransferResponse | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_transfer_response_allowed_with_size`, `peer_transfer_response_allowed`, `peer_transfer_response_denied` | `transfer_response_allowed_must_be_exactly_one`, `transfer_response_denied_needs_a_reason` | strings <= LIMIT_STRING_BYTES |
| 42 | UploadPlacehold | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_upload_placehold` | - | strings <= LIMIT_STRING_BYTES |
| 43 | QueueDownload | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_queue_download`, `peer_queue_download_latin1` | - | strings <= LIMIT_STRING_BYTES |
| 44 | PlaceInQueueResponse | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_place_in_queue_response` | - | strings <= LIMIT_STRING_BYTES |
| 46 | UploadFailed | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_upload_failed` | - | strings <= LIMIT_STRING_BYTES |
| 47 | ExactFileSearchRequest | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 48 | QueuedDownloads | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 49 | IndirectFileSearchRequest | n/a: no layout in the reference (§01a §2.16) | n/a: decodes as `UnknownCode` | n/a | `peer_codes_without_layout_decode_as_unknown` | n/a |
| 50 | UploadDenied | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_upload_denied` | - | strings <= LIMIT_STRING_BYTES |
| 51 | PlaceInQueueRequest | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_place_in_queue_request` | - | strings <= LIMIT_STRING_BYTES |
| 52 | UploadQueueNotification | `PeerMessage::encode` | `PeerMessage::decode_with` | `peer_upload_queue_notification` | - | fixed-size or empty payload |

## Distributed (§01 §4.3, §01a §1.5)

Frame length <= `LIMIT_DISTRIBUTED_MESSAGE_BYTES` on every row.

| Code | Name | Encode | Decode | Round trip | Also | Limits |
|---|---|---|---|---|---|---|
| 0 | Ping | `DistributedMessage::encode` | `DistributedMessage::decode` | `distributed_ping`, `distributed_ping_with_token` | - | fixed-size or empty payload |
| 3 | SearchRequest | `DistributedMessage::encode` | `DistributedMessage::decode` | `distributed_search_request` | `distributed_search_identifier_not_49` | strings <= LIMIT_STRING_BYTES; identifier must be 49 |
| 4 | BranchLevel | `DistributedMessage::encode` | `DistributedMessage::decode` | `distributed_branch_level` | `branch_level_negative` | negative level rejected |
| 5 | BranchRoot | `DistributedMessage::encode` | `DistributedMessage::decode` | `distributed_branch_root` | `branch_root_non_printable` | strings <= LIMIT_STRING_BYTES; username per §9 rule 6 |
| 6 | Unknown | n/a: placeholder of the original library, not a wire code | n/a: decodes as `UnknownCode` | n/a | `distributed_placeholder_code` | n/a |
| 7 | ChildDepth | `DistributedMessage::encode` | `DistributedMessage::decode` | `distributed_child_depth` | - | fixed-size or empty payload |
| 93 | EmbeddedMessage | `DistributedMessage::encode` | `DistributedMessage::decode` | `distributed_embedded_message`, `distributed_embedded_message_padded` | `embedded_message_with_short_zero_prefix_is_not_padded` | strings <= LIMIT_STRING_BYTES |

## File connection (§01 §3.3, §01a §1.4)

| Message | Encode | Decode | Round trip | Limits |
|---|---|---|---|---|
| File Transfer Init (`u32 token`) | `FileTransferInit::encode` | `FileTransferInit::decode` | `file_transfer_init_round_trip` | exactly 4 bytes |
| File Offset (`u64 offset`) | `FileOffset::encode` | `FileOffset::decode` | `file_offset_round_trip` | exactly 8 bytes; `offset > file size` (§9 rule 5) needs the file size, so the transfer layer refuses it |

## Not covered

- Type-1 ("rotated") obfuscation: the spec gives no algorithm, so there is no codec and no
  `obfuscation` fuzz target. Listed in TODO.md under A1. The obfuscation fields of
  `SetListenPort`, `GetPeerAddress` and `ConnectToPeer` are decoded and covered above.
