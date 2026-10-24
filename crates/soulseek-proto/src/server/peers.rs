//! Messages that set up connections between peers through the server.

use std::net::Ipv4Addr;

use super::session::Obfuscation;
use crate::codec::{Wire, wire_struct};
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::PeerConnectionType;
use crate::wire::{Reader, Writer};

wire_struct! {
  /// Server code 18: asks the server to tell a peer to connect to us, for when we cannot reach it
  /// directly.
  pub struct ConnectToPeerRequest {
    /// fresh; the peer sends it back in `PierceFirewall`
    pub token: u32,
    pub username: String,
    pub connection_type: PeerConnectionType,
  }
}

/// Server code 18: a peer asked the server to make us connect to it, with its address and the token
/// to send back in `PierceFirewall`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ConnectToPeerResponse {
  pub username: String,
  pub connection_type: PeerConnectionType,
  pub ip: Ipv4Addr,
  pub port: u32,
  /// send back in `PierceFirewall`
  pub token: u32,
  pub privileged: bool,
  /// never dialled
  pub obfuscation: Option<Obfuscation>,
}

impl Wire for ConnectToPeerResponse {
  const MIN_BYTES: usize = 21;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      username: r.string("username")?,
      connection_type: PeerConnectionType::read(r, "connection_type")?,
      ip: r.ip4("ip")?,
      port: r.u32("port")?,
      token: r.u32("token")?,
      privileged: r.bool("privileged")?,
      obfuscation: r.optional_fixed("obfuscation")?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    self.connection_type.write(w);
    w.ip4(self.ip);
    w.u32(self.port);
    w.u32(self.token);
    w.bool(self.privileged);
    if let Some(obfuscation) = &self.obfuscation {
      obfuscation.write(w);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
    fields.token = Some(self.token);
  }
}

wire_struct! {
  /// Server code 33: a token to check a peer connection against, relayed by the server. Obsolete:
  /// no client sends it.
  pub struct SendConnectToken {
    pub username: String,
    pub token: u32,
  }
}

wire_struct! {
  /// Server code 1001: tells the server we could not honour a peer's `ConnectToPeer`, so the server
  /// can tell that peer.
  pub struct CannotConnectRequest {
    pub token: u32,
    /// the peer we could not reach
    pub username: String,
  }
}

/// Server code 1001: the peer we asked, through the server, to connect to us could not reach us.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct CannotConnectResponse {
  pub token: u32,
  /// the live server sends only the token
  pub username: Option<String>,
}

impl Wire for CannotConnectResponse {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    Ok(Self {
      token: r.u32("token")?,
      username: r.optional("username")?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.token);
    if let Some(username) = &self.username {
      w.string(username);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.token = Some(self.token);
    fields.username = self.username.as_deref();
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::error::FieldError;
  use crate::server::{ServerRequest as Q, ServerResponse as R};
  use crate::testing::{IP, decodes, field, ip, p, vectors};

  vectors! {
    connect_to_peer_request:
      Q::ConnectToPeer(ConnectToPeerRequest { token: 7, username: "bob".into(), connection_type: PeerConnectionType::Peer })
      => p().u32(7).s("bob").s("P").frame(18);
    connect_to_peer_response:
      R::ConnectToPeer(ConnectToPeerResponse {
        username: "bob".into(), connection_type: PeerConnectionType::Transfer, ip: ip(), port: 2234, token: 77,
        privileged: false, obfuscation: Some(Obfuscation { obfuscation_type: 1, obfuscated_port: 2235 }),
      })
      => p().s("bob").s("F").ip(IP).u32(2234).u32(77).u8(0).u32(1).u32(2235).frame(18);
    connect_to_peer_response_without_trailer:
      R::ConnectToPeer(ConnectToPeerResponse {
        username: "bob".into(), connection_type: PeerConnectionType::Distributed, ip: ip(), port: 2234, token: 77,
        privileged: true, obfuscation: None,
      })
      => p().s("bob").s("D").ip(IP).u32(2234).u32(77).u8(1).frame(18);
    send_connect_token_request:
      Q::SendConnectToken(SendConnectToken { username: "bob".into(), token: 5 }) => p().s("bob").u32(5).frame(33);
    send_connect_token_response:
      R::SendConnectToken(SendConnectToken { username: "bob".into(), token: 5 }) => p().s("bob").u32(5).frame(33);
    cannot_connect_request:
      Q::CannotConnect(CannotConnectRequest { token: 12, username: "bob".into() }) => p().u32(12).s("bob").frame(1001);
    cannot_connect_response: R::CannotConnect(CannotConnectResponse { token: 12, username: None }) => p().u32(12).frame(1001);
    cannot_connect_response_with_username:
      R::CannotConnect(CannotConnectResponse { token: 12, username: Some("bob".into()) }) => p().u32(12).s("bob").frame(1001);
  }

  decodes! {
    connect_to_peer_response_rejects_unknown_type: R,
      p().s("bob").s("X").ip(IP).u32(1).u32(1).u8(0).frame(18)
      => Err(field("ConnectToPeer", "connection_type", FieldError::UnknownConnectionType));
  }
}
