//! The reseek daemon: configuration, the HTTP host, authentication and the API.

#![warn(missing_docs)]

pub mod activity;
pub mod api;
pub mod app;
pub mod auth;
pub mod cli;
pub mod clock;
pub mod config;
pub mod crypto;
pub mod databases;
pub mod db;
pub mod entries;
pub mod events;
pub mod files;
pub mod http;
pub mod live;
pub mod logging;
pub mod messaging;
pub mod network;
pub mod paths;
pub mod profile;
pub mod relay;
pub mod safety;
pub mod scripts;
pub mod searches;
pub mod services;
pub mod shares;
pub mod state;
pub mod sync;
pub mod telemetry;
pub mod transfers;
pub mod turns;
pub mod ui;
pub mod users;
pub mod version;

#[cfg(test)]
mod testing;
