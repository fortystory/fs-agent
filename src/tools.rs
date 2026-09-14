//! Tool boundary (spec §7): the `Tool` trait, the runtime registry, and
//! side-effect classification.
//!
//! Lands in ticket 03. The registry is a runtime value carried by the session
//! (never a global static), so dynamic tools have a mount point and the tool
//! table stays fixed for the life of the prefix cache.
