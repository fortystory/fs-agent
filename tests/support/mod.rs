//! Shared test support for the end-to-end seam.
//!
//! The fake provider is the acceptance instrument: neither real vendor has a
//! `seed`, so scripted responses are the only reproducible way to drive a
//! session. Later tickets extend its scripting; they do not add new seams.

#![allow(dead_code)]
// Each integration test crate compiles this module separately and uses a
// different subset of the fixtures.
#![allow(unused_imports)]

mod asker;
mod capture;
mod fake_provider;
mod hook;

pub use asker::{AlwaysAllow, ScriptedAsker};
pub use capture::CaptureBuf;
pub use fake_provider::{FakeProvider, Reply};
pub use hook::{PostCall, PreCall, ScriptedHook};
