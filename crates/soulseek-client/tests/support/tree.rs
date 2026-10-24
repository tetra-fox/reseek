//! Scripting the distributed tree around a client: the status it reports to the server, parent
//! candidates that answer as a parent must (or not), and children that join below it.

use std::net::Ipv4Addr;

use soulseek_proto::distributed::{BranchRoot, Embedded, EmbeddedMessage, SearchRequest};
use soulseek_proto::init::PeerInit;
use soulseek_proto::server::{ConnectToPeerResponse, NetInfo, ParentCandidate};
use soulseek_proto::{
  DistributedMessage, InitMessage, PeerConnectionType, ServerRequest, ServerResponse,
};

use super::ServerSide;
use super::peer::{FakePeer, PeerSide};

/// The status batch the client sends the server
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
  pub level: u32,
  pub root: String,
  pub accepts_children: bool,
  pub needs_parent: bool,
}

/// the next status batch the client sends, skipping everything before it
pub async fn status(server: &mut ServerSide) -> Status {
  let level = server
    .request_matching(|request| match request {
      ServerRequest::BranchLevel(level) => Some(level.level),
      _ => None,
    })
    .await;
  let ServerRequest::BranchRoot(root) = server.request().await else {
    panic!("the branch root follows the level");
  };
  let ServerRequest::AcceptChildren(accept) = server.request().await else {
    panic!("whether we take children follows the root");
  };
  let ServerRequest::HaveNoParents(parents) = server.request().await else {
    panic!("whether we need a parent ends the batch");
  };
  Status {
    level,
    root: root.root,
    accepts_children: accept.accept,
    needs_parent: parents.have_no_parents,
  }
}

/// the server message offering `candidates` as parents
pub fn net_info(candidates: &[(&str, &FakePeer)]) -> ServerResponse {
  ServerResponse::NetInfo(NetInfo {
    parents: candidates
      .iter()
      .map(|(username, peer)| ParentCandidate {
        username: (*username).into(),
        ip: Ipv4Addr::LOCALHOST,
        port: peer.port(),
      })
      .collect(),
  })
}

pub fn search(username: &str, token: u32, query: &str) -> Vec<u8> {
  DistributedMessage::SearchRequest(SearchRequest {
    username: username.into(),
    token,
    query: query.into(),
  })
  .encode()
}

/// a search wrapped the way the server wraps it, as an old parent forwards it; `padded` writes
/// the inner code as four bytes
pub fn wrapped_search(username: &str, token: u32, query: &str, padded: bool) -> Vec<u8> {
  let inner = search(username, token, query);
  DistributedMessage::EmbeddedMessage(EmbeddedMessage {
    embedded: Embedded {
      distributed_code: 3,
      payload: inner[5..].to_vec(),
    },
    padded,
  })
  .encode()
}

/// the server's own wrapping of a search, which it sends only to branch roots
pub fn embedded_search(username: &str, token: u32, query: &str) -> ServerResponse {
  let inner = search(username, token, query);
  ServerResponse::EmbeddedMessage(Embedded {
    distributed_code: 3,
    payload: inner[5..].to_vec(),
  })
}

pub fn branch_level(level: i32) -> Vec<u8> {
  let mut frame = 5u32.to_le_bytes().to_vec();
  frame.push(4);
  frame.extend(level.to_le_bytes());
  frame
}

pub fn branch_root(root: &str) -> Vec<u8> {
  DistributedMessage::BranchRoot(BranchRoot {
    username: root.into(),
  })
  .encode()
}

/// `count` relayed requests from peers that want the client as their parent, as one write
pub fn child_requests(count: u32) -> Vec<u8> {
  (0..count)
    .flat_map(|n| {
      ServerResponse::ConnectToPeer(ConnectToPeerResponse {
        username: format!("flood{n}"),
        connection_type: PeerConnectionType::Distributed,
        ip: Ipv4Addr::LOCALHOST,
        port: 1,
        token: 10_000 + n,
        privileged: false,
        obfuscation: None,
      })
      .encode()
    })
    .collect()
}

/// the next connection the client makes to `candidate`, introduced as ours for the tree
pub async fn approached(candidate: &FakePeer) -> PeerSide {
  let mut connection = candidate.accept().await;
  let InitMessage::PeerInit(PeerInit {
    connection_type, ..
  }) = connection.read_init().await
  else {
    panic!("a candidate is dialled directly");
  };
  assert_eq!(connection_type, PeerConnectionType::Distributed);
  connection
}

/// a child named `username` that joined below the client listening at `address`
pub async fn join_as_child(address: std::net::SocketAddr, username: &str) -> PeerSide {
  let mut child = PeerSide::connect(address).await;
  child
    .send(
      &InitMessage::PeerInit(PeerInit {
        username: username.into(),
        connection_type: PeerConnectionType::Distributed,
        token: 0,
      })
      .encode(),
    )
    .await;
  child
}

/// the branch level and root a child is told, which always come as the two frames together
pub async fn branch_information(child: &mut PeerSide) -> (u32, String) {
  let DistributedMessage::BranchLevel(level) = child.read_distributed().await else {
    panic!("the branch level comes first");
  };
  let DistributedMessage::BranchRoot(root) = child.read_distributed().await else {
    panic!("the branch root follows the level");
  };
  (level.level, root.username)
}
