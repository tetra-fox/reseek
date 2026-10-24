//! Obsolete server messages kept as codecs only, which belong to none of the other topics.

use std::net::Ipv4Addr;

use super::rooms::{read_user_lists, write_user_lists};
use super::users::UserData;
use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::wire::{Reader, Writer};

wire_struct! {
  /// Server code 34, also named `SendDownloadSpeed`: a download's speed, sent to update the
  /// uploader's statistics. Obsolete: `SendUploadSpeed` replaced it.
  pub struct SendSpeed {
    pub username: String,
    pub speed: u32,
  }
}

/// Server code 40, also named `UploadSlotsFull`: whether all our upload slots are busy, a boolean
/// carried as `u32`. Obsolete: no client sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct QueuedDownloadsRequest {
  pub slots_full: bool,
}

impl Wire for QueuedDownloadsRequest {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      slots_full: r.u32("slots_full")? != 0,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(u32::from(self.slots_full));
  }
}

/// Server code 40, also named `UploadSlotsFull`: whether all of a user's upload slots are busy.
/// Obsolete: the live server no longer sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct QueuedDownloadsResponse {
  pub username: String,
  pub slots_full: bool,
}

impl Wire for QueuedDownloadsResponse {
  const MIN_BYTES: usize = 8;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string("username")?,
      slots_full: r.u32("slots_full")? != 0,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    w.u32(u32::from(self.slots_full));
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

wire_struct! {
  /// Server code 58: a server admin command, for accounts with admin status. Obsolete: it no longer
  /// has any effect.
  pub struct AdminCommand {
    pub command: String,
    pub args: Vec<String>,
  }
}

wire_struct! {
  /// Server code 59: asks for a queue place by transfer token, relayed by the server. Obsolete:
  /// peers ask with `PlaceInQueueRequest`.
  pub struct PlaceInLineRequest {
    pub username: String,
    pub token: u32,
  }
}

wire_struct! {
  /// Server code 60: a queue place by transfer token, relayed by the server. Obsolete: peers answer
  /// with `PlaceInQueueResponse`.
  pub struct PlaceInLineResponse {
    pub username: String,
    pub token: u32,
    pub place: u32,
  }
}

/// Server code 67: every online user, in the five parallel lists of `JoinRoom` without a room name.
/// Obsolete: no longer served.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct GlobalUserListResponse {
  pub users: Vec<UserData>,
}

impl Wire for GlobalUserListResponse {
  const MIN_BYTES: usize = 20;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      users: read_user_lists(r)?,
    })
  }

  fn write(&self, w: &mut Writer) {
    write_user_lists(w, &self.users);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.count = Some(self.users.len());
  }
}

wire_struct! {
  /// Server code 68: a message tunnelled through the server to a peer we cannot reach; the field
  /// order differs from the response. Obsolete.
  pub struct TunneledMessageRequest {
    pub username: String,
    pub token: u32,
    pub code: u32,
    pub message: String,
  }
}

wire_struct! {
  /// Server code 68: a message a peer tunnelled to us through the server. Obsolete: the live server
  /// no longer sends it.
  pub struct TunneledMessageResponse {
    pub username: String,
    pub code: u32,
    pub token: u32,
    pub ip: Ipv4Addr,
    pub port: u32,
    pub message: String,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::model::UserPresence;
  use crate::server::users::tests::{stats_bytes, user};
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{IP, ip, p, vectors};

  vectors! {
    send_speed_request: Q::SendSpeed(SendSpeed { username: "bob".into(), speed: 1000 }) => p().s("bob").u32(1000).frame(34);
    queued_downloads_request: Q::QueuedDownloads(QueuedDownloadsRequest { slots_full: true }) => p().u32(1).frame(40);
    queued_downloads_response:
      R::QueuedDownloads(QueuedDownloadsResponse { username: "bob".into(), slots_full: false }) => p().s("bob").u32(0).frame(40);
    admin_command_request:
      Q::AdminCommand(AdminCommand { command: "ban".into(), args: vec!["bob".into(), "1d".into()] })
      => p().s("ban").u32(2).s("bob").s("1d").frame(58);
    place_in_line_request_request:
      Q::PlaceInLineRequest(PlaceInLineRequest { username: "bob".into(), token: 4 }) => p().s("bob").u32(4).frame(59);
    place_in_line_request_response:
      R::PlaceInLineRequest(PlaceInLineRequest { username: "bob".into(), token: 4 }) => p().s("bob").u32(4).frame(59);
    place_in_line_response_request:
      Q::PlaceInLineResponse(PlaceInLineResponse { username: "bob".into(), token: 4, place: 2 })
      => p().s("bob").u32(4).u32(2).frame(60);
    place_in_line_response_response:
      R::PlaceInLineResponse(PlaceInLineResponse { username: "bob".into(), token: 4, place: 9 })
      => p().s("bob").u32(4).u32(9).frame(60);
    global_user_list_request: Q::GlobalUserList => p().frame(67);
    global_user_list_response:
      R::GlobalUserList(GlobalUserListResponse { users: vec![user("bob", UserPresence::Online, false, "NL")] })
      => stats_bytes(p().u32(1).s("bob").u32(1).u32(2).u32(1)).u32(1).u32(0).u32(1).s("NL").frame(67);
    tunneled_message_request:
      Q::TunneledMessage(TunneledMessageRequest { username: "bob".into(), token: 1, code: 2, message: "m".into() })
      => p().s("bob").u32(1).u32(2).s("m").frame(68);
    tunneled_message_response:
      R::TunneledMessage(TunneledMessageResponse {
        username: "bob".into(), code: 2, token: 1, ip: ip(), port: 2234, message: "m".into(),
      })
      => p().s("bob").u32(2).u32(1).ip(IP).u32(2234).s("m").frame(68);
  }
}
