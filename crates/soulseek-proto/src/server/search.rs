//! Searches sent through the server, and the searches it asks us to answer (spec 01 sections 5.15,
//! 5.21, 5.29, 5.35, 5.38, 5.47, 01a sections 2.2, 2.11, 2.15).

use crate::codec::wire_struct;
use crate::wire::RemoteFilename;

wire_struct! {
  /// Server code 25, spec 01a section 2.2, obsolete
  pub struct FileSearchRoom {
    pub token: u32,
    pub room_id: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 26, spec 01 section 5.15
  pub struct FileSearchRequest {
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 26, spec 01 section 5.15
  ///
  /// A search the server asks us to answer
  pub struct FileSearchResponse {
    pub username: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 42, spec 01 section 5.21
  pub struct UserSearch {
    pub username: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 65, spec 01a section 2.11, obsolete
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
  /// Server code 65, spec 01a section 2.11, obsolete
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
  /// Server code 103, spec 01 section 5.35
  pub struct WishlistSearch {
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 104, spec 01 section 5.29
  pub struct WishlistInterval {
    pub seconds: u32,
  }
}

wire_struct! {
  /// Server code 120, spec 01 section 5.38
  pub struct RoomSearch {
    pub room: String,
    pub token: u32,
    pub query: String,
  }
}

wire_struct! {
  /// Server code 153, spec 01a section 2.15, obsolete
  pub struct RelatedSearchRequest {
    pub query: String,
  }
}

wire_struct! {
  /// Server code 153, spec 01a section 2.15, obsolete
  pub struct RelatedSearchResponse {
    pub query: String,
    pub terms: Vec<RelatedTerm>,
  }
}

wire_struct! {
  pub struct RelatedTerm {
    pub term: String,
    pub score: u32,
  }
}

wire_struct! {
  /// Server code 160, spec 01 section 5.47
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
