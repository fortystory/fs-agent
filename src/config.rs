//! Configuration boundary.
//!
//! Library code never reads the environment: the resolution here is a pure
//! function over an explicit environment map ([`resolve`]), and `cli` is the
//! only place that reads the process environment and hands the snapshot in.
//!
//! # Two levels (spec §4)
//!
//! `[providers.*]` holds a `base_url` and the key that goes with it;
//! `[models.*]` references a provider by name and overrides generation
//! parameters. A model section's key **is** the wire model id, which is also
//! the capability-table key — so "unregistered model id" is a lookup failure in
//! [`crate::provider::capability`], not a silent downgrade.
//!
//! # Precedence
//!
//! `config.toml` > exported environment > built-in default, per field. A
//! project `.env` is **never** loaded: [`Config::load`] reads exactly the path
//! it is given and the environment map it is handed.
//!
//! # Vocabulary
//!
//! A **Turn** is one provider call plus its tool execution; `max_iterations`
//! counts turns within a single agent loop. A **round** is a discussion
//! protocol step (§15). A [`Budget`] is the session's cumulative token
//! allowance (§17) — not to be confused with the per-agent **window** budget in
//! [`crate::context`].

pub mod cost;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use cost::{Budget, LandingPoint, PriceTable, Pricing, Routing};

/// Default maximum provider calls in one turn (spec §3).
pub const DEFAULT_MAX_ITERATIONS: u32 = 100;

/// Default maximum provider calls in one executor's turn (spec §16): goose's
/// `GOOSE_SUBAGENT_MAX_TURNS`. An executor is there to do a job, not to run a
/// marathon, and its own cap is what stops one runaway executor from eating the
/// session's turn budget.
pub const DEFAULT_EXECUTOR_MAX_ITERATIONS: u32 = 25;

/// Default cap on how many executors one batch of tool calls runs at once
/// (spec §16): goose's `GOOSE_MAX_BACKGROUND_TASKS`.
///
/// A cost and rate gate, not a safety gate: write exclusion between executors is
/// the shared path locks' job, and this cap exists because N executors with N
/// turn budgets and growing contexts multiply.
pub const DEFAULT_MAX_PARALLEL_EXECUTORS: usize = 5;

/// Default cap on one tool result, in estimated tokens (spec §10, ticket 07):
/// Anthropic documents 25k as Claude Code's default tool-response limit.
pub const DEFAULT_MAX_TOOL_RESULT_TOKENS: u64 = 25_000;

/// Default repo-map budget, in estimated tokens (spec §9, ticket 09): aider
/// documents the same default for its `--map-tokens` switch.
pub const DEFAULT_REPO_MAP_TOKENS: u64 = 1_024;

/// Ceiling on a configured repo-map budget (spec §9, ticket 09): aider's source
/// clamps `--map-tokens` here, and so does this configuration. A fixed budget is
/// the point — the model cannot ask for a bigger map per call.
pub const MAX_REPO_MAP_TOKENS: u64 = 4_096;

/// Model used when no `default_model` is configured or exported.
pub const DEFAULT_MODEL: &str = "kimi-k3";

/// Which vendor a provider profile speaks to. Only these two are modeled;
/// `#[non_exhaustive]` keeps a third one from being assumed anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Vendor {
    Kimi,
    DeepSeek,
}

impl Vendor {
    /// Display name used in diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Vendor::Kimi => "Kimi",
            Vendor::DeepSeek => "DeepSeek",
        }
    }

    /// Hosts a key of this vendor is allowed to be paired with.
    ///
    /// Kimi has two separate systems that share a vendor: the Open Platform
    /// (`api.moonshot.cn` / `api.moonshot.ai`) and Kimi Code, the coding plan
    /// (`api.kimi.com`). Their keys are not interchangeable, but both are Kimi.
    pub fn hosts(&self) -> &'static [&'static str] {
        match self {
            Vendor::Kimi => &["api.moonshot.cn", "api.moonshot.ai", "api.kimi.com"],
            Vendor::DeepSeek => &["api.deepseek.com"],
        }
    }
}

