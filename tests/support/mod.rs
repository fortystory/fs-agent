//! Shared test support for the end-to-end seam.
//!
//! The fake provider is the acceptance instrument: neither real vendor has a
//! `seed`, so scripted responses are the only reproducible way to drive a
//! session. Later tickets extend its scripting; they do not add new seams.

#![allow(dead_code)]

mod capture;
mod fake_provider;

pub use capture::CaptureBuf;
pub use fake_provider::{FakeProvider, Reply};
