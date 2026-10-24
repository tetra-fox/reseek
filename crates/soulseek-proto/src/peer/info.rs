//! The user info a peer shows about itself (spec 01 section 6.5).

use crate::codec::Wire;
use crate::error::DecodeError;
use crate::limits::LIMIT_USER_INFO_PICTURE_BYTES;
use crate::wire::{Reader, Writer};

/// Peer code 16, spec 01 section 6.5
///
/// The `InfoResponse` body
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct UserInfo {
  pub description: String,
  /// raw image file bytes
  pub picture: Option<Vec<u8>>,
  pub upload_slots: u32,
  pub queue_length: u32,
  pub has_free_upload_slot: bool,
  /// who may upload to this user: 0 no one, 1 everyone, 2 users in their list, 3 permitted
  /// users; absent from SoulseekQt
  pub upload_permitted: Option<u32>,
}

impl UserInfo {
  pub fn has_picture(&self) -> bool {
    self.picture.is_some()
  }
}

impl Wire for UserInfo {
  const MIN_BYTES: usize = 14;

  fn read(r: &mut Reader) -> Result<Self, DecodeError> {
    let description = r.description()?;
    let picture = if r.bool()? {
      let len = r.u32()? as usize;
      if len > LIMIT_USER_INFO_PICTURE_BYTES {
        return Err(DecodeError::LimitExceeded {
          what: "user info picture bytes",
          value: len as u64,
          limit: LIMIT_USER_INFO_PICTURE_BYTES as u64,
        });
      }
      Some(r.bytes(len)?.to_vec())
    } else {
      None
    };
    Ok(Self {
      description,
      picture,
      upload_slots: r.u32()?,
      queue_length: r.u32()?,
      has_free_upload_slot: r.bool()?,
      // Museek+ writes the slot flag as a u32, leaving 3 stray bytes that must not be read as this
      upload_permitted: if r.remaining() >= 4 {
        Some(r.u32()?)
      } else {
        None
      },
    })
  }

  fn write(&self, w: &mut Writer) {
    w.string(&self.description);
    w.bool(self.picture.is_some());
    if let Some(picture) = &self.picture {
      w.count(picture.len());
      w.bytes(picture);
    }
    w.u32(self.upload_slots);
    w.u32(self.queue_length);
    w.bool(self.has_free_upload_slot);
    if let Some(upload_permitted) = self.upload_permitted {
      w.u32(upload_permitted);
    }
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use crate::peer::PeerMessage;
  use crate::testing::{decodes, p, vectors};

  vectors! {
    info_request: PeerMessage::InfoRequest => p().frame(15);
    info_response:
      PeerMessage::InfoResponse(UserInfo {
        description: "hello".into(),
        picture: Some(vec![0x89, b'P', b'N', b'G']),
        upload_slots: 2,
        queue_length: 0,
        has_free_upload_slot: true,
        upload_permitted: Some(1),
      })
      => p().s("hello").u8(1).u32(4).raw(&[0x89, b'P', b'N', b'G']).u32(2).u32(0).u8(1).u32(1).frame(16);
    info_response_without_picture:
      PeerMessage::InfoResponse(UserInfo {
        description: String::new(),
        picture: None,
        upload_slots: 0,
        queue_length: 2_147_483_647,
        has_free_upload_slot: false,
        upload_permitted: None,
      })
      => p().s("").u8(0).u32(0).u32(2_147_483_647).u8(0).frame(16);
  }

  decodes! {
    info_response_ignores_museek_stray_bytes: PeerMessage, p().s("d").u8(0).u32(1).u32(0).u8(1).raw(&[0, 0, 0]).frame(16)
      => Ok(PeerMessage::InfoResponse(UserInfo {
        description: "d".into(), picture: None, upload_slots: 1, queue_length: 0, has_free_upload_slot: true,
        upload_permitted: None,
      }));
    info_response_rejects_a_picture_over_the_limit: PeerMessage, p().s("").u8(1).u32(16 << 20 | 1).frame(16)
      => Err(DecodeError::LimitExceeded { what: "user info picture bytes", value: 16 << 20 | 1, limit: 16 << 20 });
  }
}