/// One built-in `[providers.*]` profile: the endpoint and the environment
/// variables its key is read from. Kimi contributes two profiles because its
/// Open Platform and its coding plan are separate systems with separate keys.
pub struct BuiltinProvider {
    pub name: &'static str,
    pub vendor: Vendor,
    pub base_url: &'static str,
    pub key_env: &'static str,
    pub alt_key_envs: &'static [&'static str],
}

impl BuiltinProvider {
    /// Every environment variable that may carry this profile's key, primary
    /// first. One list shared by resolution and by error hints.
    pub fn key_envs(&self) -> Vec<&'static str> {
        let mut names = vec![self.key_env];
        names.extend(self.alt_key_envs.iter().copied());
        names
    }
}

/// The built-in profiles. `kimi` is the Open Platform and `kimi-code` is the
/// coding plan; `KIMI_API_KEY` belongs to the latter, matching Kimi's own
/// third-party-tool docs.
pub const BUILTIN_PROVIDERS: &[BuiltinProvider] = &[
    BuiltinProvider {
        name: "kimi",
        vendor: Vendor::Kimi,
        base_url: "https://api.moonshot.cn/v1",
        key_env: "MOONSHOT_API_KEY",
        alt_key_envs: &[],
    },
    BuiltinProvider {
        name: "kimi-code",
        vendor: Vendor::Kimi,
        base_url: "https://api.kimi.com/coding/v1",
        key_env: "KIMI_API_KEY",
        alt_key_envs: &["KIMI_CODE_API_KEY"],
    },
    BuiltinProvider {
        name: "deepseek",
        vendor: Vendor::DeepSeek,
        base_url: "https://api.deepseek.com",
        key_env: "DEEPSEEK_API_KEY",
        alt_key_envs: &[],
    },
];

fn builtin_provider(name: &str) -> Option<&'static BuiltinProvider> {
    BUILTIN_PROVIDERS
        .iter()
        .find(|builtin| builtin.name == name)
}

/// Reasoning tier. Both vendors accept it at the request top level, but only
/// some model ids honor it (spec §4: Kimi K3 / DeepSeek). The tier is fixed
/// when the session starts: changing it mid-session throws away the prefix
/// cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    High,
    Max,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::High => "high",
            ReasoningEffort::Max => "max",
        }
    }
}

/// Neutral generation parameters. Provider adapters filter these against the
/// model capability table and warn when they drop something explicitly set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerationParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Pinned for the whole session; never switched mid-session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// Where a provider's key came from. The source decides whether the key is
/// bound to a vendor host (spec §4: `base_url` must match the key's origin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// Written next to `base_url` in `config.toml`: the pairing is explicit.
    Config,
    /// Read from this environment variable.
    Env(String),
    /// Not found; building a provider for it is an error.
    Missing,
}

impl KeySource {
    pub fn env_var(&self) -> Option<&str> {
        match self {
            KeySource::Env(name) => Some(name),
            _ => None,
        }
    }
}

/// A resolved provider profile: one `base_url` + one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProfile {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub key_source: KeySource,
    /// The environment variable this profile reads its key from (or would,
    /// when the key is missing). Named in diagnostics so the fix is copyable.
    pub key_env: String,
    /// `Some` for the built-in vendor profiles, `None` for a custom entry.
    pub vendor: Option<Vendor>,
}

/// A resolved model entry: a wire model id, its provider, and its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelProfile {
    /// Wire model id, sent as `model` and used as the capability-table key.
    pub id: String,
    /// Name of the [`ProviderProfile`] this model is served by.
    pub provider: String,
    pub params: GenerationParams,
}

/// Fully resolved configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub default_model: String,
    pub providers: BTreeMap<String, ProviderProfile>,
    pub models: BTreeMap<String, ModelProfile>,
    /// What a million tokens cost per model, for display (spec §17). Keyed by
    /// model id; a model with no entry is reported as unpriced, never as free.
    pub pricing: PriceTable,
    /// The session's cumulative token allowance (spec §17), for display and for
    /// the gate a session assembles with.
    pub budget: Budget,
    /// Which model the two landing points answer with (spec §17). Empty by
    /// default: v1 runs everything on the discussion's model.
    pub routing: Routing,
}

