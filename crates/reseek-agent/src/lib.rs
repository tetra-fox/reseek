//! The relay agent: a small program that shares a machine's files through a reseek daemon
//! elsewhere, its controller. It keeps a share index of its own, dials out to the controller,
//! uploads the index, and sends a file's bytes when a peer downloads it through the controller;
//! it may also copy the controller's finished downloads. It opens no port and needs no Soulseek
//! login.
//!
//! [`main`] is the program; its options are the few of the daemon's it needs, under the same
//! names. [`open_socket`] dials a controller's relay socket, for anything else that speaks to it
//! as an agent.

#![warn(missing_docs)]

mod cli;
mod client;
mod options;
mod start;

pub use client::dial::{Socket, open_socket};
pub use start::main;

/// The version this agent tells its controller.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
