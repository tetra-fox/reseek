//! Which directory entries are kept out of what reseek shares and serves: hidden entries, which
//! Unix marks with a leading dot in the name and Windows with an attribute, and the entries
//! Windows marks as the system's own.

use std::ffi::OsStr;
use std::fs::Metadata;

#[cfg(windows)]
const HIDDEN: u32 = 0x2;
#[cfg(windows)]
const SYSTEM: u32 = 0x4;

/// Whether the entry named `name` is hidden. Windows keeps this in `metadata`, so there an
/// entry whose metadata was not read is not hidden.
#[cfg(not(windows))]
pub fn hidden(name: &OsStr, _metadata: Option<&Metadata>) -> bool {
  name.as_encoded_bytes().starts_with(b".")
}

/// Whether the entry named `name` is hidden. Windows keeps this in `metadata`, so there an
/// entry whose metadata was not read is not hidden.
#[cfg(windows)]
pub fn hidden(_name: &OsStr, metadata: Option<&Metadata>) -> bool {
  use std::os::windows::fs::MetadataExt;
  metadata.is_some_and(|m| m.file_attributes() & HIDDEN != 0)
}

/// Whether Windows marks the entry as the system's own; no entry is elsewhere.
#[cfg(windows)]
pub fn system(metadata: &Metadata) -> bool {
  use std::os::windows::fs::MetadataExt;
  metadata.file_attributes() & SYSTEM != 0
}

/// Whether Windows marks the entry as the system's own; no entry is elsewhere.
#[cfg(not(windows))]
pub fn system(_metadata: &Metadata) -> bool {
  false
}

/// Whether the entry is hidden or the system's own: the entries shares, archives, pruning and
/// the log file list leave out.
pub fn left_out(name: &OsStr, metadata: Option<&Metadata>) -> bool {
  hidden(name, metadata) || metadata.is_some_and(system)
}