impl Config {
    pub fn model(&self, id: &str) -> Option<&ModelProfile> {
        self.models.get(id)
    }

    pub fn provider(&self, name: &str) -> Option<&ProviderProfile> {
        self.providers.get(name)
    }

    /// The provider profile that serves `model_id`.
    pub fn provider_for(&self, model_id: &str) -> Option<&ProviderProfile> {
        let model = self.models.get(model_id)?;
        self.providers.get(&model.provider)
    }

    /// The per-agent values a session for `model_id` starts from: the model's
    /// generation parameters, the session's allowance, the price table it
    /// displays costs with, and the routing overrides (spec §17).
    ///
    /// One function rather than the same four lines at every assembly site, so
    /// "the `[budget]` table gates the session" is true wherever a session is
    /// assembled instead of only where someone remembered to copy it.
    pub fn session_config(&self, model_id: &str) -> Result<SessionConfig, ConfigError> {
        let (model, _) = self.resolve_model(Some(model_id))?;
        let mut config = SessionConfig::new(model_id).with_params(model.params.clone());
        config.pricing = self.pricing.clone();
        config.budget = self.budget.clone();
        self.routing.apply(&mut config);
        Ok(config)
    }

    /// Every model whose provider has a usable key. The CLI probe uses this to
    /// decide what it can actually run.
    pub fn models_with_keys(&self) -> Vec<&ModelProfile> {
        self.models
            .values()
            .filter(|model| {
                self.providers
                    .get(&model.provider)
                    .is_some_and(|provider| provider.api_key.is_some())
            })
            .collect()
    }

    /// Resolve the model to run, defaulting to `default_model`.
    pub fn resolve_model(
        &self,
        requested: Option<&str>,
    ) -> Result<(&ModelProfile, &ProviderProfile), ConfigError> {
        let id = requested.unwrap_or(&self.default_model);
        let model = self
            .models
            .get(id)
            .ok_or_else(|| ConfigError::UnknownModel {
                model: id.to_owned(),
            })?;
        let provider =
            self.providers
                .get(&model.provider)
                .ok_or_else(|| ConfigError::UnknownProvider {
                    model: id.to_owned(),
                    provider: model.provider.clone(),
                })?;
        Ok((model, provider))
    }
}

/// Environment snapshot. The library never reads the process environment, so
/// callers hand this in; tests build one directly.
pub type EnvMap = BTreeMap<String, String>;

/// Resolve configuration from in-memory inputs.
///
/// `file_text` is the contents of `config.toml` (`None` = no file). `env` is
/// the exported environment. Nothing else is consulted, which is what makes
/// "a project `.env` is not loaded" true by construction.
pub fn resolve(file_text: Option<&str>, env: &EnvMap) -> Result<Config, ConfigError> {
    let raw = match file_text {
        Some(text) => toml::from_str::<RawConfig>(text).map_err(|source| ConfigError::Parse {
            source: Box::new(source),
        })?,
        None => RawConfig::default(),
    };

    let providers = resolve_providers(&raw, env)?;
    let models = resolve_models(&raw, &providers)?;
    let pricing = resolve_pricing(&raw, &models)?;
    let budget = resolve_budget(raw.budget.as_ref())?;
    let routing = resolve_routing(raw.routing.as_ref(), &models)?;

    let default_model = raw
        .default_model
        .clone()
        .or_else(|| env.get("FS_AGENT_MODEL").filter(|v| !v.is_empty()).cloned())
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
    if !models.contains_key(&default_model) {
        return Err(ConfigError::UnknownModel {
            model: default_model,
        });
    }

    Ok(Config {
        default_model,
        providers,
        models,
        pricing,
        budget,
        routing,
    })
}

/// Read and resolve `config.toml` at `path`. A missing file is an error here;
/// callers that treat absence as "defaults only" pass `None` to [`resolve`].
pub fn load(path: &Path, env: &EnvMap) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.display().to_string(),
        source,
    })?;
    resolve(Some(&text), env)
}

