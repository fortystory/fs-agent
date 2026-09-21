//! Configuration resolution: two levels, field-wise precedence, and the
//! structural cross-vendor guard.
//!
//! Everything here goes through the public `config` API with an explicit
//! environment map, so no test mutates the process environment and none of
//! them touches the network.

use std::fs;

use fs_agent::config::{
    self, default_path, resolve, EnvMap, KeySource, ReasoningEffort, Vendor, DEFAULT_MODEL,
};

fn env(pairs: &[(&str, &str)]) -> EnvMap {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn builtin_defaults_exist_with_no_config_file_and_no_environment() {
    let config = resolve(None, &env(&[])).unwrap();

    assert_eq!(config.default_model, DEFAULT_MODEL);
    assert_eq!(config.models.len(), 7);

    let kimi = config.provider("kimi").expect("built-in kimi profile");
    assert_eq!(kimi.base_url, "https://api.moonshot.cn/v1");
    assert_eq!(kimi.key_source, KeySource::Missing);
    assert_eq!(kimi.api_key, None);

    let deepseek = config
        .provider("deepseek")
        .expect("built-in deepseek profile");
    assert_eq!(deepseek.base_url, "https://api.deepseek.com");

    for id in [
        "kimi-k3",
        "k3",
        "k3-256k",
        "kimi-for-coding",
        "kimi-for-coding-highspeed",
        "deepseek-v4-pro",
        "deepseek-flash",
    ] {
        assert!(config.model(id).is_some(), "built-in model {id}");
    }
}

#[test]
fn the_kimi_coding_plan_is_its_own_builtin_provider() {
    // Kimi Code and the Kimi Open Platform are separate systems: separate
    // base_url, separate key variable, same vendor.
    let config = resolve(None, &env(&[])).unwrap();
    let coding = config.provider("kimi-code").expect("built-in kimi-code");
    assert_eq!(coding.base_url, "https://api.kimi.com/coding/v1");
    assert_eq!(coding.vendor, Some(Vendor::Kimi));
    assert_eq!(coding.key_env, "KIMI_API_KEY");
    assert_eq!(config.model("k3").unwrap().provider, "kimi-code");
    assert_eq!(config.model("k3-256k").unwrap().provider, "kimi-code");
}

#[test]
fn a_coding_plan_key_on_the_coding_host_passes_the_domain_guard() {
    // `api.kimi.com` is a Kimi host, so an environment-derived Kimi key there
    // is allowed: the two Kimi systems differ by base_url, not by vendor.
    let config = resolve(None, &env(&[("KIMI_API_KEY", "sk-kimi-coding")])).unwrap();
    let coding = config.provider("kimi-code").unwrap();
    assert_eq!(coding.api_key.as_deref(), Some("sk-kimi-coding"));
    assert_eq!(coding.key_source, KeySource::Env("KIMI_API_KEY".to_owned()));
    // The Open Platform profile stays keyless: the variables do not bleed.
    assert_eq!(
        config.provider("kimi").unwrap().key_source,
        KeySource::Missing
    );
}

#[test]
fn config_toml_beats_environment_and_environment_beats_the_builtin_default() {
    // config.toml wins over an exported base_url.
    let file = r#"
[providers.kimi]
base_url = "https://api.moonshot.cn/v9"
"#;
    let config = resolve(
        Some(file),
        &env(&[
            ("KIMI_BASE_URL", "https://api.moonshot.cn/env"),
            ("MOONSHOT_API_KEY", "sk-from-env"),
        ]),
    )
    .unwrap();
    assert_eq!(
        config.provider("kimi").unwrap().base_url,
        "https://api.moonshot.cn/v9"
    );
    // The key still comes from the environment: precedence is per field.
    assert_eq!(
        config.provider("kimi").unwrap().api_key.as_deref(),
        Some("sk-from-env")
    );
    assert_eq!(
        config.provider("kimi").unwrap().key_source,
        KeySource::Env("MOONSHOT_API_KEY".to_owned())
    );

    // With no config.toml value, the exported base_url beats the built-in one.
    let config = resolve(
        Some(""),
        &env(&[
            ("KIMI_BASE_URL", "https://api.moonshot.cn/env"),
            ("MOONSHOT_API_KEY", "sk-from-env"),
        ]),
    )
    .unwrap();
    assert_eq!(
        config.provider("kimi").unwrap().base_url,
        "https://api.moonshot.cn/env"
    );
}

#[test]
fn a_project_dot_env_is_never_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let dot_env = dir.path().join(".env");
    fs::write(&dot_env, "MOONSHOT_API_KEY=sk-from-dot-env\n").unwrap();
    let config_path = dir.path().join("config.toml");
    fs::write(&config_path, "").unwrap();

    // The loader reads exactly the path it is given, so a neighbouring `.env`
    // cannot change the behaviour of a cloned repository.
    let config = config::load(&config_path, &env(&[])).unwrap();
    let kimi = config.provider("kimi").unwrap();
    assert_eq!(kimi.api_key, None);
    assert_eq!(kimi.key_source, KeySource::Missing);
}

