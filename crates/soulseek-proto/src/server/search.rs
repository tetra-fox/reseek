//! Searches sent through the server, and the searches it asks us to answer.

use crate::codec::wire_struct;
use crate::wire::RemoteFilename;

wire_struct! {
  /// Server code 25: a search of one room's members, by room id. Obsolete: `RoomSearch` replaced
  /// it.
  pub struct FileSearchRoom {
    pub token: u32,
    pub room_id: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 26: a search of the whole network; peers send their results as `SearchResponse`
  /// with the same token.
  pub struct FileSearchRequest {
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 26: a search the server asks us to answer, from a room or user search that
  /// includes us.
  pub struct FileSearchResponse {
    pub username: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 42: a search of one user's shares, which the server forwards to that user.
  pub struct UserSearch {
    pub username: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 65: a search for other sources of exactly one file. Obsolete: it yields no
  /// results.
  pub struct ExactFileSearchRequest {
    pub token: u32,
    pub filename: RemoteFilename,
    pub path: RemoteFilename,
    pub file_size: u64,
    pub checksum: u32,
    pub unknown: u8,
  }
}

wire_struct! {
  /// Server code 65: an exact file search relayed by the server. Obsolete: the live server no
  /// longer sends it.
  pub struct ExactFileSearchResponse {
    pub username: String,
    pub token: u32,
    pub filename: RemoteFilename,
    pub path: RemoteFilename,
    pub file_size: u64,
    pub checksum: u32,
  }
}

wire_struct! {
  /// Server code 103: a network search from the wishlist, rate-limited by the server to
  /// `WishlistInterval`.
  pub struct WishlistSearch {
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 104: how many seconds to wait between wishlist searches, sent after login.
  pub struct WishlistInterval {
    pub seconds: u32,
  }
}

wire_struct! {
  /// Server code 120: a search of one room's members, which the server forwards to each of them.
  pub struct RoomSearch {
    pub room: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 153: asks for search terms related to a query. Obsolete: the server returns an
  /// empty list.
  pub struct RelatedSearchRequest {
    pub query: String,
  }
}

wire_struct! {
  /// Server code 153: search terms related to a query, with scores. Obsolete: the list is always
  /// empty.
  pub struct RelatedSearchResponse {
    pub query: String,
    pub terms: Vec<RelatedTerm>,
  }
}

wire_struct! {
  /// A search term `RelatedSearchResponse` relates to the query, with its score
  pub struct RelatedTerm {
    pub term: String,
    pub score: u32,
  }
}

wire_struct! {
  /// Server code 160: phrases the server asks clients not to return search results for, sent after
  /// login.
  pub struct ExcludedSearchPhrases {
    pub phrases: Vec<String>,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{p, vectors};

  vectors! {
    file_search_room_request:
      Q::FileSearchRoom(FileSearchRoom { token: 8, room_id: 3, query: "a b".into() }) => p().u32(8).u32(3).s("a b").frame(25);
    file_search_request:
      Q::FileSearch(FileSearchRequest { token: 8, query: "boards of canada".into() })
      => p().u32(8).s("boards of canada").frame(26);
    file_search_response:
      R::FileSearch(FileSearchResponse { username: "bob".into(), token: 3, query: "aphex".into() })
      => p().s("bob").u32(3).s("aphex").frame(26);
    user_search_request:
      Q::UserSearch(UserSearch { username: "bob".into(), token: 9, query: "flac".into() }) => p().s("bob").u32(9).s("flac").frame(42);
    exact_file_search_request:
      Q::ExactFileSearch(ExactFileSearchRequest {
        token: 3, filename: "a.mp3".into(), path: "music\\a".into(), file_size: 1 << 33, checksum: 77, unknown: 1,
      })
      => p().u32(3).s("a.mp3").s("music\\a").u64(1 << 33).u32(77).u8(1).frame(65);
    exact_file_search_response:
      R::ExactFileSearch(ExactFileSearchResponse {
        username: "bob".into(), token: 3, filename: "a.mp3".into(), path: "music".into(), file_size: 99, checksum: 1,
      })
      => p().s("bob").u32(3).s("a.mp3").s("music").u64(99).u32(1).frame(65);
    wishlist_search_request:
      Q::WishlistSearch(WishlistSearch { token: 10, query: "rare".into() }) => p().u32(10).s("rare").frame(103);
    wishlist_interval_response: R::WishlistInterval(WishlistInterval { seconds: 720 }) => p().u32(720).frame(104);
    room_search_request:
      Q::RoomSearch(RoomSearch { room: "indie".into(), token: 11, query: "ep".into() }) => p().s("indie").u32(11).s("ep").frame(120);
    related_search_request: Q::RelatedSearch(RelatedSearchRequest { query: "jazz".into() }) => p().s("jazz").frame(153);
    related_search_response:
      R::RelatedSearch(RelatedSearchResponse { query: "jazz".into(), terms: vec![RelatedTerm { term: "bebop".into(), score: 5 }] })
      => p().s("jazz").u32(1).s("bebop").u32(5).frame(153);
    excluded_search_phrases_response:
      R::ExcludedSearchPhrases(ExcludedSearchPhrases { phrases: vec!["bad phrase".into()] }) => p().u32(1).s("bad phrase").frame(160);
  }
}