/// Default config path: `$XDG_CONFIG_HOME/fs-agent/config.toml`, else
/// `$HOME/.config/fs-agent/config.toml`.
pub fn default_path(env: &EnvMap) -> PathBuf {
    let base = env
        .get("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("fs-agent").join("config.toml")
}

/// Default session store root: `$XDG_DATA_HOME/fs-agent/sessions`, else
/// `$HOME/.local/share/fs-agent/sessions` (spec §11).
///
/// `None` when neither variable is set: the CLI reports that rather than
/// inventing a directory to write a session into. The store itself takes its
/// root as an argument, so the library never reads this.
pub fn sessions_dir(env: &EnvMap) -> Option<PathBuf> {
    let base = env
        .get("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })?;
    Some(base.join("fs-agent").join("sessions"))
}

// --- raw TOML shape -------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    default_model: Option<String>,
    #[serde(default)]
    providers: BTreeMap<String, RawProvider>,
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
    /// `[pricing.<model-id>]`: what a million tokens cost (spec §17). Display
    /// only — the gate reads tokens.
    #[serde(default)]
    pricing: BTreeMap<String, RawPricing>,
    budget: Option<RawBudget>,
    routing: Option<RawRouting>,
}

/// The `[routing]` table (spec §17): the two landing points a cheaper model may
/// be routed to. There is no key for a debater, which is the point.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRouting {
    synthesizer_model: Option<String>,
    executor_model: Option<String>,
}

/// One `[pricing.<model-id>]` table, in USD per million tokens.
///
/// All three prices are required: a missing one would silently price a class of
/// tokens at zero, and "unpriced" already has its own, honest spelling (no table
/// at all).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPricing {
    /// Cache-miss input tokens.
    miss_input: f64,
    /// Cache-hit input tokens.
    cached_input: f64,
    /// Output tokens, reasoning included.
    output: f64,
}

/// The `[budget]` table (spec §17).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBudget {
    /// Hard cap on the session's cumulative tokens.
    session_tokens: Option<u64>,
    /// Pre-flight tolerance, as a multiple of what is left (default 1.5).
    estimate_margin: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    base_url: Option<String>,
    api_key: Option<String>,
    /// Name of the environment variable to read the key from, when it is not
    /// the vendor default.
    api_key_env: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    provider: Option<String>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    max_output_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffort>,
}

/// The built-in model entries: wire model id -> provider profile name. Every
/// id here must also exist in the capability table (asserted by a test).
///
/// The K3 series appears twice on purpose: `kimi-k3` is the Open Platform id
/// and `k3` / `k3-256k` are the Kimi Code (coding plan) ids for the same model.
pub const BUILTIN_MODELS: &[(&str, &str)] = &[
    ("kimi-k3", "kimi"),
    ("k3", "kimi-code"),
    ("k3-256k", "kimi-code"),
    ("kimi-for-coding", "kimi-code"),
    ("kimi-for-coding-highspeed", "kimi-code"),
    ("deepseek-v4-pro", "deepseek"),
    ("deepseek-flash", "deepseek"),
];

