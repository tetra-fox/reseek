//! Private room membership and operators (spec 01 section 5.44).

use crate::codec::wire_struct;

wire_struct! {
  /// Server code 133, spec 01 section 5.44
  ///
  /// Members of a private room, excluding the owner
  pub struct PrivateRoomUsers {
    pub room: String,
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  /// Server code 134, spec 01 section 5.44
  pub struct PrivateRoomAddUser {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 135, spec 01 section 5.44
  pub struct PrivateRoomRemoveUser {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 136, spec 01 section 5.44
  pub struct PrivateRoomDropMembership {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 137, spec 01 section 5.44
  pub struct PrivateRoomDropOwnership {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 138, spec 01 section 4.1, obsolete
  pub struct PrivateRoomUnknown {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 139, spec 01 section 5.44
  pub struct PrivateRoomAdded {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 140, spec 01 section 5.44
  pub struct PrivateRoomRemoved {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 141, spec 01 section 5.44
  pub struct PrivateRoomToggle {
    pub accept_invitations: bool,
  }
}

wire_struct! {
  /// Server code 143, spec 01 section 5.44
  pub struct PrivateRoomAddOperator {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 144, spec 01 section 5.44
  pub struct PrivateRoomRemoveOperator {
    pub room: String,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 145, spec 01 section 5.44
  pub struct PrivateRoomOperatorAdded {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 146, spec 01 section 5.44
  pub struct PrivateRoomOperatorRemoved {
    pub room: String,
  }
}

wire_struct! {
  /// Server code 148, spec 01 section 5.44
  ///
  /// The operators of a private room we are in
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
