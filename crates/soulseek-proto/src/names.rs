//! Rules for usernames and room names that are checked locally, before the server or a peer
//! would reject them.

/// The pseudo-user the server sends its own notices as
pub const SERVER_USERNAME: &str = "server";

/// Why the server would reject a login username, mirroring its `INVALIDUSERNAME` details
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoginUsernameError {
  #[error("username is empty")]
  Empty,
  #[error("username is longer than 30 characters")]
  TooLong,
  #[error("username has characters other than printable ascii")]
  InvalidCharacters,
  #[error("username starts or ends with a space")]
  EdgeSpaces,
  #[error("username is reserved")]
  Reserved,
}

/// The server's rules for the username in `Login`: 1 to 30 printable ascii characters, no
/// leading or trailing space, not the reserved name `server`
pub fn validate_login_username(username: &str) -> Result<(), LoginUsernameError> {
  if username.is_empty() {
    return Err(LoginUsernameError::Empty);
  }
  if !username.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
    return Err(LoginUsernameError::InvalidCharacters);
  }
  if username.len() > 30 {
    return Err(LoginUsernameError::TooLong);
  }
  if username.starts_with(' ') || username.ends_with(' ') {
    return Err(LoginUsernameError::EdgeSpaces);
  }
  if username == SERVER_USERNAME {
    return Err(LoginUsernameError::Reserved);
  }
  Ok(())
}

/// Why the server would reject a room name in `JoinRoom`
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RoomNameError {
  #[error("room name is empty")]
  Empty,
  #[error("room name is longer than 24 characters")]
  TooLong,
  #[error("room name has non-ascii characters")]
  NonAscii,
  #[error("room name starts or ends with a space")]
  EdgeSpaces,
  #[error("room name has consecutive spaces")]
  ConsecutiveSpaces,
}

/// The server's rules for a room name in `JoinRoom`. A name that breaks them gets a private
/// message from `server` and no reply at all, so it is checked before sending.
pub fn validate_room_name(room: &str) -> Result<(), RoomNameError> {
  if room.is_empty() {
    return Err(RoomNameError::Empty);
  }
  if !room.is_ascii() {
    return Err(RoomNameError::NonAscii);
  }
  if room.len() > 24 {
    return Err(RoomNameError::TooLong);
  }
  if room.starts_with(' ') || room.ends_with(' ') {
    return Err(RoomNameError::EdgeSpaces);
  }
  if room.contains("  ") {
    return Err(RoomNameError::ConsecutiveSpaces);
  }
  Ok(())
}

/// The rule for usernames peers send in `PeerInit` and `BranchRoot`: non-empty, at most 256
/// bytes, no control characters, not `server`
pub(crate) fn is_valid_peer_username(username: &str) -> bool {
  !username.is_empty()
    && username.len() <= 256
    && !username.chars().any(char::is_control)
    && username != SERVER_USERNAME
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn login_username_rules() {
    assert_eq!(validate_login_username("alice"), Ok(()));
    assert_eq!(validate_login_username("a b"), Ok(()));
    assert_eq!(validate_login_username(&"x".repeat(30)), Ok(()));
    assert_eq!(validate_login_username(""), Err(LoginUsernameError::Empty));
    assert_eq!(
      validate_login_username(&"x".repeat(31)),
      Err(LoginUsernameError::TooLong)
    );
    assert_eq!(
      validate_login_username("caf\u{e9}"),
      Err(LoginUsernameError::InvalidCharacters)
    );
    assert_eq!(
      validate_login_username("a\tb"),
      Err(LoginUsernameError::InvalidCharacters)
    );
    assert_eq!(
      validate_login_username(" a"),
      Err(LoginUsernameError::EdgeSpaces)
    );
    assert_eq!(
      validate_login_username("a "),
      Err(LoginUsernameError::EdgeSpaces)
    );
    assert_eq!(
      validate_login_username("server"),
      Err(LoginUsernameError::Reserved)
    );
  }

  #[test]
  fn room_name_rules() {
    assert_eq!(validate_room_name("indie rock"), Ok(()));
    assert_eq!(validate_room_name(&"r".repeat(24)), Ok(()));
    assert_eq!(validate_room_name(""), Err(RoomNameError::Empty));
    assert_eq!(
      validate_room_name(&"r".repeat(25)),
      Err(RoomNameError::TooLong)
    );
    assert_eq!(
      validate_room_name("caf\u{e9}"),
      Err(RoomNameError::NonAscii)
    );
    assert_eq!(validate_room_name(" r"), Err(RoomNameError::EdgeSpaces));
    assert_eq!(
      validate_room_name("a  b"),
      Err(RoomNameError::ConsecutiveSpaces)
    );
  }

  #[test]
  fn peer_username_rules() {
    assert!(is_valid_peer_username("alice"));
    assert!(is_valid_peer_username(&"x".repeat(256)));
    assert!(!is_valid_peer_username(&"x".repeat(257)));
    assert!(!is_valid_peer_username(""));
    assert!(!is_valid_peer_username("server"));
    assert!(!is_valid_peer_username("a\nb"));
  }
}
