//! The server connection: session, rooms, users, messaging, privileges and reconfiguring a running client.

#[path = "../support/mod.rs"]
mod support;

mod messaging;
mod private_rooms;
mod privileges_interests;
mod reconfigure;
mod rooms;
mod scripted;
mod session;
mod session_scripted;
mod users;
