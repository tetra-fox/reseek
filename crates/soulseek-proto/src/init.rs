//! The first frame on an inbound peer socket (spec 01 section 3.2), and the raw values a
//! transfer (`F`) socket carries after it (3.3). Any init decode error means the socket should
//! be closed, including an invalid username or an unknown connection type.

use crate::codec::{Wire, family, wire_struct};
use crate::codes::InitCode;
use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::PeerConnectionType;
use crate::names::is_valid_peer_username;
use crate::wire::{Reader, Writer};

family! {
  pub enum InitMessage {
    code: InitCode,
    frame: Init,
    label: "init",
    quiet: [],
    messages: {
      /// sent by a peer connecting because the server relayed our `ConnectToPeer`
      PierceFirewall(PierceFirewall),
      /// sent by a peer connecting directly
      PeerInit(PeerInit),
    }
  }
}

wire_struct! {
  pub struct PierceFirewall {
    /// the token of the `ConnectToPeer` that solicited the connection
    pub token: u32,
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct PeerInit {
  /// the connecting peer's own username, unauthenticated
  #[cfg_attr(test, proptest(strategy = "crate::proptests::peer_username()"))]
  pub username: String,
  pub connection_type: PeerConnectionType,
  /// written as 0 and ignored on receipt
  pub token: u32,
}

impl Wire for PeerInit {
  const MIN_BYTES: usize = 12;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let username = r.string()?;
    if !is_valid_peer_username(&username) {
      return Err(DecodeError::InvalidUsername { what: "peer init" });
    }
    Ok(Self {
      username,
      connection_type: PeerConnectionType::read(r)?,
      token: r.u32()?,
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.username);
    self.connection_type.write(w);
    w.u32(self.token);
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.username = Some(&self.username);
  }
}

/// The `u32` token the initiator of a transfer connection writes raw after the init frame
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileTransferInit {
  pub token: u32,
}

impl FileTransferInit {
  pub fn encode(&self) -> [u8; 4] {
    self.token.to_le_bytes()
  }

  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    raw_exact(bytes).map(|bytes| Self {
      token: u32::from_le_bytes(bytes),
    })
  }
}

/// The `u64` offset the downloader writes raw on a transfer connection. It is for resuming
/// only; an offset past the file size (Soulseek NS sends `u64::MAX` when resuming past 2 GiB)
/// must be refused by the uploader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileOffset {
  pub offset: u64,
}

impl FileOffset {
  pub fn encode(&self) -> [u8; 8] {
    self.offset.to_le_bytes()
  }

  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    raw_exact(bytes).map(|bytes| Self {
      offset: u64::from_le_bytes(bytes),
    })
  }
}

fn raw_exact<const N: usize>(bytes: &[u8]) -> Result<[u8; N], DecodeError> {
  let error = match bytes.first_chunk::<N>() {
    Some(raw) if bytes.len() == N => return Ok(*raw),
    Some(_) => DecodeError::TrailingBytes {
      extra: bytes.len() - N,
    },
    None => DecodeError::Truncated {
      needed: N,
      remaining: bytes.len(),
    },
  };
  tracing::warn!(error = %error, "raw transfer value rejected");
  Err(error)
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::limits::LIMIT_INIT_MESSAGE_BYTES;
  use crate::testing::{decodes, p, vectors};

  vectors! {
    pierce_firewall: InitMessage::PierceFirewall(PierceFirewall { token: 77 }) => p().u32(77).frame_u8(0);
    peer_init:
      InitMessage::PeerInit(PeerInit { username: "bob".into(), connection_type: PeerConnectionType::Peer, token: 0 })
      => p().s("bob").s("P").u32(0).frame_u8(1);
  }

  decodes! {
    peer_init_keeps_a_non_zero_token: InitMessage, p().s("bob").s("F").u32(1234).frame_u8(1)
      => Ok(InitMessage::PeerInit(PeerInit {
        username: "bob".into(), connection_type: PeerConnectionType::Transfer, token: 1234,
      }));
    peer_init_rejects_an_unknown_type: InitMessage, p().s("bob").s("X").u32(0).frame_u8(1)
      => Err(DecodeError::UnknownConnectionType);
    peer_init_rejects_the_reserved_username: InitMessage, p().s("server").s("P").u32(0).frame_u8(1)
      => Err(DecodeError::InvalidUsername { what: "peer init" });
    peer_init_rejects_an_empty_username: InitMessage, p().s("").s("P").u32(0).frame_u8(1)
      => Err(DecodeError::InvalidUsername { what: "peer init" });
    peer_init_rejects_a_frame_over_the_limit: InitMessage, p().raw(&[0; 4096]).frame_u8(1)
      => Err(DecodeError::FrameTooLarge { len: 4097, limit: LIMIT_INIT_MESSAGE_BYTES });
  }

  #[test]
  fn file_transfer_init() {
    let raw = 77u32.to_le_bytes();
    assert_eq!(FileTransferInit { token: 77 }.encode(), raw);
    assert_eq!(
      FileTransferInit::decode(&raw),
      Ok(FileTransferInit { token: 77 })
    );
    assert_eq!(
      FileTransferInit::decode(&raw[..3]),
      Err(DecodeError::Truncated {
        needed: 4,
        remaining: 3
      })
    );
  }

  #[test]
  fn file_offset() {
    let raw = 1_048_576u64.to_le_bytes();
    assert_eq!(FileOffset { offset: 1_048_576 }.encode(), raw);
    assert_eq!(
      FileOffset::decode(&raw),
      Ok(FileOffset { offset: 1_048_576 })
    );
    // the Soulseek NS resume bug arrives as u64::MAX; decoding keeps it for the uploader to refuse
    assert_eq!(
      FileOffset::decode(&[0xff; 8]),
      Ok(FileOffset { offset: u64::MAX })
    );
    assert_eq!(
      FileOffset::decode(&[0; 9]),
      Err(DecodeError::TrailingBytes { extra: 1 })
    );
  }
}
