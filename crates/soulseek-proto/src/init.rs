//! The first frame on an inbound peer socket, and the raw values a transfer (`F`) socket carries
//! after it. Any init decode error means the socket should be closed, including an invalid username
//! or an unknown connection type.

use crate::codec::{Wire, family, wire_struct};
use crate::codes::InitCode;
use crate::error::{DecodeError, FieldError};
use crate::log::KeyFields;
use crate::model::PeerConnectionType;
use crate::names::is_valid_peer_username;
use crate::wire::{Reader, Writer};

family! {
  /// The first frame on an inbound peer socket, which says why the peer connected
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
  /// Init code 0: the first frame of a connection a peer made because the server relayed our
  /// `ConnectToPeer`.
  pub struct PierceFirewall {
    /// the token of the `ConnectToPeer` that solicited the connection
    pub token: u32,
  }
}

/// Init code 1: the first frame of a connection a peer made directly, naming itself and the
/// connection type.
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

  fn read(r: &mut Reader, _field: &'static str) -> Result<Self, DecodeError> {
    let username = r.string("username")?;
    if !is_valid_peer_username(&username) {
      return Err(r.fail("username", FieldError::InvalidUsername));
    }
    Ok(Self {
      username,
      connection_type: PeerConnectionType::read(r, "connection_type")?,
      token: r.u32("token")?,
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
  /// the four raw bytes, little-endian
  pub fn encode(&self) -> [u8; 4] {
    self.token.to_le_bytes()
  }

  /// exactly the four raw bytes, nothing more
  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    raw_exact(bytes, "FileTransferInit", "token").map(|bytes| Self {
      token: u32::from_le_bytes(bytes),
    })
  }
}

/// The `u64` offset the downloader writes raw on a transfer connection. It is for resuming only;
/// an offset past the file size (Soulseek NS sends `u64::MAX` when resuming past 2 GiB) must be
/// refused by the uploader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileOffset {
  pub offset: u64,
}

impl FileOffset {
  /// the eight raw bytes, little-endian
  pub fn encode(&self) -> [u8; 8] {
    self.offset.to_le_bytes()
  }

  /// exactly the eight raw bytes, nothing more
  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    raw_exact(bytes, "FileOffset", "offset").map(|bytes| Self {
      offset: u64::from_le_bytes(bytes),
    })
  }
}

fn raw_exact<const N: usize>(
  bytes: &[u8],
  message: &'static str,
  field: &'static str,
) -> Result<[u8; N], DecodeError> {
  let error = match bytes.first_chunk::<N>() {
    Some(raw) if bytes.len() == N => return Ok(*raw),
    Some(_) => DecodeError::TrailingBytes {
      extra: bytes.len() - N,
    },
    None => DecodeError::Field {
      message,
      field,
      error: FieldError::Truncated {
        needed: N,
        remaining: bytes.len(),
      },
    },
  };
  tracing::warn!(error = %error, "raw transfer value rejected");
  Err(error)
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::limits::LIMIT_INIT_MESSAGE_BYTES;
  use crate::testing::{decodes, field, p, vectors};

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
      => Err(field("PeerInit", "connection_type", FieldError::UnknownConnectionType));
    peer_init_rejects_the_reserved_username: InitMessage, p().s("server").s("P").u32(0).frame_u8(1)
      => Err(field("PeerInit", "username", FieldError::InvalidUsername));
    peer_init_rejects_an_empty_username: InitMessage, p().s("").s("P").u32(0).frame_u8(1)
      => Err(field("PeerInit", "username", FieldError::InvalidUsername));
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
      Err(field(
        "FileTransferInit",
        "token",
        FieldError::Truncated {
          needed: 4,
          remaining: 3
        }
      ))
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