fn resolve_providers(
    raw: &RawConfig,
    env: &EnvMap,
) -> Result<BTreeMap<String, ProviderProfile>, ConfigError> {
    let mut providers = BTreeMap::new();

    let mut names: Vec<String> = BUILTIN_PROVIDERS
        .iter()
        .map(|builtin| builtin.name.to_owned())
        .collect();
    for name in raw.providers.keys() {
        if !names.iter().any(|known| known == name) {
            names.push(name.clone());
        }
    }

    for name in names {
        let builtin = builtin_provider(&name);
        let section = raw.providers.get(&name);

        // base_url: config.toml > exported env > built-in default.
        let env_base_url = env
            .get(&format!("{}_BASE_URL", env_prefix(&name)))
            .filter(|value| !value.is_empty())
            .cloned();
        let base_url = section
            .and_then(|section| section.base_url.clone())
            .or(env_base_url)
            .or_else(|| builtin.map(|builtin| builtin.base_url.to_owned()))
            .ok_or_else(|| ConfigError::ProviderWithoutBaseUrl {
                provider: name.clone(),
            })?;

        // key: config.toml > exported env > built-in default. A built-in
        // profile also answers to its alternate environment spellings.
        let (api_key, key_source, key_env) = resolve_key(&name, section, builtin, env);

        // Structurally block the cross-vendor 401: an environment-derived
        // vendor key may only be pointed at that vendor's hosts. The key's
        // origin decides, not the section name — `[providers.kimi]` with
        // `api_key_env = "DEEPSEEK_API_KEY"` is a DeepSeek key.
        let host = host_of(&base_url).ok_or_else(|| ConfigError::InvalidBaseUrl {
            provider: name.clone(),
            base_url: base_url.clone(),
        })?;
        let key_vendor = key_source
            .env_var()
            .and_then(vendor_of_key_env)
            .or_else(|| builtin.map(|builtin| builtin.vendor));
        if let (Some(vendor), KeySource::Env(key_env)) = (key_vendor, &key_source) {
            if !vendor.hosts().contains(&host.as_str()) {
                return Err(ConfigError::CrossVendorKey {
                    provider: name.clone(),
                    host,
                    key_env: key_env.clone(),
                    vendor: vendor.as_str(),
                    expected: vendor.hosts().join(" or "),
                });
            }
        }

        providers.insert(
            name.clone(),
            ProviderProfile {
                name,
                base_url,
                api_key,
                key_source,
                key_env,
                vendor: builtin.map(|builtin| builtin.vendor),
            },
        );
    }

    Ok(providers)
}

/// Returns `(key, source, recommended_env_var)`. The recommended variable is
/// named even when the key is missing, so the error can tell the user exactly
/// what to export.
fn resolve_key(
    name: &str,
    section: Option<&RawProvider>,
    builtin: Option<&BuiltinProvider>,
    env: &EnvMap,
) -> (Option<String>, KeySource, String) {
    let recommended = section
        .and_then(|section| section.api_key_env.clone())
        .or_else(|| builtin.map(|builtin| builtin.key_env.to_owned()))
        .unwrap_or_else(|| format!("{}_API_KEY", env_prefix(name)));

    if let Some(key) = section.and_then(|section| section.api_key.clone()) {
        if !key.is_empty() {
            return (Some(key), KeySource::Config, recommended);
        }
    }

    let mut candidates: Vec<String> = Vec::new();
    if let Some(explicit) = section.and_then(|section| section.api_key_env.clone()) {
        candidates.push(explicit);
    } else if let Some(builtin) = builtin {
        candidates.extend(builtin.key_envs().into_iter().map(str::to_owned));
    } else {
        candidates.push(recommended.clone());
    }

    for candidate in candidates {
        if let Some(value) = env.get(&candidate).filter(|value| !value.is_empty()) {
            return (Some(value.clone()), KeySource::Env(candidate), recommended);
        }
    }
    (None, KeySource::Missing, recommended)
}

fn resolve_models(
    raw: &RawConfig,
    providers: &BTreeMap<String, ProviderProfile>,
) -> Result<BTreeMap<String, ModelProfile>, ConfigError> {
    let mut models = BTreeMap::new();

    for (id, provider) in BUILTIN_MODELS {
        let section = raw.models.get(*id);
        models.insert(
            (*id).to_owned(),
            ModelProfile {
                id: (*id).to_owned(),
                // A built-in id may still be re-pointed at a custom provider
                // (a proxy, say); absent an override it keeps its vendor.
                provider: section
                    .and_then(|section| section.provider.clone())
                    .unwrap_or_else(|| (*provider).to_owned()),
                params: params_of(section),
            },
        );
    }

    for (id, section) in &raw.models {
        if models.contains_key(id) {
            continue;
        }
        let provider = section
            .provider
            .clone()
            .ok_or_else(|| ConfigError::ModelWithoutProvider { model: id.clone() })?;
        models.insert(
            id.clone(),
            ModelProfile {
                id: id.clone(),
                provider,
                params: params_of(Some(section)),
            },
        );
    }

    for model in models.values() {
        if !providers.contains_key(&model.provider) {
            return Err(ConfigError::UnknownProvider {
                model: model.id.clone(),
                provider: model.provider.clone(),
            });
        }
    }

    Ok(models)
}

