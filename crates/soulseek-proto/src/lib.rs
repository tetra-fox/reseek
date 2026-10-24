//! The Soulseek wire protocol: framing, codecs for every server, peer, init and distributed
//! message, the compressed payloads and the data model. No I/O.
//!
//! Every message enum has `encode(&self) -> Vec<u8>`, which produces a complete frame, and
//! `decode(&[u8])`, which takes exactly one complete frame as [`frame::split`] returns it.
//! Decoders enforce the `LIMIT_*` constants and log what they decode at `debug` and what they
//! reject at `warn` (unknown codes at `debug`), so callers do not log decode errors again.

mod codes;
mod compressed;
mod distributed;
mod error;
pub mod frame;
mod init;
mod limits;
mod log;
mod model;
mod names;
mod peer;
mod server_request;
mod server_response;
mod wire;

#[cfg(test)]
mod proptests;

pub use codes::{DistributedCode, InitCode, PeerCode, ServerCode};
pub use compressed::FolderContentsResponse;
pub use distributed::{DistributedMessage, Embedded};
pub use error::DecodeError;
pub use init::{FileOffset, FileTransferInit, InitMessage};
pub use limits::*;
pub use model::*;
pub use names::{
  LoginUsernameError, RoomNameError, SERVER_USERNAME, validate_login_username, validate_room_name,
};
pub use peer::{PeerMessage, TransferRequest, TransferResponse, peek_search_response};
pub use server_request::{
  PROTOCOL_MAJOR_VERSION, PROTOCOL_MINOR_VERSION, ServerRequest, login_hash,
};
pub use server_response::{
  LoginResponse, ParentCandidate, RelatedTerm, ServerResponse, SimilarUser, WatchedUser,
};
pub use wire::{RemoteFilename, Secret};
