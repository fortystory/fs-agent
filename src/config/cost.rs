//! Cost and the session's spend cap (spec §17).
//!
//! # Two budgets, never to be confused
//!
//! [`crate::context::usable_input`] is the **window**: how much input one call
//! may carry, computed per agent from **its own** model, enforced by dropping
//! the oldest droppable material. [`Budget`] is the **session's cumulative
//! allowance**: one hard stop shared by the debaters, the synthesizer and every
//! executor they dispatch, enforced by degrading and wrapping up.
//!
//! # Money is display only
//!
//! The gate reads **tokens**. [`Pricing`] exists so a human can see what those
//! tokens cost, and it never decides anything (spec §17). Prices are quoted per
//! million tokens — the unit both vendors publish — and `cached` and `miss` are
//! priced apart because a prefix-cache hit is usually an order of magnitude
//! cheaper than a miss.
//!
//! Everything here is a value resolved from configuration, never state: the
//! spend a [`Budget`] is compared against is summed from the event stream
//! ([`crate::events::total_usage`]), so no lock guards it and `--continue`
//! cannot lose it (spec §10).

use std::collections::BTreeMap;

use crate::events::Usage;

/// The default pre-flight tolerance (spec §17).
///
/// A call is refused only when its estimated size exceeds `remaining * margin`.
/// The estimate is `chars / 4`, which is wrong by tens of percent on code and on
/// non-Latin text; comparing it straight against what is left (`margin = 1.0`)
/// refuses calls that would have fitted, which is the failure the spec names.
/// The observed cumulative sum stays the real gate, so a margin above one buys
/// fewer false refusals at the price of occasionally letting one call overshoot
/// — which the next gate then catches.
pub const DEFAULT_ESTIMATE_MARGIN: f64 = 1.5;

/// The session's cumulative token allowance (spec §17).
///
/// `limit: None` means no cap at all, which is v1's default: the mechanism is in
/// place and the number waits for data. An executor's allowance is **not** its
/// own — "independent budget" means its turn cap, never its money (spec §16) —
/// so this value travels into every nested session unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    /// Hard cap on the session's cumulative tokens, summed from every
    /// `UsageRecorded` on the stream.
    pub limit: Option<u64>,
    /// Pre-flight tolerance, as a multiple of what is left. See
    /// [`DEFAULT_ESTIMATE_MARGIN`].
    pub estimate_margin: f64,
}

impl Budget {
    /// No cap, with the default tolerance.
    pub fn new() -> Self {
        Self {
            limit: None,
            estimate_margin: DEFAULT_ESTIMATE_MARGIN,
        }
    }

    /// Cap the session at `tokens`. Zero is legal and stops before the first
    /// call, which is what makes "the synthesizer is the one call that cannot be
    /// skipped" testable.
    pub fn with_limit(mut self, tokens: u64) -> Self {
        self.limit = Some(tokens);
        self
    }

    /// Set the pre-flight tolerance.
    pub fn with_estimate_margin(mut self, margin: f64) -> Self {
        self.estimate_margin = margin;
        self
    }

    /// What is left of the allowance, or `None` when there is no cap.
    pub fn remaining(&self, spent: u64) -> Option<u64> {
        self.limit.map(|limit| limit.saturating_sub(spent))
    }

    /// The hard stop: has the session already spent its allowance?
    ///
    /// `spent` is the sum over the whole stream — debaters, synthesizer and
    /// executors alike — and never an estimate. The comparison is `>=`, so a
    /// session that lands exactly on its cap is done.
    pub fn is_exhausted(&self, spent: u64) -> bool {
        self.limit.is_some_and(|limit| spent >= limit)
    }

    /// The pre-flight rule: does a call estimated at `estimate` tokens still fit?
    ///
    /// Refusing here costs one call's worth of work; not refusing costs an
    /// overshoot the cumulative gate catches on the next round.
    pub fn admits_estimate(&self, spent: u64, estimate: u64) -> bool {
        let Some(remaining) = self.remaining(spent) else {
            return true;
        };
        let threshold = (remaining as f64 * self.estimate_margin).floor();
        (estimate as f64) <= threshold
    }

    /// The sentence the hard stop narrates when the allowance is gone, or `None`
    /// while there is room.
    ///
    /// The gate sites each append what they do about it — close the round, refuse
    /// the dispatch — so the fact itself is phrased once.
    pub fn exhausted_note(&self, spent: u64) -> Option<String> {
        self.is_exhausted(spent).then(|| {
            format!(
                "session token budget exhausted: {spent} tokens spent, {}",
                self.cap_text()
            )
        })
    }

    /// The sentence for a call the pre-flight estimate refuses.
    pub fn estimate_refusal_note(&self, estimate: u64) -> String {
        format!(
            "session token budget: a call estimated at ~{estimate} tokens does not fit {}",
            self.cap_text()
        )
    }

