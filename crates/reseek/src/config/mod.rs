//! The configuration: every option, how the layers are loaded, what is valid, and how values
//! are shown without their secrets.

pub mod addresses;
pub mod api_key;
pub mod cidr;
pub(crate) mod duration;
pub mod enums;
pub mod load;
mod options;
pub mod patterns;
pub mod redact;
pub mod template;
pub mod validate;
pub mod view;

pub use options::*;
