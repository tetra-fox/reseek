//! The first frame on an inbound peer socket (spec 01 section 3.2), and the raw values a transfer
//! (`F`) socket carries after it (3.3).

use crate::codes::InitCode;
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::Limits;
use crate::log::{self, KeyFields};
use crate::model::PeerConnectionType;
use crate::names::is_valid_peer_username;
use crate::wire::{Reader, Writer};

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum InitMessage {
  /// sent by a peer connecting because the server relayed our `ConnectToPeer`
  PierceFirewall { token: u32 },
  /// sent by a peer connecting directly
  PeerInit {
    /// the connecting peer's own username, unauthenticated
    #[cfg_attr(test, proptest(strategy = "crate::proptests::peer_username()"))]
    username: String,
    connection_type: PeerConnectionType,
    /// written as 0 and ignored on receipt
    token: u32,
  },
}

impl InitMessage {
  pub fn code(&self) -> InitCode {
    match self {
      Self::PierceFirewall { .. } => InitCode::PierceFirewall,
      Self::PeerInit { .. } => InitCode::PeerInit,
    }
  }

  /// the complete frame, length prefix included
  pub fn encode(&self) -> Vec<u8> {
    let mut w = Writer::frame_u8(self.code() as u8);
    match self {
      Self::PierceFirewall { token } => w.u32(*token),
      Self::PeerInit {
        username,
        connection_type,
        token,
      } => {
        w.string(username);
        w.string(connection_type.as_str());
        w.u32(*token);
      }
    }
    w.finish()
  }

  /// Decodes one complete frame. Any error, including an invalid username or an unknown
  /// connection type, means the socket should be closed.
  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    let frame = frame::exact(FrameKind::Init, bytes, &Limits::DEFAULT)?;
    let code = InitCode::from_code(frame.code);
    let message = match code {
      Some(code) => Self::read(code, frame.payload),
      None => Err(DecodeError::UnknownCode { code: frame.code }),
    }
    .inspect_err(|error| log::rejected("init", frame.code, code.map(InitCode::name), error))?;
    let fields = match &message {
      Self::PierceFirewall { token } => KeyFields::token(*token),
      Self::PeerInit { username, .. } => KeyFields::username(username),
    };
    log::decoded("init", frame.code, message.code().name(), fields);
    Ok(message)
  }

  fn read(code: InitCode, payload: &[u8]) -> Result<Self, DecodeError> {
    let r = &mut Reader::new(payload);
    Ok(match code {
      InitCode::PierceFirewall => Self::PierceFirewall { token: r.u32()? },
      InitCode::PeerInit => {
        let username = r.string()?;
        if !is_valid_peer_username(&username) {
          return Err(DecodeError::InvalidUsername { what: "peer init" });
        }
        Self::PeerInit {
          username,
          connection_type: PeerConnectionType::parse(&r.string()?)?,
          token: r.u32()?,
        }
      }
    })
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
