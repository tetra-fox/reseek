//! Events: the bus that hands each raised event to its subscribers, and the event log that
//! keeps them in `events.db` for the events API.

mod bus;
pub(crate) mod log;
mod samples;

pub use bus::EventBus;
pub use log::EventLog;
