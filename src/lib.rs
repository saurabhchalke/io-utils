//! This crate implements networking wrappers for measuring the amount of
//! used communcation, efficiently sending/receiving large messages over
//! slow networks, and using a single stream across multiple threads.
#![warn(
    unused,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms,
    missing_docs,
    clippy::all
)]

pub mod counting;
pub mod imux;
pub mod threaded;

#[cfg(test)]
mod tests;