/// Resolve `[pricing.*]` into the display table (spec §17).
///
/// A price for a model that is not configured is a startup error: the key is a
/// model id, and a typo there would silently turn every cost into "unknown".
fn resolve_pricing(
    raw: &RawConfig,
    models: &BTreeMap<String, ModelProfile>,
) -> Result<PriceTable, ConfigError> {
    let mut table = PriceTable::new();
    for (model, price) in &raw.pricing {
        if !models.contains_key(model) {
            return Err(ConfigError::UnknownPricedModel {
                model: model.clone(),
            });
        }
        for (field, value) in [
            ("miss_input", price.miss_input),
            ("cached_input", price.cached_input),
            ("output", price.output),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ConfigError::InvalidPrice {
                    model: model.clone(),
                    field,
                });
            }
        }
        table.set(
            model,
            Pricing::new(price.miss_input, price.cached_input, price.output),
        );
    }
    Ok(table)
}

/// Resolve `[budget]` into the session's allowance (spec §17).
fn resolve_budget(raw: Option<&RawBudget>) -> Result<Budget, ConfigError> {
    let mut budget = Budget::new();
    let Some(raw) = raw else {
        return Ok(budget);
    };
    if let Some(tokens) = raw.session_tokens {
        budget = budget.with_limit(tokens);
    }
    if let Some(margin) = raw.estimate_margin {
        if !margin.is_finite() || margin <= 0.0 {
            return Err(ConfigError::InvalidBudget {
                reason: format!(
                    "estimate_margin must be a positive number of multiples, got {margin}"
                ),
            });
        }
        budget = budget.with_estimate_margin(margin);
    }
    Ok(budget)
}

/// Resolve `[routing]` into the two landing points' overrides (spec §17).
///
/// A routed model must be configured, for the same reason a price's key must be:
/// a typo would silently leave the participant on the discussion's model. Whether
/// it is a model the **dispatcher's provider** can serve is checked where the
/// executor is built (spec §16), because that depends on the model chosen at
/// assembly rather than on this file.
fn resolve_routing(
    raw: Option<&RawRouting>,
    models: &BTreeMap<String, ModelProfile>,
) -> Result<Routing, ConfigError> {
    let Some(raw) = raw else {
        return Ok(Routing::default());
    };
    for model in [&raw.synthesizer_model, &raw.executor_model]
        .into_iter()
        .flatten()
    {
        if !models.contains_key(model) {
            return Err(ConfigError::UnknownRoutedModel {
                model: model.clone(),
            });
        }
    }
    Ok(Routing {
        synthesizer_model: raw.synthesizer_model.clone(),
        executor_model: raw.executor_model.clone(),
    })
}

fn params_of(section: Option<&RawModel>) -> GenerationParams {
    match section {
        Some(section) => GenerationParams {
            temperature: section.temperature,
            top_p: section.top_p,
            max_output_tokens: section.max_output_tokens,
            reasoning_effort: section.reasoning_effort,
        },
        None => GenerationParams::default(),
    }
}

