//! The analyzer: decoding a file, measuring its spectrum a window at a time, reading the
//! measures into a verdict and drawing the pictures. It knows nothing of the queue, the database
//! or the files the service keeps.

mod aggregate;
pub(crate) mod decode;
pub(super) mod engine;
mod flags;
pub(super) mod measure;
pub(super) mod picture;
pub(super) mod reading;
pub(super) mod thresholds;
mod totals;

#[cfg(test)]
pub(super) mod fixtures;
#[cfg(test)]
mod tests;
