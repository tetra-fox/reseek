//! Server messages about our place in the distributed search tree (spec 01 sections 5.28 to 5.30,
//! 5.33, 5.34, 5.43). The embedded message body is [`crate::distributed::Embedded`].

use std::net::Ipv4Addr;

use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::wire::{Reader, Writer};

wire_struct! {
  /// Server code 71, spec 01 section 5.28
  pub struct HaveNoParents {
    pub have_no_parents: bool,
  }
}

/// Server code 73, spec 01 section 5.28, deprecated, never sent; an empty payload means no parent
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ParentsIP {
  pub ip: Option<Ipv4Addr>,
}

impl Wire for ParentsIP {
  const MIN_BYTES: usize = 0;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      ip: if r.has_more() {
        Some(r.ip4("ip")?)
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    if let Some(ip) = self.ip {
      w.ip4(ip);
    }
  }
}

wire_struct! {
  /// Server code 83, spec 01 section 5.29
  pub struct ParentMinSpeed {
    pub speed: u32,
  }
}

wire_struct! {
  /// Server code 84, spec 01 section 5.29
  pub struct ParentSpeedRatio {
    pub ratio: u32,
  }
}

wire_struct! {
  /// Server code 86, spec 01 section 5.30, obsolete
  pub struct ParentInactivityTimeout {
    pub seconds: u32,
  }
}

wire_struct! {
  /// Server code 87, spec 01 section 5.30, obsolete
  pub struct SearchInactivityTimeout {
    pub seconds: u32,
  }
}

wire_struct! {
  /// Server code 88, spec 01 section 5.30, obsolete
  pub struct MinimumParentsInCache {
    pub count: u32,
  }
}

wire_struct! {
  /// Server code 90, spec 01 section 5.30, obsolete
  pub struct DistributedAliveInterval {
    pub seconds: u32,
  }
}

wire_struct! {
  /// Server code 100, spec 01 section 5.28
  pub struct AcceptChildren {
    pub accept: bool,
  }
}

wire_struct! {
  /// Server code 102, spec 01 section 5.34
  ///
  /// Parent candidates, sent after `HaveNoParents(true)`
  pub struct NetInfo {
    pub parents: Vec<ParentCandidate>,
  }
}

wire_struct! {
  pub struct ParentCandidate {
    pub username: String,
    pub ip: Ipv4Addr,
    pub port: u32,
  }
}

wire_struct! {
  /// Server code 126, spec 01 section 5.28
  pub struct BranchLevel {
    pub level: u32,
  }
}

wire_struct! {
  /// Server code 127, spec 01 section 5.28
  pub struct BranchRoot {
    pub root: String,
  }
}

wire_struct! {
  /// Server code 129, spec 01 section 5.28, obsolete, never sent
  pub struct ChildDepth {
    pub depth: u32,
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::distributed::Embedded;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{IP, ip, p, vectors};

  fn embedded_search() -> Vec<u8> {
    p().u32(49).s("bob").u32(5).s("query").0
  }

  vectors! {
    have_no_parents_request: Q::HaveNoParents(HaveNoParents { have_no_parents: true }) => p().u8(1).frame(71);
    parents_ip_request: Q::ParentsIP(ParentsIP { ip: Some(ip()) }) => p().ip(IP).frame(73);
    parents_ip_request_without_parent: Q::ParentsIP(ParentsIP { ip: None }) => p().frame(73);
    parent_min_speed_response: R::ParentMinSpeed(ParentMinSpeed { speed: 1 }) => p().u32(1).frame(83);
    parent_speed_ratio_response: R::ParentSpeedRatio(ParentSpeedRatio { ratio: 50 }) => p().u32(50).frame(84);
    parent_inactivity_timeout_response:
      R::ParentInactivityTimeout(ParentInactivityTimeout { seconds: 300 }) => p().u32(300).frame(86);
    search_inactivity_timeout_response:
      R::SearchInactivityTimeout(SearchInactivityTimeout { seconds: 0 }) => p().u32(0).frame(87);
    minimum_parents_in_cache_response: R::MinimumParentsInCache(MinimumParentsInCache { count: 10 }) => p().u32(10).frame(88);
    distributed_alive_interval_response:
      R::DistributedAliveInterval(DistributedAliveInterval { seconds: 0 }) => p().u32(0).frame(90);
    embedded_message_response:
      R::EmbeddedMessage(Embedded { distributed_code: 3, payload: embedded_search() })
      => p().u8(3).raw(&embedded_search()).frame(93);
    accept_children_request: Q::AcceptChildren(AcceptChildren { accept: false }) => p().u8(0).frame(100);
    net_info_response:
      R::NetInfo(NetInfo { parents: vec![ParentCandidate { username: "fast".into(), ip: ip(), port: 2234 }] })
      => p().u32(1).s("fast").ip(IP).u32(2234).frame(102);
    branch_level_request: Q::BranchLevel(BranchLevel { level: 2 }) => p().u32(2).frame(126);
    branch_root_request: Q::BranchRoot(BranchRoot { root: "root".into() }) => p().s("root").frame(127);
    child_depth_request: Q::ChildDepth(ChildDepth { depth: 1 }) => p().u32(1).frame(129);
    distributed_reset_response: R::DistributedReset => p().frame(130);
  }
}