fn env_prefix(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Which vendor a key environment variable belongs to, if any. Makes the
/// cross-domain guard depend on the key's origin rather than the section name.
fn vendor_of_key_env(name: &str) -> Option<Vendor> {
    BUILTIN_PROVIDERS
        .iter()
        .find(|builtin| builtin.key_envs().contains(&name))
        .map(|builtin| builtin.vendor)
}

fn host_of(base_url: &str) -> Option<String> {
    url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
}

/// Configuration failures. Every one of these is a startup error: none of them
/// degrades silently.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config.toml: {source}")]
    Parse {
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("cannot read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("provider `{provider}` has no `base_url` and no built-in default")]
    ProviderWithoutBaseUrl { provider: String },
    #[error("provider `{provider}`: `{base_url}` is not a valid URL (a base_url needs a scheme and host)")]
    InvalidBaseUrl { provider: String, base_url: String },
    #[error(
        "model `{model}` references unknown provider `{provider}`; \
         declare it as [providers.{provider}] or point the model at an existing provider"
    )]
    UnknownProvider { model: String, provider: String },
    #[error("model `{model}` has no `provider` and is not a built-in model id")]
    ModelWithoutProvider { model: String },
    #[error("unknown model `{model}`; set `default_model` or pass --model to one of the configured models")]
    UnknownModel { model: String },
    #[error(
        "[pricing.{model}] names no configured model; prices are keyed by model id, so use a \
         built-in id or declare the model under [models.{model}] first"
    )]
    UnknownPricedModel { model: String },
    #[error(
        "[routing] names `{model}`, which is not a configured model; routing may only point at a \
         model id that exists in [models.*] or the built-in table"
    )]
    UnknownRoutedModel { model: String },
    #[error(
        "[pricing.{model}] {field}: a price is a non-negative number of USD per million tokens"
    )]
    InvalidPrice { model: String, field: &'static str },
    #[error("[budget] {reason}")]
    InvalidBudget { reason: String },
    #[error(
        "provider `{provider}`: base_url host `{host}` does not match the source of the key \
         (`{key_env}` is a {vendor} key, and a {vendor} key expects {expected}). \
         Mixing a key with another vendor's base_url returns 401. Point `base_url` at {expected}, \
         or set `api_key` explicitly in config.toml if you really mean this pairing."
    )]
    CrossVendorKey {
        provider: String,
        host: String,
        key_env: String,
        vendor: &'static str,
        expected: String,
    },
}

/// Injected configuration values for one agent's turn loop.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Model identifier used for the provider call.
    pub model: String,
    /// Hard cap on provider calls in one turn.
    pub max_iterations: u32,
    /// Generation parameters, including the pinned reasoning tier.
    pub params: GenerationParams,
    /// Cap on one tool result, in estimated tokens. An oversized result is
    /// truncated before it enters the stream (spec §10).
    pub max_tool_result_tokens: u64,
    /// Cap on the repo map, in estimated tokens (spec §9). A fixed budget, never
    /// a model-supplied argument; [`MAX_REPO_MAP_TOKENS`] is the ceiling.
    pub repo_map_tokens: u64,
    /// Turn cap for an executor this agent dispatches (spec §16). Independent of
    /// [`SessionConfig::max_iterations`] — "a runaway executor must not eat the
    /// session's turns" is the requirement, not an optimization — while its token
    /// spend still counts toward the session total.
    pub executor_max_iterations: u32,
    /// Model an executor answers with (spec §16, §17). `None` inherits the
    /// dispatcher's model, which is the default and the v1 behaviour; the override
    /// is the mechanism §17 leaves in place for routing work to a cheaper model
    /// once there is data to route on.
    ///
    /// The **client** is not overridable here: an executor answers on its
    /// dispatcher's provider, so this names a model that client can serve (a model
    /// of the same provider profile). Swapping the provider for executors is an
    /// assembly decision, not a per-session value.
    ///
    /// One of the two **landing points** a cheaper model may be routed to; read it
    /// through [`SessionConfig::model_for`], which is the routing rule's one home.
    pub executor_model: Option<String>,
    /// How many executors one batch may run at once (spec §16).
    pub max_parallel_executors: usize,
    /// Model the synthesizer answers with (spec §17). `None` inherits the
    /// discussion's model, which is the default and the v1 behaviour.
    ///
    /// The other landing point. A **debater is never routed** — there is no
    /// field for it and no call site that would read one.
    pub synthesizer_model: Option<String>,
    /// What a million tokens cost per model, for **display** (spec §17). The
    /// gate reads tokens; this table never decides anything.
    pub pricing: PriceTable,
    /// The session's cumulative token allowance (spec §17). Every participant on
    /// one stream shares it, which `assemble_discussion` enforces.
    pub budget: Budget,
}

