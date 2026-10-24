//! The Soulseek wire protocol: framing, codecs for every server, peer, init and distributed
//! message, the compressed payloads and the data model. No I/O.
//!
//! Every message enum has `encode(&self) -> Vec<u8>`, which produces a complete frame, and
//! `decode(&[u8])`, which takes exactly one complete frame as [`frame::split`] returns it.
//! Decoders enforce the `LIMIT_*` constants and log what they decode at `debug` and what they
//! reject at `warn` (unknown codes at `debug`), so callers do not log decode errors again.

mod codes;
mod error;
pub mod frame;
mod limits;
mod wire;

pub use codes::{DistributedCode, InitCode, PeerCode, ServerCode};
pub use error::DecodeError;
pub use limits::*;
pub use wire::{RemoteFilename, Secret};
