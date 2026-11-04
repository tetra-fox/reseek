//! The relay protocol between a reseek controller and its agents, as both sides speak it: the
//! socket's frames, close codes and timings ([`frames`]), the credentials an agent proves its
//! secret with ([`credential`]), which versions may relay to each other ([`version`]), and how a
//! loop that keeps failing waits before it tries again ([`backoff`]).
//!
//! The messages themselves are in `api_types::relay`, with the rest of the API's types.

#![warn(missing_docs)]

pub mod backoff;
pub mod credential;
pub mod frames;
pub mod version;