    /// The cap as the diagnostics above read it out.
    fn cap_text(&self) -> String {
        match self.limit {
            Some(limit) => format!("a cap of {limit} tokens"),
            None => "no cap".to_owned(),
        }
    }
}

impl Default for Budget {
    /// No cap, with the default tolerance — **not** a zero margin, which would
    /// refuse everything.
    fn default() -> Self {
        Self::new()
    }
}

/// One model's prices, in USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    /// Cache-**miss** input tokens: what a fresh prompt costs.
    pub miss_input_per_mtok: f64,
    /// Cache-**hit** input tokens. Pricing these at zero is a statement that the
    /// vendor does not charge for a hit, not a missing value.
    pub cached_input_per_mtok: f64,
    /// Output tokens, reasoning included.
    pub output_per_mtok: f64,
}

impl Pricing {
    pub fn new(miss_input_per_mtok: f64, cached_input_per_mtok: f64, output_per_mtok: f64) -> Self {
        Self {
            miss_input_per_mtok,
            cached_input_per_mtok,
            output_per_mtok,
        }
    }

    /// What one usage record cost, in USD. Display only (spec §17).
    pub fn cost(&self, usage: Usage) -> f64 {
        let (cached, miss) = self.billed_input(usage);
        (cached as f64 * self.cached_input_per_mtok
            + miss as f64 * self.miss_input_per_mtok
            + usage.output_tokens as f64 * self.output_per_mtok)
            / 1_000_000.0
    }

    /// The input tokens, split into the two classes the vendors distinguish.
    ///
    /// The adapters normalize `cached + miss == input`, but a `Usage` that
    /// carries only a total — a test, or a provider that reports no cache
    /// detail — must not be billed as free: the uncached remainder is charged at
    /// the miss price whenever the reported `miss` is smaller than it.
    fn billed_input(&self, usage: Usage) -> (u64, u64) {
        let cached = usage.cached_tokens.min(usage.input_tokens);
        let miss = usage
            .miss_tokens
            .max(usage.input_tokens.saturating_sub(cached));
        (cached, miss)
    }
}

/// The configured prices, keyed by wire model id (spec §17).
///
/// Keyed by model id rather than by provider profile, because the two debaters
/// are different models and a model id is what the capability table and
/// `[models.*]` already key on. A model with no entry has **no** cost rather than
/// a zero one: "unpriced" and "free" must not look the same in a report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PriceTable {
    entries: BTreeMap<String, Pricing>,
}

impl PriceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, model: impl Into<String>, pricing: Pricing) {
        self.entries.insert(model.into(), pricing);
    }

    pub fn with(mut self, model: impl Into<String>, pricing: Pricing) -> Self {
        self.set(model, pricing);
        self
    }

    pub fn pricing(&self, model: &str) -> Option<&Pricing> {
        self.entries.get(model)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What `usage` cost on `model`, or `None` when the model has no price.
    pub fn cost(&self, model: &str, usage: Usage) -> Option<f64> {
        self.pricing(model).map(|pricing| pricing.cost(usage))
    }
}

/// A place a cheaper model may be routed to (spec §17).
///
/// Exactly two exist: the synthesizer's one closing call, and the executors a
/// debater dispatches. A **debater is never routed** — heterogeneity is the
/// strongest diversity lever the protocol has (spec §15), and two sides on the
/// same model have stopped being heterogeneous — so there is deliberately no
/// third variant here and no debater-shaped call to
/// [`crate::config::SessionConfig::model_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingPoint {
    /// The synthesizer's single call, assembled by [`crate::assemble_discussion`].
    Synthesizer,
    /// The nested sessions `task` dispatches (`crate::agent::executor`).
    Executor,
}

/// Which model each landing point answers with, as configured (spec §17).
///
/// The session-level `[routing]` table. Both values are `None` by default, which
/// is v1's behaviour: everything answers with the discussion's model until there
/// is data to route on — the mechanism is in place, the numbers wait.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Routing {
    /// Model the synthesizer answers with.
    pub synthesizer_model: Option<String>,
    /// Model the executors answer with.
    pub executor_model: Option<String>,
}

impl Routing {
    /// Whether neither landing point is routed, which is v1's default.
    pub fn is_empty(&self) -> bool {
        self.synthesizer_model.is_none() && self.executor_model.is_none()
    }

    /// Apply the configured overrides to one agent's values.
    ///
    /// Note what is **not** here: a debater's model. There is no field for it and
    /// no call site that would read one.
    pub fn apply(&self, config: &mut crate::config::SessionConfig) {
        config.synthesizer_model = self.synthesizer_model.clone();
        config.executor_model = self.executor_model.clone();
    }
}
