//! Private messages, admin broadcasts and the public chat feed (spec 01 sections 5.13, 5.14, 5.26,
//! 5.46, 01a section 2.14).

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::wire::{Reader, Writer};

wire_struct! {
  /// Server code 22, spec 01 section 5.13
  pub struct PrivateMessageRequest {
    pub username: String,
    pub message: String,
  }
}

/// Server code 22, spec 01 section 5.13
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct PrivateMessageResponse {
  /// acknowledge with `AcknowledgePrivateMessage`
  pub id: u32,
  /// unix seconds, UTC
  pub timestamp: u32,
  pub username: String,
  pub message: String,
  /// false when replayed from the server's offline store
  pub is_new: bool,
}

impl Wire for PrivateMessageResponse {
  const MIN_BYTES: usize = 17;

  /// `is_new` is true only for exactly 1
  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    Ok(Self {
      id: r.u32()?,
      timestamp: r.u32()?,
      username: r.string()?,
      message: r.string()?,
      is_new: r.bool_exact()?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.id);
    w.u32(self.timestamp);
    w.string(&self.username);
    w.string(&self.message);
    w.bool(self.is_new);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
    fields.token = Some(self.id);
  }
}

wire_struct! {
  /// Server code 23, spec 01 section 5.14
  pub struct AcknowledgePrivateMessage {
    pub id: u32,
  }
}

wire_struct! {
  /// Server code 66, spec 01 section 5.26
  pub struct GlobalAdminMessage {
    pub message: String,
  }
}

wire_struct! {
  /// Server code 149, spec 01a section 2.14, codec only
  pub struct MessageUsers {
    pub usernames: Vec<String>,
    pub message: String,
  }
}

wire_struct! {
  /// Server code 152, spec 01 section 5.46
  pub struct PublicChat {
    pub room: String,
    pub username: String,
    pub message: String,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{decodes, p, vectors};

  vectors! {
    private_message_request:
      Q::PrivateMessage(PrivateMessageRequest { username: "bob".into(), message: "hello\nthere".into() })
      => p().s("bob").s("hello\nthere").frame(22);
    private_message_response:
      R::PrivateMessage(PrivateMessageResponse {
        id: 5, timestamp: 1_700_000_000, username: "bob".into(), message: "yo".into(), is_new: true,
      })
      => p().u32(5).u32(1_700_000_000).s("bob").s("yo").u8(1).frame(22);
    private_message_response_replayed:
      R::PrivateMessage(PrivateMessageResponse {
        id: 5, timestamp: 1_700_000_000, username: "server".into(), message: "x".into(), is_new: false,
      })
      => p().u32(5).u32(1_700_000_000).s("server").s("x").u8(0).frame(22);
    acknowledge_private_message_request:
      Q::AcknowledgePrivateMessage(AcknowledgePrivateMessage { id: 99 }) => p().u32(99).frame(23);
    global_admin_message_response:
      R::GlobalAdminMessage(GlobalAdminMessage { message: "maintenance".into() }) => p().s("maintenance").frame(66);
    message_users_request:
      Q::MessageUsers(MessageUsers { usernames: vec!["bob".into(), "eve".into()], message: "hey".into() })
      => p().u32(2).s("bob").s("eve").s("hey").frame(149);
    ask_public_chat_request: Q::AskPublicChat => p().frame(150);
    stop_public_chat_request: Q::StopPublicChat => p().frame(151);
    public_chat_response:
      R::PublicChat(PublicChat { room: "indie".into(), username: "bob".into(), message: "hi".into() })
      => p().s("indie").s("bob").s("hi").frame(152);
  }

  decodes! {
    private_message_response_is_new_must_be_exactly_one: R, p().u32(1).u32(2).s("bob").s("m").u8(2).frame(22)
      => Ok(R::PrivateMessage(PrivateMessageResponse {
        id: 1, timestamp: 2, username: "bob".into(), message: "m".into(), is_new: false,
      }));
  }
}
