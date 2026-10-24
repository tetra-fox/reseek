//! Sending a search through the server, in the form its scope needs. Results come back from
//! peers on their message connections, matched by the token.

use soulseek_proto::ServerRequest;
use soulseek_proto::server::{FileSearchRequest, RoomSearch, UserSearch, WishlistSearch};

use super::Client;
use super::request::non_blank;
use crate::error::ClientError;

/// Who a search goes to
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchScope {
  /// everyone, through the distributed tree
  Network,
  /// everyone, from the wishlist, which the server allows only at its wishlist interval
  Wishlist,
  /// the members of one room
  Room(String),
  /// these users
  User(Vec<String>),
}

impl Client {
  /// Sends a search for `text` with `token` to everyone in `scope`. A search of several users is
  /// one request per user, written together.
  pub async fn send_search_request(
    &self,
    scope: &SearchScope,
    token: u32,
    text: &str,
  ) -> Result<(), ClientError> {
    non_blank("search text", text)?;
    let query = text.to_owned();
    let requests = match scope {
      SearchScope::Network => vec![ServerRequest::FileSearch(FileSearchRequest {
        token,
        query,
      })],
      SearchScope::Wishlist => vec![ServerRequest::WishlistSearch(WishlistSearch {
        token,
        query,
      })],
      SearchScope::Room(room) => {
        non_blank("room name", room)?;
        vec![ServerRequest::RoomSearch(RoomSearch {
          room: room.clone(),
          token,
          query,
        })]
      }
      SearchScope::User(usernames) => {
        if usernames.is_empty() {
          return Err(ClientError::InvalidArgument(
            "A user search needs at least one username".into(),
          ));
        }
        usernames
          .iter()
          .map(|username| {
            non_blank("username", username)?;
            Ok(ServerRequest::UserSearch(UserSearch {
              username: username.clone(),
              token,
              query: query.clone(),
            }))
          })
          .collect::<Result<_, ClientError>>()?
      }
    };
    let verb = format!("send search request with token {token}");
    self.require_logged_in(&verb)?;
    self
      .write_server_messages(&requests)
      .await
      .map_err(|error| super::request::failed(&verb, error))?;
    tracing::debug!(token, ?scope, "search request sent");
    Ok(())
  }
}