#[test]
fn a_model_section_references_a_provider_and_overrides_parameters() {
    let file = r#"
default_model = "deepseek-v4-pro"

[providers.deepseek]
api_key = "sk-deepseek"

[models.deepseek-v4-pro]
provider = "deepseek"
temperature = 0.2
top_p = 0.7
max_output_tokens = 4096
reasoning_effort = "low"
"#;
    let config = resolve(Some(file), &env(&[])).unwrap();
    assert_eq!(config.default_model, "deepseek-v4-pro");

    let model = config.model("deepseek-v4-pro").unwrap();
    assert_eq!(model.provider, "deepseek");
    assert_eq!(model.params.temperature, Some(0.2));
    assert_eq!(model.params.top_p, Some(0.7));
    assert_eq!(model.params.max_output_tokens, Some(4096));
    assert_eq!(model.params.reasoning_effort, Some(ReasoningEffort::Low));

    // The provider is shared; the override did not fork it. (Three built-in
    // profiles: kimi, kimi-code, deepseek.)
    assert_eq!(config.providers.len(), 3);
    assert_eq!(
        config
            .provider_for("deepseek-v4-pro")
            .unwrap()
            .api_key
            .as_deref(),
        Some("sk-deepseek")
    );
}

#[test]
fn a_custom_provider_is_allowed_to_pair_an_explicit_key_with_any_host() {
    let file = r#"
[providers.proxy]
base_url = "https://llm-proxy.internal/v1"
api_key = "sk-proxy"

[models.proxy-model]
provider = "proxy"
"#;
    let config = resolve(Some(file), &env(&[])).unwrap();
    let proxy = config.provider("proxy").unwrap();
    assert_eq!(proxy.vendor, None);
    assert_eq!(proxy.base_url, "https://llm-proxy.internal/v1");
    assert_eq!(proxy.key_source, KeySource::Config);
}

#[test]
fn an_environment_key_pointed_at_another_vendors_host_is_a_readable_error() {
    let file = r#"
[providers.kimi]
base_url = "https://api.deepseek.com"
"#;
    let error = resolve(Some(file), &env(&[("MOONSHOT_API_KEY", "sk-kimi")]))
        .expect_err("cross-vendor pairing must be rejected")
        .to_string();

    assert!(error.contains("api.deepseek.com"), "{error}");
    assert!(error.contains("MOONSHOT_API_KEY"), "{error}");
    assert!(error.contains("401"), "{error}");
    assert!(error.contains("api.moonshot.cn"), "{error}");
}

#[test]
fn a_builtin_model_id_can_be_repointed_at_a_custom_provider() {
    let file = r#"
[providers.proxy]
base_url = "https://proxy.internal/v1"
api_key = "sk-proxy"

[models.kimi-k3]
provider = "proxy"
temperature = 0.1
"#;
    let config = resolve(Some(file), &env(&[])).unwrap();
    let model = config.model("kimi-k3").unwrap();
    assert_eq!(model.provider, "proxy");
    assert_eq!(model.params.temperature, Some(0.1));
    assert_eq!(
        config.provider_for("kimi-k3").unwrap().base_url,
        "https://proxy.internal/v1"
    );
}

#[test]
fn a_vendor_key_env_is_domain_checked_whatever_the_provider_is_called() {
    let file = r#"
[providers.mirror]
base_url = "https://api.deepseek.com"
api_key_env = "MOONSHOT_API_KEY"
"#;
    let error = resolve(Some(file), &env(&[("MOONSHOT_API_KEY", "sk-kimi")]))
        .expect_err("the key's origin, not the section name, decides the allowed host")
        .to_string();
    assert!(error.contains("401"), "{error}");
    assert!(error.contains("MOONSHOT_API_KEY"), "{error}");
}

#[test]
fn an_explicit_key_env_binds_the_key_to_its_own_vendors_hosts() {
    // The section is named `kimi`, but the key comes from `DEEPSEEK_API_KEY`:
    // it is a DeepSeek key, so the Moonshot host is the wrong one.
    let file = r#"
[providers.kimi]
api_key_env = "DEEPSEEK_API_KEY"
"#;
    let error = resolve(Some(file), &env(&[("DEEPSEEK_API_KEY", "sk-deepseek")]))
        .expect_err("the key's origin must win over the section name")
        .to_string();
    assert!(error.contains("DEEPSEEK_API_KEY"), "{error}");
    assert!(error.contains("api.deepseek.com"), "{error}");
    assert!(error.contains("401"), "{error}");
}

