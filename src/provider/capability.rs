//! The capability table: model id -> what that model actually supports.
//!
//! Built by model id, modeling only the two vendors this crate speaks to. It is
//! `#[non_exhaustive]` so a new capability is not a breaking change, and an
//! unregistered id is an **error**, never a silent downgrade (spec §4): the
//! adapter either knows a model's shape or it refuses to guess.
//!
//! Sources for the numbers are the vendors' own API references:
//! Kimi (`kimi-k3`, 1M context, `max_completion_tokens` up to 1048576,
//! `reasoning_effort` low/high/max, `cached_tokens`, `prompt_cache_key`, and a
//! cache floor of 256 prompt tokens) and DeepSeek (`deepseek-flash` /
//! `deepseek-v4-pro`, 1M context, 384K max output, `prompt_cache_hit_tokens` /
//! `prompt_cache_miss_tokens`, no `prompt_cache_key`).

use std::fmt;

use crate::config::Vendor;

/// The wire parameter carrying the output-token cap. Kimi deprecated
/// `max_tokens` in favor of `max_completion_tokens`; DeepSeek still documents
/// `max_tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTokensField {
    MaxTokens,
    MaxCompletionTokens,
}

impl MaxTokensField {
    pub fn field_name(&self) -> &'static str {
        match self {
            MaxTokensField::MaxTokens => "max_tokens",
            MaxTokensField::MaxCompletionTokens => "max_completion_tokens",
        }
    }
}

/// What one model id supports. Every field is a fact about the vendor, not a
/// policy of this crate.
///
/// Some entries are read by later tickets rather than by the adapter: this is
/// the single home for the facts the spec pins (context windows for §10's
/// usable-input budget, the cache floor for §17's accounting, the reasoning
/// replay contract for §5's projection), so they are not re-guessed there.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ModelCaps {
    pub vendor: Vendor,
    /// Total context window, input plus output.
    pub context_window: u32,
    /// Largest value the output-token cap may take. Never larger than
    /// `context_window`: the cap is a slice of the same window.
    pub max_output_tokens: u32,
    pub supports_tools: bool,
    /// Returns `reasoning_content`.
    pub supports_reasoning: bool,
    /// Accepts the top-level `reasoning_effort` tier.
    pub supports_reasoning_effort: bool,
    /// Whether the model's own `reasoning_content` must be replayed on a
    /// request that carries tools; dropping it is a vendor-side error
    /// (DeepSeek 400s). Projection (§5) owns the replay; the flag documents why
    /// it is not optional.
    pub requires_reasoning_replay: bool,
    pub supports_temperature: bool,
    pub supports_top_p: bool,
    /// Accepts `prompt_cache_key` (Kimi); DeepSeek's cache is automatic and the
    /// parameter does not exist.
    pub supports_prompt_cache_key: bool,
    /// Accepts `stream_options: {include_usage: true}`, which yields a final
    /// usage-only chunk before `[DONE]`.
    pub supports_stream_options: bool,
    pub max_tokens_field: MaxTokensField,
    /// Prompt-token count below which the vendor does not cache at all.
    pub min_cacheable_tokens: u32,
}

/// Every model id this crate models, in a stable order for diagnostics.
pub const KNOWN_MODELS: &[&str] = &[
    // Kimi Open Platform.
    "kimi-k3",
    // Kimi Code (coding plan); K3 is exposed to it as `k3` / `k3-256k`.
    "k3",
    "k3-256k",
    "kimi-for-coding",
    "kimi-for-coding-highspeed",
    // DeepSeek.
    "deepseek-flash",
    "deepseek-v4-pro",
];

/// Look up a model id. Unknown ids are an error, never a default.
pub fn caps_for(model: &str) -> Result<ModelCaps, UnknownModel> {
    let caps = match model {
        // The same K3 model, under its Open Platform id and its coding-plan ids.
        "kimi-k3" | "k3" => k3_caps(1_048_576),
        "k3-256k" => k3_caps(262_144),
        // Kimi Code's K2.x models. `kimi-for-coding` is K2.8 Preview (takes an
        // effort tier); `kimi-for-coding-highspeed` is K2.7 Code with thinking
        // always on and no tier.
        "kimi-for-coding" => kimi_code_k2_caps(1_048_576, true),
        "kimi-for-coding-highspeed" => kimi_code_k2_caps(262_144, false),
        "deepseek-v4-pro" | "deepseek-flash" => deepseek_caps(),
        other => return Err(UnknownModel::new(other)),
    };
    Ok(caps)
}

/// K3, whether reached through the Open Platform or Kimi Code.
///
/// K3 documents no output cap tighter than the window, and fixes sampling at
/// temperature 1.0 / top_p 0.95 with a request not to send them.
fn k3_caps(context_window: u32) -> ModelCaps {
    ModelCaps {
        vendor: Vendor::Kimi,
        context_window,
        max_output_tokens: context_window,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort: true,
        requires_reasoning_replay: true,
        supports_temperature: false,
        supports_top_p: false,
        supports_prompt_cache_key: true,
        supports_stream_options: true,
        max_tokens_field: MaxTokensField::MaxCompletionTokens,
        min_cacheable_tokens: 257,
    }
}

fn kimi_code_k2_caps(context_window: u32, supports_reasoning_effort: bool) -> ModelCaps {
    ModelCaps {
        vendor: Vendor::Kimi,
        context_window,
        max_output_tokens: context_window,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort,
        requires_reasoning_replay: true,
        supports_temperature: true,
        supports_top_p: true,
        supports_prompt_cache_key: true,
        supports_stream_options: true,
        max_tokens_field: MaxTokensField::MaxCompletionTokens,
        min_cacheable_tokens: 257,
    }
}

fn deepseek_caps() -> ModelCaps {
    ModelCaps {
        vendor: Vendor::DeepSeek,
        context_window: 1_048_576,
        // DeepSeek documents a hard 384K output cap below its 1M window.
        max_output_tokens: 393_216,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort: true,
        requires_reasoning_replay: true,
        supports_temperature: true,
        supports_top_p: true,
        supports_prompt_cache_key: false,
        supports_stream_options: true,
        max_tokens_field: MaxTokensField::MaxTokens,
        min_cacheable_tokens: 0,
    }
}

/// An unregistered model id. This is deliberately loud: a new model must be
/// added to the table with sourced numbers before it can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownModel {
    pub model: String,
}

impl UnknownModel {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }
}

impl fmt::Display for UnknownModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown model `{}`: it is not in the capability table, and fs-agent never guesses \
             capabilities. Known model ids: {}",
            self.model,
            KNOWN_MODELS.join(", ")
        )
    }
}

impl std::error::Error for UnknownModel {}
