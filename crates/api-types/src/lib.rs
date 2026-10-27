//! Every type that crosses reseek's HTTP API or its event stream, defined once.
//!
//! The daemon's handlers serialise these types and the OpenAPI document is generated from them,
//! so the web UI's generated types and the daemon cannot disagree. The JSON is camelCase,
//! `null` is always written rather than left out, timestamps are UTC RFC 3339 with `Z`, and an
//! error is `{ "error", "detail" }`. Clients must treat a missing field and `null` alike.
//!
//! One module per domain, so "where is the transfer type" has one answer: [`transfers`].
//! The event stream's frames are [`live::LiveFrame`].

#![warn(missing_docs)]

mod flags;

pub mod analysis;
pub mod application;
pub mod auth;
pub mod blocking;
pub mod chat;
pub mod conversations;
pub mod error;
pub mod events;
pub mod files;
pub mod live;
pub mod logs;
pub mod options;
pub mod paging;
pub mod profile;
pub mod relay;
pub mod reports;
pub mod rooms;
pub mod search;
pub mod session;
pub mod shares;
pub mod state;
pub mod telemetry;
pub mod tokens;
pub mod transfers;
pub mod users;

pub use error::ErrorBody;
pub use paging::Page;
