//! Hook boundary (spec §3, user stories I).
//!
//! Lands in ticket 05. A pre-hook's output is a *constraint*
//! (`Continue | Rewrite | Tighten(Ask|Deny) | Skip | Stop`); there is no
//! "relax" variant, so "hooks can only tighten" is an algebraic property rather
//! than a runtime check.
