//! Downloads and uploads: their connections, their stages, files sent unasked and the scripted
//! edge cases.

#[path = "../support/mod.rs"]
mod support;

mod connections;
mod download_stages;
mod downloads;
mod downloads_scripted;
mod offered;
mod uploads_scripted;
