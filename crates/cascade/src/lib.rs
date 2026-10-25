//! Layered configuration that remembers where each value came from.
//!
//! A configuration is a struct deriving [`Cascade`]; its [`Schema`] lists every leaf key with
//! its kind, default, environment variable, command-line flag, when a change to it takes effect
//! ([`Apply`]), whether it is secret, and its help.
//!
//! Field attributes, all optional:
//!
//! - `env = "NAME"`: the environment variable that sets the field; `env_alias = "OTHER"`, any
//!   number of times, more names for it. When several are set, the aliases win in the order
//!   given, then `env`. On a struct, `env_alias_prefix(from = "A_", to = "B_")` gives every
//!   field under it named `A_...` the alias `B_...`.
//! - `cli = "--name"` or `cli = "-n|--name"`: the command-line flag that sets the field.
//! - `apply = "live" | "reconnect" | "restart"`: when a change takes effect, `live` when not
//!   given. On a section it is the default for every field inside.
//! - `secret`: shown as `*****`, never logged. On a section, every field inside.
//! - `doc = "..."`: the field's help, the doc comment when not given.
//! - `kind = "bool" | "integer" | "float" | "string" | "duration" | "size"`: the field's kind
//!   when its type does not say, such as a size type or an enum written as a string.
//! - `user_keyed`: a map whose keys the user picks, one value as a whole.
//!
//! A field's key is its name, or its `#[serde(rename = "...")]`; `#[serde(skip)]` fields are
//! not part of the configuration.

// the derive's generated code names `::cascade`, which this makes work inside the crate too
extern crate self as cascade;

mod describe;
mod schema;
mod tree;

#[cfg(test)]
mod testing;

pub use cascade_derive::Cascade;
pub use describe::{Apply, Cascade, Kind, Shape};
pub use schema::{Field, REDACTED, Schema};

#[doc(hidden)]
pub use describe::private as __private;

/// A dotted configuration key, such as `web.authentication.password`.
pub type Key = String;
