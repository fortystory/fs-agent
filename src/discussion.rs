//! Discussion protocol boundary (spec §15).
//!
//! Lands in ticket 10. Crucially, `discussion` does **not** touch `provider`:
//! it only drives the `agent` turn loop, so the loop stays the single writer of
//! the event stream.
