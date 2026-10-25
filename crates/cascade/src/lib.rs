//! Layered configuration that remembers where each value came from.
//!
//! A configuration is a struct deriving [`Cascade`]. Its values come from layers, each
//! replacing the one before per key: the defaults (`T::default()`), environment variables, TOML
//! files, the command line, and runtime overrides. A list or a map whose keys the user picks is
//! one value, replaced whole by the highest layer that sets it. [`Layers`] loads a
//! [`Loaded`]: the values, the [`Source`] of each, and warnings for keys nothing knows.
//!
//! A [`Watcher`] reloads when the files change or when asked, keeps the last good values when
//! a reload fails, and reports what changed grouped by when it takes effect ([`Apply`]). It
//! also sets and removes runtime [`Overrides`], and writes the files with [`FileEditor`] so
//! their comments survive. [`ConfigView`] is all of it in one serialisable document.
//!
//! ```
//! use std::collections::BTreeMap;
//! use cascade::{Cascade, Layers};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Default, Serialize, Deserialize, Cascade)]
//! #[cascade(env_alias_prefix(from = "APP_", to = "NEW_"))]
//! #[serde(default)]
//! struct Options {
//!   /// Logs more.
//!   #[cascade(env = "APP_DEBUG", cli = "-d|--debug", apply = "restart")]
//!   debug: bool,
//!   server: Server,
//!   /// Keys that may use the API, by name.
//!   #[cascade(user_keyed)]
//!   api_keys: BTreeMap<String, ApiKey>,
//! }
//!
//! #[derive(Clone, Default, Serialize, Deserialize, Cascade)]
//! #[serde(default)]
//! struct Server {
//!   /// The password to log in with.
//!   #[cascade(env = "APP_PASSWORD", apply = "reconnect", secret)]
//!   password: Option<String>,
//!   /// Read buffer size.
//!   #[cascade(env = "APP_BUFFER", kind = "size")]
//!   buffer: u64,
//! }
//!
//! #[derive(Clone, Default, Serialize, Deserialize, Cascade)]
//! struct ApiKey {
//!   #[cascade(secret)]
//!   key: String,
//! }
//!
//! let env = [("NEW_PASSWORD".to_string(), "hunter2".to_string())];
//! let loaded = Layers::<Options>::new().env(env).load().unwrap();
//! assert_eq!(loaded.values.server.password.as_deref(), Some("hunter2"));
//! assert_eq!(loaded.redacted()["server"]["password"], "*****");
//! ```
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

#![warn(missing_docs)]

// the derive's generated code names `::cascade`, which this makes work inside the crate too
extern crate self as cascade;

mod describe;
mod edit;
mod error;
mod files;
mod layers;
mod loaded;
mod overrides;
mod schema;
mod source;
mod tree;
mod view;
mod watcher;
mod write;

#[cfg(test)]
mod testing;

pub use cascade_derive::Cascade;
pub use describe::{Apply, Cascade, Kind, Shape};
pub use edit::FileEditor;
pub use error::{Error, Invalid};
pub use files::LoadedFile;
pub use layers::{Checked, Layers};
pub use loaded::{Loaded, NotApplied};
pub use overrides::{Override, Overrides};
pub use schema::{Field, REDACTED, Schema};
pub use source::Source;
pub use view::{ConfigView, FieldView, FileView, OverrideView};
pub use watcher::{ByApply, Change, Watch, Watcher, Written};
pub use write::writability;

#[doc(hidden)]
pub use describe::private as __private;

/// A dotted configuration key, such as `web.authentication.password`.
pub type Key = String;

/// What a whole configuration's type needs: a struct deriving [`Cascade`] that has defaults,
/// serialises, and is shared between threads.
pub trait Root:
  Cascade + Default + Clone + serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static
{
}

impl<T> Root for T where
  T: Cascade
    + Default
    + Clone
    + serde::Serialize
    + serde::de::DeserializeOwned
    + Send
    + Sync
    + 'static
{
}

/// Locks a mutex even when a thread panicked while holding it: no update to what it guards is
/// left half-made, so it stays usable.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
}
