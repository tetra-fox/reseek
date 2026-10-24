//! Private room membership and operators.

use crate::codec::wire_struct;

wire_struct! {
  /// Server code 133: the members of a private room we are in, the owner excluded.
  pub struct PrivateRoomUsers {
    pub room: String,
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  /// Server code 134: adds a member to a private room we own or operate; the server sends the same
  /// message to every member once done.
  pub struct PrivateRoomAddUser {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 135: removes a member from a private room we own or operate; the server sends the
  /// same message to every member once done.
  pub struct PrivateRoomRemoveUser {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 136: gives up our membership of a private room.
  pub struct PrivateRoomDropMembership {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 137: gives up our ownership of a private room.
  pub struct PrivateRoomDropOwnership {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 138: names a private room; its purpose is unknown. Obsolete: no longer used.
  pub struct PrivateRoomUnknown {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 139: we were made a member of a private room.
  pub struct PrivateRoomAdded {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 140: we are no longer a member of a private room.
  pub struct PrivateRoomRemoved {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 141: whether we accept private room invitations, sent after login; the server
  /// echoes it.
  pub struct PrivateRoomToggle {
    pub accept_invitations: bool,
  }
}

wire_struct! {
  /// Server code 143: makes a member an operator of a private room we own; the server sends the
  /// same message to every member once done.
  pub struct PrivateRoomAddOperator {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 144: removes an operator of a private room we own; the server sends the same
  /// message to every member once done.
  pub struct PrivateRoomRemoveOperator {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 145: we were made an operator of a private room.
  pub struct PrivateRoomOperatorAdded {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 146: we are no longer an operator of a private room.
  pub struct PrivateRoomOperatorRemoved {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 148: the operators of a private room we are in.
  pub struct PrivateRoomOwned {
    pub room: String,
    pub operators: Vec<String>,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{p, vectors};

  vectors! {
    private_room_users_response:
      R::PrivateRoomUsers(PrivateRoomUsers { room: "club".into(), usernames: vec!["bob".into()] })
      => p().s("club").u32(1).s("bob").frame(133);
    private_room_add_user_request:
      Q::PrivateRoomAddUser(PrivateRoomAddUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(134);
    private_room_add_user_response:
      R::PrivateRoomAddUser(PrivateRoomAddUser { room: "club".into(), username: "bob".into() }) => p().s("club").s("bob").frame(134);
    private_room_remove_user_request:
      Q::PrivateRoomRemoveUser(PrivateRoomRemoveUser { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(135);
    private_room_remove_user_response:
      R::PrivateRoomRemoveUser(PrivateRoomRemoveUser { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(135);
    private_room_drop_membership_request:
      Q::PrivateRoomDropMembership(PrivateRoomDropMembership { room: "club".into() }) => p().s("club").frame(136);
    private_room_drop_ownership_request:
      Q::PrivateRoomDropOwnership(PrivateRoomDropOwnership { room: "club".into() }) => p().s("club").frame(137);
    private_room_unknown_request: Q::PrivateRoomUnknown(PrivateRoomUnknown { room: "club".into() }) => p().s("club").frame(138);
    private_room_unknown_response: R::PrivateRoomUnknown(PrivateRoomUnknown { room: "club".into() }) => p().s("club").frame(138);
    private_room_added_response: R::PrivateRoomAdded(PrivateRoomAdded { room: "club".into() }) => p().s("club").frame(139);
    private_room_removed_response: R::PrivateRoomRemoved(PrivateRoomRemoved { room: "club".into() }) => p().s("club").frame(140);
    private_room_toggle_request:
      Q::PrivateRoomToggle(PrivateRoomToggle { accept_invitations: true }) => p().u8(1).frame(141);
    private_room_toggle_response:
      R::PrivateRoomToggle(PrivateRoomToggle { accept_invitations: false }) => p().u8(0).frame(141);
    private_room_add_operator_request:
      Q::PrivateRoomAddOperator(PrivateRoomAddOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(143);
    private_room_add_operator_response:
      R::PrivateRoomAddOperator(PrivateRoomAddOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(143);
    private_room_remove_operator_request:
      Q::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(144);
    private_room_remove_operator_response:
      R::PrivateRoomRemoveOperator(PrivateRoomRemoveOperator { room: "club".into(), username: "bob".into() })
      => p().s("club").s("bob").frame(144);
    private_room_operator_added_response:
      R::PrivateRoomOperatorAdded(PrivateRoomOperatorAdded { room: "club".into() }) => p().s("club").frame(145);
    private_room_operator_removed_response:
      R::PrivateRoomOperatorRemoved(PrivateRoomOperatorRemoved { room: "club".into() }) => p().s("club").frame(146);
    private_room_owned_response:
      R::PrivateRoomOwned(PrivateRoomOwned { room: "club".into(), operators: vec!["op".into()] })
      => p().s("club").u32(1).s("op").frame(148);
  }
}