impl SessionConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            params: GenerationParams::default(),
            max_tool_result_tokens: DEFAULT_MAX_TOOL_RESULT_TOKENS,
            repo_map_tokens: DEFAULT_REPO_MAP_TOKENS,
            executor_max_iterations: DEFAULT_EXECUTOR_MAX_ITERATIONS,
            executor_model: None,
            max_parallel_executors: DEFAULT_MAX_PARALLEL_EXECUTORS,
            synthesizer_model: None,
            pricing: PriceTable::new(),
            budget: Budget::new(),
        }
    }

    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Override the turn cap an executor runs under (spec §16).
    pub fn with_executor_max_iterations(mut self, executor_max_iterations: u32) -> Self {
        self.executor_max_iterations = executor_max_iterations;
        self
    }

    /// Override how many executors one batch may run at once (spec §16).
    pub fn with_max_parallel_executors(mut self, max_parallel_executors: usize) -> Self {
        self.max_parallel_executors = max_parallel_executors.max(1);
        self
    }

    /// Route this agent's executors to another model (spec §16, §17).
    pub fn with_executor_model(mut self, executor_model: impl Into<String>) -> Self {
        self.executor_model = Some(executor_model.into());
        self
    }

    /// Override the per-result truncation cap (spec §10).
    pub fn with_max_tool_result_tokens(mut self, max_tool_result_tokens: u64) -> Self {
        self.max_tool_result_tokens = max_tool_result_tokens;
        self
    }

    /// Set the repo-map budget, clamped to [`MAX_REPO_MAP_TOKENS`] (spec §9): no
    /// configuration can make one `repo_map` call unbounded.
    pub fn with_repo_map_tokens(mut self, repo_map_tokens: u64) -> Self {
        self.repo_map_tokens = repo_map_tokens.min(MAX_REPO_MAP_TOKENS);
        self
    }

    pub fn with_params(mut self, params: GenerationParams) -> Self {
        self.params = params;
        self
    }

    /// Pin the reasoning tier for the whole session (spec §4).
    pub fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.params.reasoning_effort = Some(effort);
        self
    }

    /// Route the synthesizer's one closing call to another model (spec §17).
    pub fn with_synthesizer_model(mut self, synthesizer_model: impl Into<String>) -> Self {
        self.synthesizer_model = Some(synthesizer_model.into());
        self
    }

    /// Carry the price table this session displays costs with (spec §17).
    pub fn with_pricing(mut self, pricing: PriceTable) -> Self {
        self.pricing = pricing;
        self
    }

    /// Give this session its cumulative token allowance (spec §17).
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Cap the session's cumulative tokens, keeping the default tolerance.
    pub fn with_session_token_limit(mut self, tokens: u64) -> Self {
        self.budget = self.budget.with_limit(tokens);
        self
    }

    /// Set the pre-flight tolerance, as a multiple of what is left (spec §17).
    pub fn with_estimate_margin(mut self, margin: f64) -> Self {
        self.budget = self.budget.with_estimate_margin(margin);
        self
    }

    /// The model `point` answers with: the configured override, else the model
    /// this config already carries (spec §17).
    ///
    /// This is the system's **only** routing rule, and it is called from exactly
    /// the two landing points — the executor port and the synthesizer's session.
    /// A debater is never routed: it answers with [`SessionConfig::model`], which
    /// no routing call can move.
    pub fn model_for(&self, point: LandingPoint) -> &str {
        let override_model = match point {
            LandingPoint::Synthesizer => self.synthesizer_model.as_deref(),
            LandingPoint::Executor => self.executor_model.as_deref(),
        };
        override_model.unwrap_or(&self.model)
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            params: GenerationParams::default(),
            max_tool_result_tokens: DEFAULT_MAX_TOOL_RESULT_TOKENS,
            repo_map_tokens: DEFAULT_REPO_MAP_TOKENS,
            executor_max_iterations: DEFAULT_EXECUTOR_MAX_ITERATIONS,
            executor_model: None,
            max_parallel_executors: DEFAULT_MAX_PARALLEL_EXECUTORS,
            synthesizer_model: None,
            pricing: PriceTable::new(),
            budget: Budget::new(),
        }
    }
}
