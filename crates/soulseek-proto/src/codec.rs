//! The read/write pair every message and every field type implements, and the two macros the
//! family modules build their messages and enums from.

use std::net::Ipv4Addr;

use crate::error::DecodeError;
use crate::log::KeyFields;
use crate::model::{
  FileAttributeType, PeerConnectionType, RoomType, TransferDirection, UserPresence,
};
use crate::wire::{Reader, RemoteFilename, Secret, Writer};

/// A value with one wire layout, read and written in the same place
pub(crate) trait Wire: Sized {
  /// the fewest bytes a value can take, which bounds a list count before its loop starts
  const MIN_BYTES: usize;

  /// reads the value as `field` of a message; primitives and lists name it in their errors,
  /// structs name their own fields
  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError>;

  fn write(&self, w: &mut Writer);

  /// adds the value to the logged fields when `name` is one of the logged field names
  fn log<'a>(&'a self, _name: &'static str, _fields: &mut KeyFields<'a>) {}
}

impl Wire for u8 {
  const MIN_BYTES: usize = 1;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.u8(field)
  }

  fn write(&self, w: &mut Writer) {
    w.u8(*self);
  }
}

impl Wire for u32 {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.u32(field)
  }

  fn write(&self, w: &mut Writer) {
    w.u32(*self);
  }

  fn log<'a>(&'a self, name: &'static str, fields: &mut KeyFields<'a>) {
    if matches!(name, "token" | "id") {
      fields.token = Some(*self);
    }
  }
}

impl Wire for i32 {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.i32(field)
  }

  fn write(&self, w: &mut Writer) {
    w.i32(*self);
  }
}

impl Wire for u64 {
  const MIN_BYTES: usize = 8;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.u64(field)
  }

  fn write(&self, w: &mut Writer) {
    w.u64(*self);
  }
}

/// a `u8` where any non-zero value is true
impl Wire for bool {
  const MIN_BYTES: usize = 1;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.bool(field)
  }

  fn write(&self, w: &mut Writer) {
    w.bool(*self);
  }
}

impl Wire for Ipv4Addr {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.ip4(field)
  }

  fn write(&self, w: &mut Writer) {
    w.ip4(*self);
  }
}

impl Wire for String {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.string(field)
  }

  fn write(&self, w: &mut Writer) {
    w.string(self);
  }

  fn log<'a>(&'a self, name: &'static str, fields: &mut KeyFields<'a>) {
    match name {
      "username" | "root" => fields.username = Some(self),
      "room" => fields.room = Some(self),
      _ => {}
    }
  }
}

impl Wire for RemoteFilename {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.filename(field)
  }

  fn write(&self, w: &mut Writer) {
    w.filename(self);
  }

  fn log<'a>(&'a self, name: &'static str, fields: &mut KeyFields<'a>) {
    if matches!(name, "filename" | "directory_name") {
      fields.filename = Some(self.as_str());
    }
  }
}

impl Wire for Secret {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.string(field).map(Secret::new)
  }

  fn write(&self, w: &mut Writer) {
    w.string(self.expose());
  }
}

/// `u32 count`, then the elements
impl<T: Wire> Wire for Vec<T> {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    let count = r.count(T::MIN_BYTES, field)?;
    (0..count).map(|_| T::read(r, field)).collect()
  }

  fn write(&self, w: &mut Writer) {
    w.count(self.len());
    for element in self {
      element.write(w);
    }
  }

  fn log<'a>(&'a self, _name: &'static str, fields: &mut KeyFields<'a>) {
    fields.count = Some(self.len());
  }
}

impl Wire for UserPresence {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    Self::from_u32(r.u32(field)?).map_err(|error| r.fail(field, error))
  }

  fn write(&self, w: &mut Writer) {
    w.u32(*self as u32);
  }
}

impl Wire for TransferDirection {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    Self::from_u32(r.u32(field)?).map_err(|error| r.fail(field, error))
  }

  fn write(&self, w: &mut Writer) {
    w.u32(*self as u32);
  }
}

impl Wire for RoomType {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    Self::from_u32(r.u32(field)?).map_err(|error| r.fail(field, error))
  }

  fn write(&self, w: &mut Writer) {
    w.u32(*self as u32);
  }
}

impl Wire for FileAttributeType {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    r.u32(field).map(Self::from_u32)
  }

  fn write(&self, w: &mut Writer) {
    w.u32(self.to_u32());
  }
}

/// the one-character type string
impl Wire for PeerConnectionType {
  const MIN_BYTES: usize = 4;

  fn read(r: &mut Reader, field: &'static str) -> Result<Self, DecodeError> {
    Self::parse(&r.string(field)?).map_err(|error| r.fail(field, error))
  }

  fn write(&self, w: &mut Writer) {
    w.string(self.as_str());
  }
}

