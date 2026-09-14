//! Permission boundary (spec §12): rules, modes, the circuit breaker, and
//! delegation-chain inheritance.
//!
//! Lands in ticket 04. The gate is a pure function of `(policy, tool, args)`; it
//! never reads conversation text, and it never reads the environment — the
//! "no interactive answerer means Ask becomes Deny" downgrade happens in the
//! loop, outside the gate.
