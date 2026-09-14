//! Configuration boundary.
//!
//! Library code never reads the environment: every value here is injected by
//! the caller. Resolving `config.toml`, environment variables and built-in
//! defaults happens in `cli` (and lands with ticket 02).
//!
//! Vocabulary: a **Turn** is one provider call plus its tool execution; the
//! limit below counts turns within a single agent loop.

/// Default maximum provider calls in one turn (spec §3).
pub const DEFAULT_MAX_ITERATIONS: u32 = 100;

/// Values the assembly entry injects into a single-agent session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Model identifier used for the provider call.
    pub model: String,
    /// Hard cap on provider calls in one turn.
    pub max_iterations: u32,
}

impl SessionConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }

    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}