/// A struct whose layout is its fields in declaration order, each read and written with its own
/// `Wire` impl. Messages with optional or conditional fields implement `Wire` by hand.
macro_rules! wire_struct {
  (
    $(#[$meta:meta])*
    pub struct $name:ident {
      $( $(#[$field_meta:meta])* pub $field:ident: $ty:ty, )*
    }
  ) => {
    $(#[$meta])*
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[cfg_attr(test, derive(proptest_derive::Arbitrary))]
    pub struct $name {
      $( $(#[$field_meta])* pub $field: $ty, )*
    }

    impl $crate::codec::Wire for $name {
      const MIN_BYTES: usize = 0 $( + <$ty as $crate::codec::Wire>::MIN_BYTES )*;

      fn read(
        r: &mut $crate::wire::Reader,
        _field: &'static str,
      ) -> Result<Self, $crate::error::DecodeError> {
        Ok(Self { $( $field: $crate::codec::Wire::read(r, stringify!($field))?, )* })
      }

      fn write(&self, w: &mut $crate::wire::Writer) {
        $( $crate::codec::Wire::write(&self.$field, w); )*
      }

      fn log<'a>(&'a self, _name: &'static str, fields: &mut $crate::log::KeyFields<'a>) {
        $( $crate::codec::Wire::log(&self.$field, stringify!($field), fields); )*
      }
    }
  };
}
pub(crate) use wire_struct;

/// The message enum of one connection kind. Each variant is named after its code in `code` and
/// holds the message struct, or nothing for an empty payload; this is the one place a variant
/// is tied to its code. `quiet` variants are decoded without a debug line because they arrive
/// too often to log.
macro_rules! family {
  (@ignore $ty:ty) => {};
  (@bind $ty:ty, $binding:ident) => { $binding };
  (
    $(#[$meta:meta])*
    pub enum $name:ident {
      code: $code:ident,
      frame: $kind:ident,
      label: $label:literal,
      quiet: [$($quiet:ident),*],
      messages: {
        $( $(#[$variant_meta:meta])* $variant:ident $(($ty:ty))?, )*
      }
    }
  ) => {
    $(#[$meta])*
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[cfg_attr(test, derive(proptest_derive::Arbitrary))]
    pub enum $name {
      $( $(#[$variant_meta])* $variant $(($ty))?, )*
    }

    impl $name {
      pub fn code(&self) -> $code {
        match self {
          $( $name::$variant $(($crate::codec::family!(@bind $ty, _message)))? => $code::$variant, )*
        }
      }

      /// the complete frame, length prefix included
      pub fn encode(&self) -> Vec<u8> {
        let mut w = $crate::wire::Writer::frame($crate::frame::FrameKind::$kind, self.code().code());
        match self {
          $(
            $name::$variant $(($crate::codec::family!(@bind $ty, message)))? => {
              $( $crate::codec::family!(@ignore $ty); $crate::codec::Wire::write(message, &mut w); )?
            }
          )*
        }
        w.finish()
      }

      /// decodes one complete frame, length prefix included
      pub fn decode(bytes: &[u8]) -> Result<Self, $crate::error::DecodeError> {
        Self::decode_limited(bytes, &$crate::limits::Limits::DEFAULT)
      }

      fn decode_limited(
        bytes: &[u8],
        limits: &$crate::limits::Limits,
      ) -> Result<Self, $crate::error::DecodeError> {
        let frame = $crate::frame::exact($crate::frame::FrameKind::$kind, bytes, limits)?;
        let code = $code::from_code(frame.code);
        let message = match code {
          Some(code) => Self::read(code, &mut $crate::wire::Reader::new(frame.payload, limits, code.name())),
          None => Err($crate::error::DecodeError::UnknownCode { code: frame.code }),
        }
        .inspect_err(|error| $crate::log::rejected($label, frame.code, code.map($code::name), error))?;
        let quiet = match &message {
          $( $name::$quiet { .. } => true, )*
          _ => false,
        };
        if !quiet {
          $crate::log::decoded($label, frame.code, message.code().name(), message.key_fields());
        }
        Ok(message)
      }

      fn read(code: $code, r: &mut $crate::wire::Reader) -> Result<Self, $crate::error::DecodeError> {
        // the fallback is unreachable for a family that has a message for every code
        #[allow(unreachable_patterns)]
        Ok(match code {
          $( $code::$variant => $name::$variant $((<$ty as $crate::codec::Wire>::read(r, "")?))?, )*
          _ => return Err($crate::error::DecodeError::UnknownCode { code: code.code() }),
        })
      }

      fn key_fields(&self) -> $crate::log::KeyFields<'_> {
        let mut fields = $crate::log::KeyFields::default();
        match self {
          $(
            $name::$variant $(($crate::codec::family!(@bind $ty, message)))? => {
              $( $crate::codec::family!(@ignore $ty); $crate::codec::Wire::log(message, "", &mut fields); )?
            }
          )*
        }
        fields
      }
    }
  };
}
pub(crate) use family;
