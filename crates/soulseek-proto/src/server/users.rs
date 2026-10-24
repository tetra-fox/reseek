//! Messages about other users: their address, status and statistics, watching and ignoring them
//! (spec 01 sections 5.3 to 5.6, 5.19, 01a section 2.1). `UserStats` and `UserData`, which the
//! room messages also carry, are here.

use std::net::Ipv4Addr;

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::UserPresence;
use crate::wire::{Reader, Writer};

/// The statistics tuple of `WatchUser`, `GetUserStats` and the room messages. On the wire
/// `upload_count` is followed by a `u32` that is always 0; it is written as 0 and not kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserStats {
  /// bytes/second of recent uploads
  pub average_speed: u32,
  /// completed uploads as counted by the server, a rolling 32-bit counter
  pub upload_count: u32,
  pub file_count: u32,
  pub directory_count: u32,
}

impl Wire for UserStats {
  const MIN_BYTES: usize = 20;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    let average_speed = r.u32("average_speed")?;
    let upload_count = r.u32("upload_count")?;
    // always 0 on the live server
    r.u32("unknown")?;
    Ok(Self {
      average_speed,
      upload_count,
      file_count: r.u32("file_count")?,
      directory_count: r.u32("directory_count")?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.average_speed);
    w.u32(self.upload_count);
    w.u32(0);
    w.u32(self.file_count);
    w.u32(self.directory_count);
  }
}

/// A room member as `JoinRoom`, `UserJoinedRoom` and `GlobalUserList` describe one; those
/// messages always carry `slots_full` and `country_code`
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserData {
  pub username: String,
  pub status: UserPresence,
  pub stats: UserStats,
  /// every upload slot of this user is busy
  pub slots_full: bool,
  /// ISO 3166-1 alpha-2, upper-case, may be blank
  pub country_code: String,
}

wire_struct! {
  /// Server code 3, spec 01 section 5.3
  pub struct GetPeerAddressRequest {
    pub username: String,
  }
}

/// Server code 3, spec 01 section 5.3
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct GetPeerAddressResponse {
  pub username: String,
  /// `0.0.0.0` when the user is offline
  pub ip: Ipv4Addr,
  pub port: u32,
  /// absent from older servers; never dialled
  pub obfuscation: Option<PeerAddressObfuscation>,
}

wire_struct! {
  /// The obfuscated port trailer of `GetPeerAddressResponse`, where the port is two bytes. Type 0
  /// is none and 1 is "rotated"; this client never advertises one and never dials the obfuscated
  /// port.
  pub struct PeerAddressObfuscation {
    pub obfuscation_type: u32,
    pub obfuscated_port: u16,
  }
}

