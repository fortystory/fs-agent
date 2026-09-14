//! Context boundary: usable-input accounting, single-result truncation, and
//! budget-driven dropping (spec §10).
//!
//! Lands in ticket 07. This module exists now so the dependency DAG is fixed
//! from the first commit: `trim` is a pure step *after* projection, and dropping
//! only ever happens here, never in the event log.