#[test]
fn the_reasoning_effort_wire_form_matches_its_serde_name() {
    // The wire string and the config-file spelling must not drift apart.
    for effort in [
        ReasoningEffort::Low,
        ReasoningEffort::High,
        ReasoningEffort::Max,
    ] {
        assert_eq!(
            serde_json::to_value(effort).unwrap(),
            serde_json::json!(effort.as_str())
        );
    }
}

#[test]
fn a_missing_key_is_reported_as_missing_not_as_a_failure() {
    let config = resolve(None, &env(&[])).unwrap();
    assert_eq!(
        config.provider("deepseek").unwrap().key_source,
        KeySource::Missing
    );
}

#[test]
fn a_model_pointing_at_an_unknown_provider_is_a_startup_error() {
    let file = r#"
[models.mystery]
provider = "nobody"
"#;
    let error = resolve(Some(file), &env(&[]))
        .expect_err("unknown provider must be rejected")
        .to_string();
    assert!(error.contains("nobody"), "{error}");
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn a_custom_model_without_a_provider_is_a_startup_error() {
    let error = resolve(Some("[models.mystery]\ntemperature = 0.1\n"), &env(&[]))
        .expect_err("a custom model needs a provider")
        .to_string();
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn an_unknown_default_model_is_a_startup_error() {
    let error = resolve(Some("default_model = \"nope\"\n"), &env(&[]))
        .expect_err("the default model must exist")
        .to_string();
    assert!(error.contains("nope"), "{error}");
}

#[test]
fn a_typo_in_the_config_file_is_rejected_rather_than_ignored() {
    let error = resolve(
        Some("[providers.kimi]\nbase_uri = \"https://api.moonshot.cn/v1\"\n"),
        &env(&[]),
    )
    .expect_err("unknown keys must not be silently ignored")
    .to_string();
    assert!(error.contains("base_uri"), "{error}");
}

#[test]
fn the_default_path_prefers_xdg_config_home_then_home() {
    assert_eq!(
        default_path(&env(&[
            ("XDG_CONFIG_HOME", "/tmp/xdg"),
            ("HOME", "/home/someone")
        ])),
        std::path::PathBuf::from("/tmp/xdg/fs-agent/config.toml")
    );
    assert_eq!(
        default_path(&env(&[("HOME", "/home/someone")])),
        std::path::PathBuf::from("/home/someone/.config/fs-agent/config.toml")
    );
}

#[test]
fn the_session_store_root_prefers_xdg_data_home_then_home() {
    // The store's root is computed at the CLI boundary and injected into the
    // library, so this is the only place the layout lives (spec §11).
    assert_eq!(
        config::sessions_dir(&env(&[
            ("XDG_DATA_HOME", "/tmp/data"),
            ("HOME", "/home/someone")
        ])),
        Some(std::path::PathBuf::from("/tmp/data/fs-agent/sessions"))
    );
    assert_eq!(
        config::sessions_dir(&env(&[("HOME", "/home/someone")])),
        Some(std::path::PathBuf::from(
            "/home/someone/.local/share/fs-agent/sessions"
        ))
    );
    assert_eq!(config::sessions_dir(&env(&[])), None);
}

#[test]
fn each_builtin_profile_reads_its_own_key_variable() {
    // `KIMI_API_KEY` is the coding plan's variable (Kimi's own third-party-tool
    // docs use it), so it must not leak into the Open Platform profile.
    let coding = resolve(None, &env(&[("KIMI_API_KEY", "sk-kimi-coding")])).unwrap();
    assert_eq!(
        coding.provider("kimi-code").unwrap().key_source,
        KeySource::Env("KIMI_API_KEY".to_owned())
    );
    assert_eq!(
        coding.provider("kimi").unwrap().key_source,
        KeySource::Missing
    );

    let platform = resolve(None, &env(&[("MOONSHOT_API_KEY", "sk-platform")])).unwrap();
    assert_eq!(
        platform.provider("kimi").unwrap().key_source,
        KeySource::Env("MOONSHOT_API_KEY".to_owned())
    );
    assert_eq!(
        platform.provider("kimi-code").unwrap().key_source,
        KeySource::Missing
    );

    let alternate = resolve(None, &env(&[("KIMI_CODE_API_KEY", "sk-kimi-coding")])).unwrap();
    assert_eq!(
        alternate.provider("kimi-code").unwrap().key_source,
        KeySource::Env("KIMI_CODE_API_KEY".to_owned())
    );
}

#[test]
fn a_price_table_is_configuration_and_prices_a_hit_apart_from_a_miss() {
    let config = resolve(
        Some(
            "[pricing.deepseek-flash]\n\
             miss_input = 0.28\n\
             cached_input = 0.028\n\
             output = 0.42\n",
        ),
        &env(&[]),
    )
    .unwrap();

    let usage = fs_agent::events::Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cached_tokens: 900_000,
        miss_tokens: 100_000,
        reasoning_tokens: None,
    };
    // 0.1 Mtok miss at 0.28 + 0.9 Mtok hit at 0.028 + 1 Mtok out at 0.42.
    let cost = config.pricing.cost("deepseek-flash", usage).unwrap();
    assert!((cost - (0.028 + 0.0252 + 0.42)).abs() < 1e-12, "{cost}");

    // A model with no table has no price, and the tables are per model.
    assert!(config.pricing.cost("kimi-k3", usage).is_none());
    assert!(config.pricing.pricing("deepseek-flash").is_some());
}

#[test]
fn a_budget_section_caps_the_session_and_takes_the_tolerant_default_margin() {
    let default = resolve(None, &env(&[])).unwrap();
    assert_eq!(default.budget.limit, None, "v1 ships uncapped");
    assert!(default.budget.estimate_margin > 1.0);

    let capped = resolve(Some("[budget]\nsession_tokens = 250000\n"), &env(&[])).unwrap();
    assert_eq!(capped.budget.limit, Some(250_000));
    assert_eq!(capped.budget.remaining(50_000), Some(200_000));

    let tuned = resolve(Some("[budget]\nestimate_margin = 1.25\n"), &env(&[])).unwrap();
    assert_eq!(tuned.budget.estimate_margin, 1.25);
}

#[test]
fn a_price_for_an_unregistered_model_is_a_startup_error() {
    let error = resolve(
        Some("[pricing.mystery]\nmiss_input = 1.0\ncached_input = 0.1\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("a price is keyed by model id")
    .to_string();
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn a_missing_or_nonsensical_price_is_rejected_rather_than_defaulted() {
    // All three prices are required: a missing one would quietly price a whole
    // class of tokens at zero.
    let missing = resolve(
        Some("[pricing.deepseek-flash]\nmiss_input = 1.0\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("cached_input is required")
    .to_string();
    assert!(missing.contains("cached_input"), "{missing}");

    let negative = resolve(
        Some("[pricing.deepseek-flash]\nmiss_input = -1.0\ncached_input = 0.1\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("a negative price is not a price")
    .to_string();
    assert!(negative.contains("miss_input"), "{negative}");

    let typo = resolve(
        Some("[pricing.deepseek-flash]\nmiss = 1.0\ncached_input = 0.1\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("unknown keys must not be silently ignored")
    .to_string();
    assert!(typo.contains("miss"), "{typo}");
}

#[test]
fn a_nonsensical_estimate_margin_is_a_startup_error() {
    for margin in ["0.0", "-1.0", "nan"] {
        let error = resolve(
            Some(&format!("[budget]\nestimate_margin = {margin}\n")),
            &env(&[]),
        )
        .expect_err("the tolerance must be a positive multiple")
        .to_string();
        assert!(error.contains("estimate_margin"), "{margin}: {error}");
    }
}

#[test]
fn routing_is_configuration_and_reaches_only_the_two_landing_points() {
    let unrouted = resolve(None, &env(&[])).unwrap();
    assert!(unrouted.routing.is_empty(), "v1 routes nobody");

    let routed = resolve(
        Some(
            "[routing]\n\
             synthesizer_model = \"deepseek-flash\"\n\
             executor_model = \"deepseek-flash\"\n",
        ),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(
        routed.routing.synthesizer_model.as_deref(),
        Some("deepseek-flash")
    );

    // The routing reaches the agent's values through the one assembly helper,
    // and it moves the debater's own model not at all.
    let config = routed.session_config("kimi-k3").unwrap();
    assert_eq!(config.model, "kimi-k3");
    assert_eq!(
        config.model_for(fs_agent::config::LandingPoint::Synthesizer),
        "deepseek-flash"
    );
    assert_eq!(
        config.model_for(fs_agent::config::LandingPoint::Executor),
        "deepseek-flash"
    );
    // An unrouted session answers everything with its own model.
    let plain = unrouted.session_config("kimi-k3").unwrap();
    assert_eq!(
        plain.model_for(fs_agent::config::LandingPoint::Synthesizer),
        "kimi-k3"
    );
}

#[test]
fn routing_to_an_unconfigured_model_is_a_startup_error() {
    // A typo here would silently leave a participant on the discussion's model,
    // which is exactly the kind of quiet downgrade this repo refuses.
    let error = resolve(
        Some("[routing]\nsynthesizer_model = \"mystery\"\n"),
        &env(&[]),
    )
    .expect_err("a routed model must be configured")
    .to_string();
    assert!(error.contains("mystery"), "{error}");
}