impl Wire for GetPeerAddressResponse {
  const MIN_BYTES: usize = 12;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string("username")?,
      ip: r.ip4("ip")?,
      port: r.u32("port")?,
      obfuscation: r.optional_fixed("obfuscation")?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    w.ip4(self.ip);
    w.u32(self.port);
    if let Some(obfuscation) = &self.obfuscation {
      obfuscation.write(w);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

wire_struct! {
  /// Server code 5, spec 01 section 5.4
  pub struct WatchUserRequest {
    pub username: String,
  }
}

/// Server code 5, spec 01 section 5.4
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct WatchUserResponse {
  pub username: String,
  /// `None` when the user does not exist
  pub user: Option<WatchedUser>,
}

impl Wire for WatchUserResponse {
  const MIN_BYTES: usize = 5;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    let username = r.string("username")?;
    let user = if r.bool("exists")? {
      Some(WatchedUser {
        status: UserPresence::read(r, "status")?,
        stats: UserStats::read(r, "stats")?,
        country_code: r.optional("country_code")?,
      })
    } else {
      None
    };
    Ok(Self { username, user })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    w.bool(self.user.is_some());
    if let Some(user) = &self.user {
      user.status.write(w);
      user.stats.write(w);
      if let Some(country_code) = &user.country_code {
        w.string(country_code);
      }
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct WatchedUser {
  pub status: UserPresence,
  pub stats: UserStats,
  /// present only when the user is not offline, and may be blank
  pub country_code: Option<String>,
}

wire_struct! {
  /// Server code 6, spec 01 section 5.5
  pub struct UnwatchUser {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 7, spec 01 section 5.6
  pub struct GetStatusRequest {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 7, spec 01 section 5.6
  ///
  /// The `GetStatus` reply, also pushed for watched users and room members
  pub struct UserStatus {
    pub username: String,
    pub presence: UserPresence,
    pub is_privileged: bool,
  }
}

wire_struct! {
  /// Server code 11, spec 01a section 2.1, obsolete
  pub struct IgnoreUser {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 12, spec 01a section 2.1, obsolete
  pub struct UnignoreUser {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 36, spec 01 section 5.19
  pub struct GetUserStatsRequest {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 36, spec 01 section 5.19
  ///
  /// The `GetUserStats` reply, also pushed for users who share a room with us
  pub struct UserStatistics {
    pub username: String,
    pub stats: UserStats,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::error::FieldError;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{IP, P, decodes, field, ip, p, vectors};

  pub(crate) fn stats() -> UserStats {
    UserStats {
      average_speed: 51_200,
      upload_count: 12,
      file_count: 3_400,
      directory_count: 210,
    }
  }

  /// the stats tuple with its always-zero third field
  pub(crate) fn stats_bytes(b: P) -> P {
    b.u32(51_200).u32(12).u32(0).u32(3_400).u32(210)
  }

  pub(crate) fn user(
    username: &str,
    status: UserPresence,
    slots_full: bool,
    country_code: &str,
  ) -> UserData {
    UserData {
      username: username.into(),
      status,
      stats: stats(),
      slots_full,
      country_code: country_code.into(),
    }
  }

  vectors! {
    get_peer_address_request: Q::GetPeerAddress(GetPeerAddressRequest { username: "bob".into() }) => p().s("bob").frame(3);
    get_peer_address_response:
      R::GetPeerAddress(GetPeerAddressResponse { username: "bob".into(), ip: ip(), port: 2234,
        obfuscation: Some(PeerAddressObfuscation { obfuscation_type: 1, obfuscated_port: 2235 }),
      })
      => p().s("bob").ip(IP).u32(2234).u32(1).u16(2235).frame(3);
    get_peer_address_response_without_trailer:
      R::GetPeerAddress(GetPeerAddressResponse { username: "bob".into(), ip: Ipv4Addr::UNSPECIFIED, port: 0, obfuscation: None })
      => p().s("bob").ip([0, 0, 0, 0]).u32(0).frame(3);
    watch_user_request: Q::WatchUser(WatchUserRequest { username: "bob".into() }) => p().s("bob").frame(5);
    watch_user_response:
      R::WatchUser(WatchUserResponse {
        username: "bob".into(),
        user: Some(WatchedUser { status: UserPresence::Online, stats: stats(), country_code: Some("NL".into()) }),
      })
      => stats_bytes(p().s("bob").u8(1).u32(2)).s("NL").frame(5);
    watch_user_response_blank_country:
      R::WatchUser(WatchUserResponse {
        username: "bob".into(),
        user: Some(WatchedUser { status: UserPresence::Away, stats: stats(), country_code: Some(String::new()) }),
      })
      => stats_bytes(p().s("bob").u8(1).u32(1)).s("").frame(5);
    watch_user_response_offline:
      R::WatchUser(WatchUserResponse {
        username: "bob".into(),
        user: Some(WatchedUser { status: UserPresence::Offline, stats: stats(), country_code: None }),
      })
      => stats_bytes(p().s("bob").u8(1).u32(0)).frame(5);
    watch_user_response_missing:
      R::WatchUser(WatchUserResponse { username: "nobody".into(), user: None }) => p().s("nobody").u8(0).frame(5);
    unwatch_user_request: Q::UnwatchUser(UnwatchUser { username: "bob".into() }) => p().s("bob").frame(6);
    get_status_request: Q::GetStatus(GetStatusRequest { username: "bob".into() }) => p().s("bob").frame(7);
    get_status_response:
      R::GetStatus(UserStatus { username: "bob".into(), presence: UserPresence::Away, is_privileged: true })
      => p().s("bob").u32(1).u8(1).frame(7);
    ignore_user_request: Q::IgnoreUser(IgnoreUser { username: "bob".into() }) => p().s("bob").frame(11);
    ignore_user_response: R::IgnoreUser(IgnoreUser { username: "bob".into() }) => p().s("bob").frame(11);
    unignore_user_request: Q::UnignoreUser(UnignoreUser { username: "bob".into() }) => p().s("bob").frame(12);
    unignore_user_response: R::UnignoreUser(UnignoreUser { username: "bob".into() }) => p().s("bob").frame(12);
    get_user_stats_request: Q::GetUserStats(GetUserStatsRequest { username: "bob".into() }) => p().s("bob").frame(36);
    get_user_stats_response:
      R::GetUserStats(UserStatistics { username: "bob".into(), stats: stats() }) => stats_bytes(p().s("bob")).frame(36);
  }

  decodes! {
    get_peer_address_response_ignores_a_partial_trailer: R, p().s("bob").ip(IP).u32(1).u32(1).frame(3)
      => Ok(R::GetPeerAddress(GetPeerAddressResponse { username: "bob".into(), ip: ip(), port: 1, obfuscation: None }));
    get_status_response_bool_accepts_any_non_zero: R, p().s("bob").u32(2).u8(7).frame(7)
      => Ok(R::GetStatus(UserStatus { username: "bob".into(), presence: UserPresence::Online, is_privileged: true }));
    get_status_response_rejects_unknown_presence: R, p().s("bob").u32(3).u8(0).frame(7)
      => Err(field("GetStatus", "presence", FieldError::InvalidValue { value: 3 }));
  }
}
