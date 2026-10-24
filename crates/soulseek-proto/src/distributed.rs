//! Messages on a `D` connection in the distributed search tree (spec 01 section 7), with 1-byte
//! codes.

use crate::codes::DistributedCode;
use crate::error::DecodeError;
use crate::frame::{self, FrameKind};
use crate::limits::Limits;
use crate::log::{self, KeyFields};
use crate::names::is_valid_peer_username;
use crate::wire::{Reader, Writer};

/// The constant first field of every distributed search request: ASCII `'1'`
const SEARCH_IDENTIFIER: u32 = 49;

/// A distributed message wrapped in an `EmbeddedMessage` (93)
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct Embedded {
  /// only 3 (`SearchRequest`) is ever embedded by the server
  pub distributed_code: u8,
  pub payload: Vec<u8>,
}

impl Embedded {
  /// the unwrapped distributed frame, which is what gets broadcast to children and what
  /// [`DistributedMessage::decode`] takes
  pub fn inner_frame(&self) -> Vec<u8> {
    let mut w = Writer::frame_u8(self.distributed_code);
    w.bytes(&self.payload);
    w.finish()
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub enum DistributedMessage {
  /// deprecated; empty in the protocol reference, a `u32` token from the original library
  Ping { token: Option<u32> },
  /// written with the identifier 49; any other identifier is rejected
  SearchRequest {
    username: String,
    token: u32,
    query: String,
  },
  /// a level of 0 means the sender is the branch root
  BranchLevel {
    #[cfg_attr(test, proptest(strategy = "0..=i32::MAX as u32"))]
    level: u32,
  },
  BranchRoot {
    #[cfg_attr(test, proptest(strategy = "crate::proptests::peer_username()"))]
    root: String,
  },
  /// obsolete, parsed and ignored
  ChildDepth { depth: u32 },
  /// deprecated: sent by parents running a SoulseekQt from before early 2026
  EmbeddedMessage {
    #[cfg_attr(test, proptest(strategy = "crate::proptests::distributed_embedded()"))]
    embedded: Embedded,
    /// the inner code was preceded by three zero bytes, as when a parent forwards the server's
    /// wrapper with its 4-byte code
    padded: bool,
  },
}

impl DistributedMessage {
  pub fn code(&self) -> DistributedCode {
    match self {
      Self::Ping { .. } => DistributedCode::Ping,
      Self::SearchRequest { .. } => DistributedCode::SearchRequest,
      Self::BranchLevel { .. } => DistributedCode::BranchLevel,
      Self::BranchRoot { .. } => DistributedCode::BranchRoot,
      Self::ChildDepth { .. } => DistributedCode::ChildDepth,
      Self::EmbeddedMessage { .. } => DistributedCode::EmbeddedMessage,
    }
  }

  /// the complete frame, length prefix included
  pub fn encode(&self) -> Vec<u8> {
    let mut w = Writer::frame_u8(self.code() as u8);
    match self {
      Self::Ping { token } => {
        if let Some(token) = token {
          w.u32(*token);
        }
      }
      Self::SearchRequest {
        username,
        token,
        query,
      } => {
        w.u32(SEARCH_IDENTIFIER);
        w.string(username);
        w.u32(*token);
        w.string(query);
      }
      Self::BranchLevel { level } => w.u32(*level),
      Self::BranchRoot { root } => w.string(root),
      Self::ChildDepth { depth } => w.u32(*depth),
      Self::EmbeddedMessage { embedded, padded } => {
        if *padded {
          w.bytes(&[0; 3]);
        }
        w.u8(embedded.distributed_code);
        w.bytes(&embedded.payload);
      }
    }
    w.finish()
  }

  /// decodes one complete frame, length prefix included
  pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
    let frame = frame::exact(FrameKind::Distributed, bytes, &Limits::DEFAULT)?;
    let code = DistributedCode::from_code(frame.code);
    let message = match code {
      Some(code) => Self::read(code, frame.payload),
      None => Err(DecodeError::UnknownCode { code: frame.code }),
    }
    .inspect_err(|error| {
      log::rejected(
        "distributed",
        frame.code,
        code.map(DistributedCode::name),
        error,
      )
    })?;
    // pings are too frequent to log
    if !matches!(message, Self::Ping { .. }) {
      log::decoded(
        "distributed",
        frame.code,
        message.code().name(),
        message.key_fields(),
      );
    }
    Ok(message)
  }

  fn read(code: DistributedCode, payload: &[u8]) -> Result<Self, DecodeError> {
    let r = &mut Reader::new(payload);
    Ok(match code {
      DistributedCode::Ping => Self::Ping {
        token: if r.remaining() >= 4 {
          Some(r.u32()?)
        } else {
          None
        },
      },
      DistributedCode::SearchRequest => {
        let identifier = r.u32()?;
        if identifier != SEARCH_IDENTIFIER {
          return Err(DecodeError::BadSearchIdentifier { identifier });
        }
        Self::SearchRequest {
          username: r.string()?,
          token: r.u32()?,
          query: r.string()?,
        }
      }
      DistributedCode::BranchLevel => {
        let level = r.i32()?;
        let level = u32::try_from(level).map_err(|_| DecodeError::NegativeBranchLevel { level })?;
        Self::BranchLevel { level }
      }
      DistributedCode::BranchRoot => {
        let root = r.string()?;
        if !is_valid_peer_username(&root) {
          return Err(DecodeError::InvalidUsername {
            what: "branch root",
          });
        }
        Self::BranchRoot { root }
      }
      DistributedCode::ChildDepth => Self::ChildDepth { depth: r.u32()? },
      DistributedCode::EmbeddedMessage => {
        let padded = payload.len() >= 4 && payload[..3] == [0; 3];
        if padded {
          r.bytes(3)?;
        }
        Self::EmbeddedMessage {
          embedded: Embedded {
            distributed_code: r.u8()?,
            payload: r.rest().to_vec(),
          },
          padded,
        }
      }
      DistributedCode::Unknown => return Err(DecodeError::UnknownCode { code: code.code() }),
    })
  }

  fn key_fields(&self) -> KeyFields<'_> {
    match self {
      Self::SearchRequest {
        username, token, ..
      } => KeyFields::username(username).with_token(*token),
      Self::BranchRoot { root } => KeyFields::username(root),
      Self::Ping { .. }
      | Self::BranchLevel { .. }
      | Self::ChildDepth { .. }
      | Self::EmbeddedMessage { .. } => KeyFields::default(),
    }
  }
}
