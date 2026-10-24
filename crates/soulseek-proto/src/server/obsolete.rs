//! Legacy messages kept as codecs only (spec 01a section 2) that belong to no current topic.

use std::net::Ipv4Addr;

use super::rooms::{read_user_lists, write_user_lists};
use super::users::UserData;
use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::wire::{Reader, Writer};

wire_struct! {
  /// Server code 34, spec 01a section 2.4, obsolete `SendDownloadSpeed`
  pub struct SendSpeed {
    pub username: String,
    pub speed: u32,
  }
}

/// Server code 40, spec 01a section 2.5, obsolete `UploadSlotsFull`, a boolean carried as `u32`
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct QueuedDownloadsRequest {
  pub slots_full: bool,
}

impl Wire for QueuedDownloadsRequest {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      slots_full: r.u32()? != 0,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(u32::from(self.slots_full));
  }
}

/// Server code 40, spec 01a section 2.5
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct QueuedDownloadsResponse {
  pub username: String,
  pub slots_full: bool,
}

impl Wire for QueuedDownloadsResponse {
  const MIN_BYTES: usize = 8;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string()?,
      slots_full: r.u32()? != 0,
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
  /// Server code 58, spec 01a section 2.8, obsolete
  pub struct AdminCommand {
    pub command: String,
    pub args: Vec<String>,
  }
}

wire_struct! {
  /// Server code 59, spec 01a section 2.9, obsolete
  pub struct PlaceInLineRequest {
    pub username: String,
    pub token: u32,
  }
}

wire_struct! {
  /// Server code 60, spec 01a section 2.9, obsolete
  pub struct PlaceInLineResponse {
    pub username: String,
    pub token: u32,
    pub place: u32,
  }
}

/// Server code 67, spec 01a section 2.12, obsolete: the same five parallel lists as `JoinRoom`,
/// without a room name
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct GlobalUserListResponse {
  pub users: Vec<UserData>,
}

impl Wire for GlobalUserListResponse {
  const MIN_BYTES: usize = 20;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
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
  /// Server code 68, spec 01a section 2.13, obsolete; the field order differs from the response
  pub struct TunneledMessageRequest {
    pub username: String,
    pub token: u32,
    pub code: u32,
    pub message: String,
  }
}

wire_struct! {
  /// Server code 68, spec 01a section 2.13, obsolete
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
