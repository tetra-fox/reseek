//! The reseek daemon: configuration, the HTTP host, authentication and the API.

#![warn(missing_docs)]

pub mod api;
pub mod app;
pub mod auth;
pub mod cli;
pub mod config;
pub mod crypto;
pub mod db;
pub mod events;
pub mod http;
pub mod live;
pub mod logging;
pub mod paths;
pub mod safety;
pub mod services;
pub mod state;
pub mod transfers;
pub mod ui;
pub mod version;

#[cfg(test)]
mod testing;
