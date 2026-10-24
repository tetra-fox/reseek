//! The Soulseek wire protocol: framing, codecs for every server, peer, init and distributed
//! message, the compressed payloads and the data model. No I/O.
//!
//! One module per message family: [`server`], [`peer`], [`distributed`] and [`init`]. Each
//! message is a struct, in the file of its spec topic for the larger families and reached through
//! its family module, whose wire layout is read and written in one place;
//! the family enum (`ServerRequest`, `PeerMessage`, ...) ties every variant to its code and has
//! `encode(&self) -> Vec<u8>`, which produces a complete frame, and `decode(&[u8])`, which takes
//! exactly one complete frame as [`frame::split`] returns it. Decoders enforce the `LIMIT_*`
//! constants and log what they decode at `debug` and what they reject at `warn` (unknown codes
//! at `debug`), so callers do not log decode errors again.

mod codec;
mod codes;
pub mod distributed;
mod error;
pub mod frame;
pub mod init;
mod limits;
mod log;
mod model;
mod names;
pub mod peer;
pub mod server;
mod wire;

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod testing;

pub use codes::{DistributedCode, InitCode, PeerCode, ServerCode};
pub use distributed::DistributedMessage;
pub use error::DecodeError;
pub use init::InitMessage;
pub use limits::*;
pub use model::*;
pub use names::{
  LoginUsernameError, RoomNameError, SERVER_USERNAME, validate_login_username, validate_room_name,
};
pub use peer::{PeerMessage, peek_search_response};
pub use server::{
  PROTOCOL_MAJOR_VERSION, PROTOCOL_MINOR_VERSION, ServerRequest, ServerResponse, login_hash,
};
pub use wire::{RemoteFilename, Secret};
