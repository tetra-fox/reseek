//! Privileges: who has them, how long ours last, and giving them away (spec 01 sections 5.27, 5.31,
//! 5.32, 5.40 to 5.42).

use crate::codec::wire_struct;

wire_struct! {
  /// Server code 69, spec 01 section 5.27
  pub struct PrivilegedUsers {
    pub usernames: Vec<String>,
  }
}

wire_struct! {
  /// Server code 91, spec 01 section 5.31, obsolete, the parser is kept
  pub struct AddPrivilegedUser {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 92, spec 01 section 5.32
  pub struct CheckPrivilegesResponse {
    pub seconds_remaining: u32,
  }
}

wire_struct! {
  /// Server code 122, spec 01 section 5.40, deprecated
  pub struct UserPrivilegesRequest {
    pub username: String,
  }
}

wire_struct! {
  /// Server code 122, spec 01 section 5.40, deprecated
  pub struct UserPrivilegesResponse {
    pub username: String,
    pub privileged: bool,
  }
}

wire_struct! {
  /// Server code 123, spec 01 section 5.41
  pub struct GivePrivileges {
    pub username: String,
    pub days: u32,
  }
}

wire_struct! {
  /// Server code 124, spec 01 section 5.42, deprecated
  pub struct NotifyPrivileges {
    pub id: u32,
    pub username: String,
  }
}

wire_struct! {
  /// Server code 125, spec 01 section 5.42, deprecated
  pub struct AcknowledgeNotifyPrivileges {
    pub id: u32,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{p, vectors};

  vectors! {
    privileged_users_response:
      R::PrivilegedUsers(PrivilegedUsers { usernames: vec!["bob".into(), "eve".into()] }) => p().u32(2).s("bob").s("eve").frame(69);
    add_privileged_user_response: R::AddPrivilegedUser(AddPrivilegedUser { username: "bob".into() }) => p().s("bob").frame(91);
    check_privileges_request: Q::CheckPrivileges => p().frame(92);
    check_privileges_response:
      R::CheckPrivileges(CheckPrivilegesResponse { seconds_remaining: 86_400 }) => p().u32(86_400).frame(92);
    user_privileges_request: Q::UserPrivileges(UserPrivilegesRequest { username: "bob".into() }) => p().s("bob").frame(122);
    user_privileges_response:
      R::UserPrivileges(UserPrivilegesResponse { username: "bob".into(), privileged: true }) => p().s("bob").u8(1).frame(122);
    give_privileges_request:
      Q::GivePrivileges(GivePrivileges { username: "bob".into(), days: 30 }) => p().s("bob").u32(30).frame(123);
    notify_privileges_response:
      R::NotifyPrivileges(NotifyPrivileges { id: 4, username: "bob".into() }) => p().u32(4).s("bob").frame(124);
    acknowledge_notify_privileges_request:
      Q::AcknowledgeNotifyPrivileges(AcknowledgeNotifyPrivileges { id: 6 }) => p().u32(6).frame(125);
  }
}
