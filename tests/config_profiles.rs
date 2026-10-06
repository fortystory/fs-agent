//! 配置解析：两级、按字段的优先级，
//! 以及跨厂商的结构性护栏。
//!
//! 这里一切都走公开的 `config` API 并显式传一份环境映射，
//! 所以没有哪个测试会改进程的环境变量，
//! 也没有哪个测试碰网络。

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use heng::config::{
    self, default_path, resolve, EnvMap, FileViewer, KeySource, ReasoningEffort,
    SandboxAvailability, SandboxMode, Vendor, DEFAULT_FILE_VIEWER_WIDTH, DEFAULT_MODEL,
};
use heng::events::Decision;
use heng::permissions::Mode;
use heng::render::wording::NumberStyle;

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

    let kimi = config.provider("kimi").expect("内建的 kimi profile");
    assert_eq!(kimi.base_url, "https://api.moonshot.cn/v1");
    assert_eq!(kimi.key_source, KeySource::Missing);
    assert_eq!(kimi.api_key, None);

    let deepseek = config
        .provider("deepseek")
        .expect("内建的 deepseek profile");
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
        assert!(config.model(id).is_some(), "内建模型 {id}");
    }
}

#[test]
fn the_kimi_coding_plan_is_its_own_builtin_provider() {
    // Kimi Code 与 Kimi 开放平台是两个系统：base_url 不同、
    // 密钥变量不同、厂商相同。
    let config = resolve(None, &env(&[])).unwrap();
    let coding = config.provider("kimi-code").expect("内建的 kimi-code");
    assert_eq!(coding.base_url, "https://api.kimi.com/coding/v1");
    assert_eq!(coding.vendor, Some(Vendor::Kimi));
    assert_eq!(coding.key_env, "KIMI_API_KEY");
    assert_eq!(config.model("k3").unwrap().provider, "kimi-code");
    assert_eq!(config.model("k3-256k").unwrap().provider, "kimi-code");
}

#[test]
fn a_coding_plan_key_on_the_coding_host_passes_the_domain_guard() {
    // `api.kimi.com` 是 Kimi 的主机，所以那里由环境变量派生的
    // Kimi 密钥是放行的：两套 Kimi 系统按 base_url 分，不按厂商分。
    let config = resolve(None, &env(&[("KIMI_API_KEY", "sk-kimi-coding")])).unwrap();
    let coding = config.provider("kimi-code").unwrap();
    assert_eq!(coding.api_key.as_deref(), Some("sk-kimi-coding"));
    assert_eq!(coding.key_source, KeySource::Env("KIMI_API_KEY".to_owned()));
    // 开放平台那份 profile 仍然无密钥：变量之间不渗漏。
    assert_eq!(
        config.provider("kimi").unwrap().key_source,
        KeySource::Missing
    );
}

#[test]
fn config_toml_beats_environment_and_environment_beats_the_builtin_default() {
    // config.toml 胜过导出的 base_url。
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
    // 密钥仍然来自环境：优先级是按字段算的。
    assert_eq!(
        config.provider("kimi").unwrap().api_key.as_deref(),
        Some("sk-from-env")
    );
    assert_eq!(
        config.provider("kimi").unwrap().key_source,
        KeySource::Env("MOONSHOT_API_KEY".to_owned())
    );

    // 没有 config.toml 的值时，导出的 base_url 胜过内建的那份。
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

    // 加载器只读它拿到的那条路径，所以旁边放一个 `.env`
    // 改不了克隆下来的仓库的行为。
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

    // provider 是共享的那一份；这次覆盖没有给它开叉。
    // （三个内建 profile：kimi、kimi-code、deepseek。）
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
        .expect_err("跨厂商的配对必须被拒")
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
        .expect_err("决定允许哪些主机的是密钥的来源，不是节名")
        .to_string();
    assert!(error.contains("401"), "{error}");
    assert!(error.contains("MOONSHOT_API_KEY"), "{error}");
}

#[test]
fn an_explicit_key_env_binds_the_key_to_its_own_vendors_hosts() {
    // 这一节叫 `kimi`，但密钥来自 `DEEPSEEK_API_KEY`：
    // 它是 DeepSeek 的密钥，所以 Moonshot 的主机才是错的那个。
    let file = r#"
[providers.kimi]
api_key_env = "DEEPSEEK_API_KEY"
"#;
    let error = resolve(Some(file), &env(&[("DEEPSEEK_API_KEY", "sk-deepseek")]))
        .expect_err("密钥的来源必须胜过节名")
        .to_string();
    assert!(error.contains("DEEPSEEK_API_KEY"), "{error}");
    assert!(error.contains("api.deepseek.com"), "{error}");
    assert!(error.contains("401"), "{error}");
}

#[test]
fn the_reasoning_effort_wire_form_matches_its_serde_name() {
    // 线上的字符串与配置文件里的写法不许漂开。
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
        .expect_err("未知的 provider 必须被拒")
        .to_string();
    assert!(error.contains("nobody"), "{error}");
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn a_custom_model_without_a_provider_is_a_startup_error() {
    let error = resolve(Some("[models.mystery]\ntemperature = 0.1\n"), &env(&[]))
        .expect_err("自定义模型需要一个 provider")
        .to_string();
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn an_unknown_default_model_is_a_startup_error() {
    let error = resolve(Some("default_model = \"nope\"\n"), &env(&[]))
        .expect_err("默认模型必须存在")
        .to_string();
    assert!(error.contains("nope"), "{error}");
}

#[test]
fn a_typo_in_the_config_file_is_rejected_rather_than_ignored() {
    let error = resolve(
        Some("[providers.kimi]\nbase_uri = \"https://api.moonshot.cn/v1\"\n"),
        &env(&[]),
    )
    .expect_err("未知的键不许被静默忽略")
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
        std::path::PathBuf::from("/tmp/xdg/heng/config.toml")
    );
    assert_eq!(
        default_path(&env(&[("HOME", "/home/someone")])),
        std::path::PathBuf::from("/home/someone/.config/heng/config.toml")
    );
}

#[test]
fn the_session_store_root_prefers_xdg_data_home_then_home() {
    // 会话存储的根在 CLI 边界算出来、再注入库，所以这里
    // 是这套布局唯一的所在（spec §11）。
    assert_eq!(
        config::sessions_dir(&env(&[
            ("XDG_DATA_HOME", "/tmp/data"),
            ("HOME", "/home/someone")
        ])),
        Some(std::path::PathBuf::from("/tmp/data/heng/sessions"))
    );
    assert_eq!(
        config::sessions_dir(&env(&[("HOME", "/home/someone")])),
        Some(std::path::PathBuf::from(
            "/home/someone/.local/share/heng/sessions"
        ))
    );
    assert_eq!(config::sessions_dir(&env(&[])), None);
}

#[test]
fn each_builtin_profile_reads_its_own_key_variable() {
    // `KIMI_API_KEY` 是编码计划那一档的变量（Kimi 自己的第三方工具
    // 文档也用它），所以它不许渗进开放平台那份 profile。
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

    let usage = heng::events::Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cached_tokens: 900_000,
        miss_tokens: 100_000,
        reasoning_tokens: None,
    };
    // 0.1 M token 未命中按 0.28 + 0.9 M token 命中按 0.028 + 1 M token 输出按 0.42。
    let cost = config.pricing.cost("deepseek-flash", usage).unwrap();
    assert!((cost - (0.028 + 0.0252 + 0.42)).abs() < 1e-12, "{cost}");

    // 没有价目表的模型就没有价格，而价目表是按模型分的。
    assert!(config.pricing.cost("kimi-k3", usage).is_none());
    assert!(config.pricing.pricing("deepseek-flash").is_some());
}

#[test]
fn a_budget_section_caps_the_session_and_takes_the_tolerant_default_margin() {
    let default = resolve(None, &env(&[])).unwrap();
    assert_eq!(default.budget.limit, None, "v1 出厂就是不设上限的");
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
    .expect_err("价格以模型 id 为键")
    .to_string();
    assert!(error.contains("mystery"), "{error}");
}

#[test]
fn a_missing_or_nonsensical_price_is_rejected_rather_than_defaulted() {
    // 三档价格都是必需的：缺一档就会悄悄把一整类
    // token 定成零价。
    let missing = resolve(
        Some("[pricing.deepseek-flash]\nmiss_input = 1.0\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("cached_input 是必需的")
    .to_string();
    assert!(missing.contains("cached_input"), "{missing}");

    let negative = resolve(
        Some("[pricing.deepseek-flash]\nmiss_input = -1.0\ncached_input = 0.1\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("负的价格不是价格")
    .to_string();
    assert!(negative.contains("miss_input"), "{negative}");

    let typo = resolve(
        Some("[pricing.deepseek-flash]\nmiss = 1.0\ncached_input = 0.1\noutput = 2.0\n"),
        &env(&[]),
    )
    .expect_err("未知的键不许被静默忽略")
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
        .expect_err("容差必须是正的倍数")
        .to_string();
        assert!(error.contains("estimate_margin"), "{margin}: {error}");
    }
}

#[test]
fn routing_is_configuration_and_reaches_only_the_two_landing_points() {
    let unrouted = resolve(None, &env(&[])).unwrap();
    assert!(unrouted.routing.is_empty(), "v1 谁都不改派");

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

    // 改派通过唯一那个组装辅助函数到达 agent 的值，
    // 而它一点都不挪动讨论者自己的模型。
    let config = routed.session_config("kimi-k3").unwrap();
    assert_eq!(config.model, "kimi-k3");
    assert_eq!(
        config.model_for(heng::config::LandingPoint::Synthesizer),
        "deepseek-flash"
    );
    assert_eq!(
        config.model_for(heng::config::LandingPoint::Executor),
        "deepseek-flash"
    );
    // 没配改派的会话，一切都用自己的模型作答。
    let plain = unrouted.session_config("kimi-k3").unwrap();
    assert_eq!(
        plain.model_for(heng::config::LandingPoint::Synthesizer),
        "kimi-k3"
    );
}

#[test]
fn routing_to_an_unconfigured_model_is_a_startup_error() {
    // 这里打错一个字会悄悄把某个参与者留在讨论的模型上，
    // 而这类静默降级正是本仓库拒绝的。
    let error = resolve(
        Some("[routing]\nsynthesizer_model = \"mystery\"\n"),
        &env(&[]),
    )
    .expect_err("改派到的模型必须是配置好的")
    .to_string();
    assert!(error.contains("mystery"), "{error}");
}

// --- 回合上限（spec §3，用户故事 11） ------------------------------------------------

#[test]
fn the_turn_table_sets_the_turn_cap() {
    let configured = resolve(Some("[turn]\nmax_iterations = 1000\n"), &env(&[])).unwrap();

    // 从「config.toml 里的一张表」到「回合循环读的那个值」这条接线
    // 走在配置变成注入值的唯一那一处
    // （`Config::session_config`），于是没有哪条组装路径得记住它。
    let config = configured.session_config("kimi-k3").unwrap();
    assert_eq!(config.max_iterations, 1000);
}

#[test]
fn the_turn_table_sets_the_executors_own_cap() {
    // 执行者的上限与派发者的配在一起，而不是从它推出来
    // （spec §16）。
    let configured = resolve(Some("[turn]\nexecutor_max_iterations = 500\n"), &env(&[])).unwrap();
    let config = configured.session_config("kimi-k3").unwrap();
    assert_eq!(config.executor_max_iterations, 500);
}

#[test]
fn a_configuration_that_says_nothing_about_turns_keeps_the_spec_defaults() {
    // 故事 11 的「单 agent 默认 100 回合」与 §16 的 25，照 spec 与 README
    // 的说法写出来。让上限可配不许变成一次静默的抬高，所以一份
    // 从不提 `[turn]` 的配置花的还是原来那么多。
    let plain = resolve(None, &env(&[]))
        .unwrap()
        .session_config("kimi-k3")
        .unwrap();
    assert_eq!(plain.max_iterations, 100);
    assert_eq!(plain.executor_max_iterations, 25);
}

// --- 讨论池（spec §15） --------------------------------------------------------

/// 池子作为 `(name, model)` 对，测试想断言的就是这个样子。
fn pool(config: &config::Config) -> Vec<(String, String)> {
    config
        .discussion
        .as_ref()
        .expect("配了 [discussion]")
        .debaters
        .iter()
        .map(|debater| (debater.name.clone(), debater.model.clone()))
        .collect()
}

#[test]
fn a_discussion_pool_resolves_names_models_and_a_round_cap() {
    // 简写：模型 id 就是名字。
    let shorthand = resolve(
        Some("[discussion]\ndebaters = [\"kimi-k3\", \"deepseek-v4-pro\"]\nmax_rounds = 1\n"),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(
        pool(&shorthand),
        vec![
            ("kimi-k3".to_owned(), "kimi-k3".to_owned()),
            ("deepseek-v4-pro".to_owned(), "deepseek-v4-pro".to_owned()),
        ]
    );
    assert_eq!(shorthand.discussion.unwrap().max_rounds, Some(1));

    // 起了名字的人物，而且多于两个：讨论从中抽取的那个池子。
    let named = resolve(
        Some(
            "[discussion]\n\
             [[discussion.debaters]]\nname = \"保守\"\nmodel = \"kimi-k3\"\n\
             [[discussion.debaters]]\nname = \"激进\"\nmodel = \"deepseek-v4-pro\"\n\
             [[discussion.debaters]]\nname = \"审查\"\nmodel = \"deepseek-flash\"\n",
        ),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(
        pool(&named),
        vec![
            ("保守".to_owned(), "kimi-k3".to_owned()),
            ("激进".to_owned(), "deepseek-v4-pro".to_owned()),
            ("审查".to_owned(), "deepseek-flash".to_owned()),
        ]
    );
    assert_eq!(named.discussion.unwrap().max_rounds, None);
}

#[test]
fn a_configuration_without_a_discussion_table_has_no_pool() {
    // 缺席意味着「这个文件是给单 agent 会话用的」，而那正是
    // 子命令会说出口的话，而不是凭空造两个讨论者来问。
    let config = resolve(None, &env(&[])).unwrap();
    assert_eq!(config.discussion, None);
}

#[test]
fn a_pool_that_cannot_serve_a_discussion_is_a_startup_error() {
    let one = resolve(Some("[discussion]\ndebaters = [\"kimi-k3\"]\n"), &env(&[]))
        .expect_err("一个成员不成其为池子")
        .to_string();
    assert!(one.contains("至少要两个"), "{one}");

    // 空表对谁参与讨论什么都没说。
    let missing = resolve(Some("[discussion]\nmax_rounds = 2\n"), &env(&[]))
        .expect_err("池子需要讨论者")
        .to_string();
    assert!(missing.contains("debaters"), "{missing}");

    let unknown = resolve(
        Some("[discussion]\ndebaters = [\"kimi-k3\", \"mystery\"]\n"),
        &env(&[]),
    )
    .expect_err("讨论者必须是配置好的模型")
    .to_string();
    assert!(unknown.contains("mystery"), "{unknown}");

    let zero_rounds = resolve(
        Some("[discussion]\ndebaters = [\"kimi-k3\", \"deepseek-v4-pro\"]\nmax_rounds = 0\n"),
        &env(&[]),
    )
    .expect_err("一场讨论得有轮次")
    .to_string();
    assert!(zero_rounds.contains("max_rounds"), "{zero_rounds}");
}

#[test]
fn one_model_twice_needs_two_names() {
    // 名字是参与者在流上的身份，而每一次投影都是它的
    // 函数 —— 所以一个模型 id 不能是两个身份。这条报错说的是该写什么，
    // 而不是凭空造一个后缀。
    let error = resolve(
        Some("[discussion]\ndebaters = [\"kimi-k3\", \"kimi-k3\"]\n"),
        &env(&[]),
    )
    .expect_err("同一个模型的两个讨论者需要名字")
    .to_string();
    assert!(error.contains("都叫 `kimi-k3`"), "{error}");
    assert!(error.contains("name = \"甲\""), "{error}");

    // 起了名字，同一个模型用两次就没问题 —— 而这两个是一家厂商，
    // 前台会明说，而不是拒绝。
    let config = resolve(
        Some(
            "[discussion]\n\
             [[discussion.debaters]]\nname = \"甲\"\nmodel = \"kimi-k3\"\n\
             [[discussion.debaters]]\nname = \"乙\"\nmodel = \"kimi-k3\"\n",
        ),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(
        pool(&config),
        vec![
            ("甲".to_owned(), "kimi-k3".to_owned()),
            ("乙".to_owned(), "kimi-k3".to_owned()),
        ]
    );
    assert!(config.debaters_share_a_vendor("kimi-k3", "kimi-k3"));
    assert!(
        config.debaters_share_a_vendor("kimi-k3", "k3"),
        "两家都是 Kimi"
    );
    assert!(!config.debaters_share_a_vendor("kimi-k3", "deepseek-v4-pro"));
}

#[test]
fn a_persona_can_carry_a_soul_and_it_is_capped() {
    let config = resolve(
        Some(
            "[discussion]\n\
             [[discussion.debaters]]\nname = \"张三\"\nmodel = \"kimi-k3\"\n\
             soul = \"法外狂徒，思路不受限制\"\n\
             [[discussion.debaters]]\nname = \"李四\"\nmodel = \"deepseek-v4-pro\"\n",
        ),
        &env(&[]),
    )
    .unwrap();
    let pool = config.discussion.unwrap().debaters;
    assert_eq!(pool[0].soul.as_deref(), Some("法外狂徒，思路不受限制"));
    assert_eq!(pool[1].soul, None, "灵魂是可选的");

    // 空灵魂什么都没说：写了这个字段又留空是错误，
    // 而不是静默的空操作。
    let empty = resolve(
        Some(
            "[discussion]\n\
             [[discussion.debaters]]\nname = \"张三\"\nmodel = \"kimi-k3\"\nsoul = \"  \"\n\
             [[discussion.debaters]]\nname = \"李四\"\nmodel = \"deepseek-v4-pro\"\n",
        ),
        &env(&[]),
    )
    .expect_err("空的灵魂被拒")
    .to_string();
    assert!(empty.contains("空的 `soul`"), "{empty}");

    // 灵魂每一轮都钉着，所以它有上限。
    let long = "长".repeat(config::MAX_DEBATER_SOUL + 1);
    let over = resolve(
        Some(&format!(
            "[discussion]\n\
             [[discussion.debaters]]\nname = \"张三\"\nmodel = \"kimi-k3\"\nsoul = \"{long}\"\n\
             [[discussion.debaters]]\nname = \"李四\"\nmodel = \"deepseek-v4-pro\"\n"
        )),
        &env(&[]),
    )
    .expect_err("过长的灵魂被拒")
    .to_string();
    assert!(over.contains("超过"), "{over}");
}

#[test]
fn a_name_that_cannot_be_an_identity_is_a_startup_error() {
    // 换行符在 TOML 基本字符串里根本写不出来；制表符可以，而它正是
    // 前缀会断开的那类空白。
    for (name, why) in [("两 个", "spaces"), ("", "empty"), ("一\t二", "tab")] {
        let error = resolve(
            Some(&format!(
                "[discussion]\n\
                 [[discussion.debaters]]\nname = \"{name}\"\nmodel = \"kimi-k3\"\n\
                 [[discussion.debaters]]\nname = \"另一个\"\nmodel = \"deepseek-v4-pro\"\n"
            )),
            &env(&[]),
        )
        .expect_err(why)
        .to_string();
        assert!(error.contains("不断开的词"), "{why}: {error}");
    }

    let long = "长".repeat(config::MAX_DEBATER_NAME + 1);
    let error = resolve(
        Some(&format!(
            "[discussion]\n\
             [[discussion.debaters]]\nname = \"{long}\"\nmodel = \"kimi-k3\"\n\
             [[discussion.debaters]]\nname = \"另一个\"\nmodel = \"deepseek-v4-pro\"\n"
        )),
        &env(&[]),
    )
    .expect_err("过长的名字被拒")
    .to_string();
    assert!(error.contains("超过"), "{error}");
}

// --- 权限模式（`.scratch/todo-and-modes` 的票 01） --------------------------------

#[test]
fn the_permissions_table_selects_the_mode_a_session_starts_in() {
    for (written, expected) in [
        ("readonly", Mode::Readonly),
        ("ask", Mode::Ask),
        ("workspace", Mode::Workspace),
        ("auto", Mode::Auto),
    ] {
        let config = resolve(
            Some(&format!("[permissions]\nmode = \"{written}\"\n")),
            &env(&[]),
        )
        .unwrap_or_else(|error| panic!("`{written}` 是一档模式：{error}"));
        assert_eq!(config.mode, expected);
    }
}

#[test]
fn the_permissions_table_has_an_outside_read_knob_that_defaults_to_deny() {
    // 缺省不动摇：那条地板保住的正是 provider key 所在的那条路，
    // **写下来才算放弃**（`.scratch/workspace-mode/spec.md` §2）。
    assert_eq!(
        resolve(None, &env(&[])).unwrap().outside_read,
        Decision::Deny
    );
    assert_eq!(
        resolve(Some("[permissions]\n"), &env(&[]))
            .unwrap()
            .outside_read,
        Decision::Deny,
        "写了 [permissions] 却没写这一项时，缺省仍是 deny"
    );
    for (written, expected) in [
        ("deny", Decision::Deny),
        ("ask", Decision::Ask),
        ("allow", Decision::Allow),
    ] {
        let config = resolve(
            Some(&format!("[permissions]\noutside_read = \"{written}\"\n")),
            &env(&[]),
        )
        .unwrap_or_else(|error| panic!("`{written}` 是这条旋钮的合法取值：{error}"));
        assert_eq!(config.outside_read, expected);
    }
}

#[test]
fn an_unknown_outside_read_is_a_startup_error() {
    // 写错的人以为自己放开了区外读、实际仍然拒着，比反过来更糟 —— 他会去别处找原因。
    let error = resolve(Some("[permissions]\noutside_read = \"yes\"\n"), &env(&[]))
        .expect_err("这不是三个值之一")
        .to_string();
    for word in ["yes", "deny", "ask", "allow"] {
        assert!(error.contains(word), "这句里缺了 `{word}`：{error}");
    }
    assert!(!error.contains("  "), "一句话，里面没有连续空格：{error:?}");
}

#[test]
fn a_configuration_that_says_nothing_about_permissions_asks() {
    // 三个组装点过去硬编的那一档交互式默认，如今成了取代它的
    // 那张表所记下的默认。
    assert_eq!(resolve(None, &env(&[])).unwrap().mode, Mode::Ask);
    assert_eq!(
        resolve(Some("[budget]\nsession_tokens = 1000\n"), &env(&[]))
            .unwrap()
            .mode,
        Mode::Ask
    );
}

#[test]
fn an_unknown_mode_is_a_startup_error_that_names_the_four() {
    // `plan` 是这次改动之前写下的配置会持有的那个值，所以
    // 该说清「改成写什么」的正是它。
    let error = resolve(Some("[permissions]\nmode = \"plan\"\n"), &env(&[]))
        .expect_err("plan 不再是某一档模式")
        .to_string();
    for word in ["plan", "readonly", "ask", "workspace", "auto"] {
        assert!(error.contains(word), "这句里缺了 `{word}`：{error}");
    }
    // 启动路径原样打出这句话，所以丢掉一个行续反斜杠
    // 就会在句子中间塞进一串空格。
    assert!(!error.contains("  "), "一句话，里面没有连续空格：{error:?}");
}

#[test]
fn the_sandbox_section_defaults_to_bwrap_with_the_tool_caches_writable() {
    let config = resolve(None, &env(&[("HOME", "/home/ada"), ("PATH", "/usr/bin")])).unwrap();

    assert_eq!(config.sandbox.mode, SandboxMode::Bwrap);
    assert_eq!(
        config.sandbox.writable_roots,
        vec![
            PathBuf::from("/home/ada/.cargo"),
            PathBuf::from("/home/ada/.rustup"),
            PathBuf::from("/home/ada/.cache"),
        ],
        "缺省可写根：没有它们 `cargo build` 会失败。这是可用性决定，不是安全决定"
    );
    assert_eq!(
        config.sandbox.masks,
        vec![
            PathBuf::from("/home/ada/.config/heng"),
            PathBuf::from("/home/ada/.ssh"),
        ],
        "遮罩目录写死、不给旋钮"
    );
    assert_eq!(config.sandbox.availability, SandboxAvailability::Untested);
    assert_eq!(
        config.sandbox.search_path.as_deref(),
        Some(OsStr::new("/usr/bin"))
    );
}

#[test]
fn the_sandbox_section_takes_off_and_its_own_writable_roots() {
    let text = "[sandbox]\nmode = \"off\"\nwritable_roots = [\"~/work\", \"/srv/cache\"]\n";
    let config = resolve(
        Some(text),
        &env(&[("HOME", "/home/ada"), ("PATH", "/usr/bin")]),
    )
    .unwrap();

    assert_eq!(config.sandbox.mode, SandboxMode::Off);
    assert_eq!(
        config.sandbox.writable_roots,
        vec![PathBuf::from("/home/ada/work"), PathBuf::from("/srv/cache")]
    );
}

#[test]
fn an_unknown_sandbox_mode_is_a_startup_error() {
    let text = "[sandbox]\nmode = \"seatbelt\"\n";
    let error = resolve(Some(text), &env(&[])).unwrap_err().to_string();

    assert!(error.contains("seatbelt"), "{error}");
    assert!(error.contains("bwrap"), "{error}");
    assert!(error.contains("off"), "{error}");
}

// --- `[goals]`：目标循环的两个阈值（`.scratch/goal-loop/spec.md` §6） -------

#[test]
fn the_goal_thresholds_default_to_fifty_and_eighty() {
    let config = resolve(None, &env(&[])).unwrap();

    assert_eq!(config.goals.remind_at, 50);
    assert_eq!(config.goals.compact_at, 80);
}

#[test]
fn the_goal_thresholds_come_from_the_goals_table() {
    let config = resolve(
        Some("[goals]\nremind_at = 30\ncompact_at = 90\n"),
        &env(&[]),
    )
    .unwrap();

    assert_eq!(config.goals.remind_at, 30);
    assert_eq!(config.goals.compact_at, 90);

    // 只写一个，另一个留在缺省上。
    let config = resolve(Some("[goals]\nremind_at = 30\n"), &env(&[])).unwrap();
    assert_eq!(config.goals.remind_at, 30);
    assert_eq!(config.goals.compact_at, 80);
}

#[test]
fn a_threshold_that_could_never_fire_is_a_startup_error() {
    // 提醒必须在翻页之前：反过来的话它永远轮不到，压缩先来了。
    let error = resolve(
        Some("[goals]\nremind_at = 80\ncompact_at = 80\n"),
        &env(&[]),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("remind_at"), "{error}");
    assert!(error.contains("compact_at"), "{error}");

    for value in ["0", "101"] {
        let text = format!("[goals]\ncompact_at = {value}\n");
        let error = resolve(Some(&text), &env(&[])).unwrap_err().to_string();
        assert!(error.contains("compact_at"), "{value}: {error}");
    }

    // 一个不认识的字段仍然按 `deny_unknown_fields` 拒掉。
    let error = resolve(Some("[goals]\nremind = 30\n"), &env(&[]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("remind"), "{error}");
}

#[test]
fn the_goal_stop_knobs_come_from_the_same_table() {
    let config = resolve(None, &env(&[])).unwrap();
    assert_eq!(config.goals.no_progress_rollovers, 3);
    assert_eq!(config.goals.provider_retries, 2);

    let config = resolve(
        Some("[goals]\nno_progress_rollovers = 5\nprovider_retries = 0\n"),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(config.goals.no_progress_rollovers, 5);
    assert_eq!(config.goals.provider_retries, 0, "0 次就是不重试");

    // 0 次翻页会让循环在第一次翻页之前就认输。
    let error = resolve(Some("[goals]\nno_progress_rollovers = 0\n"), &env(&[]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no_progress_rollovers"), "{error}");

    // 重试是有代价的，所以有一个上限。
    let error = resolve(Some("[goals]\nprovider_retries = 99\n"), &env(&[]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("provider_retries"), "{error}");
}

// --- `[ui]`：数字的书写制式（`.scratch/usage-stats-format/spec.md` §2） ------

#[test]
fn the_number_style_defaults_to_chinese_units() {
    let config = resolve(None, &env(&[])).unwrap();
    assert_eq!(config.number_style, NumberStyle::Cn);
}

#[test]
fn the_number_style_comes_from_the_ui_table() {
    let config = resolve(Some("[ui]\nnumber_style = \"si\"\n"), &env(&[])).unwrap();
    assert_eq!(config.number_style, NumberStyle::Si);

    // 写回缺省值也是合法的、也仍然是那一档。
    let config = resolve(Some("[ui]\nnumber_style = \"cn\"\n"), &env(&[])).unwrap();
    assert_eq!(config.number_style, NumberStyle::Cn);
}

#[test]
fn an_unknown_number_style_is_a_startup_error() {
    // 大小写不合与自造的词都不算数：写错了的人以为自己配好了，屏幕上却是另一套读法。
    for value in ["CN", "wan"] {
        let text = format!("[ui]\nnumber_style = \"{value}\"\n");
        let error = resolve(Some(&text), &env(&[])).unwrap_err().to_string();
        assert!(error.contains("number_style"), "{value}: {error}");
        assert!(error.contains("cn"), "{value}: {error}");
        assert!(error.contains("si"), "{value}: {error}");
    }

    // 一个不认识的键仍然按 `deny_unknown_fields` 拒掉。
    let error = resolve(
        Some("[ui]\nnumber_style = \"cn\"\nnumber_ways = 1\n"),
        &env(&[]),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("number_ways"), "{error}");
}

// --- `[web]`：两个联网工具的部署设置（`.scratch/web-search-tool/spec.md` §9） ---

#[test]
fn the_web_tools_are_off_by_default_and_their_knobs_have_defaults() {
    let config = resolve(None, &env(&[])).unwrap();

    assert!(!config.web.enabled, "缺省关：工具不进表");
    assert_eq!(config.web.search_provider, "deepseek");
    assert_eq!(config.web.fetch_provider, "http");
    assert_eq!(
        config.web.search_base_url,
        "https://api.deepseek.com/anthropic"
    );
    assert_eq!(config.web.search_max_results, 8);
    assert_eq!(config.web.search_max_queries, 4);
    assert_eq!(config.web.fetch_max_chars, 100_000);
    assert_eq!(config.web.fetch_timeout_ms, 30_000);
    assert!(
        !config.web.trust_proxy_dns,
        "缺省严格：DNS 不被代理接管，解析结果照样整体校验"
    );
}

#[test]
fn the_web_table_comes_from_the_config_file() {
    let config = resolve(
        Some(
            "[web]\nenabled = true\nsearch_max_results = 3\nsearch_max_queries = 2\n\
             trust_proxy_dns = true\n\
             search_base_url = \"https://example.com/anthropic/\"\n",
        ),
        &env(&[]),
    )
    .unwrap();

    assert!(config.web.enabled);
    assert_eq!(config.web.search_max_results, 3);
    assert_eq!(config.web.search_max_queries, 2);
    assert_eq!(
        config.web.search_base_url, "https://example.com/anthropic",
        "尾部斜杠收干净，好与 `/v1/messages` 拼得对"
    );
    assert_eq!(config.web.search_provider, "deepseek", "没写的留在缺省上");
    assert!(config.web.trust_proxy_dns, "这台机器的 DNS 由代理接管");
}

#[test]
fn a_web_knob_of_zero_is_clamped_rather_than_believed() {
    // 写 0 的人多半想要「不限制」，而实际会得到「什么都搜不到」；当场纠正比事后排查便宜。
    let config = resolve(Some("[web]\nsearch_max_results = 0\n"), &env(&[])).unwrap();
    assert_eq!(config.web.search_max_results, 1);

    // 一个不认识的键仍然按 `deny_unknown_fields` 拒掉。
    let error = resolve(Some("[web]\nserch_provider = \"deepseek\"\n"), &env(&[]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("serch_provider"), "{error}");
}

#[test]
fn the_file_viewer_defaults_to_the_builtin_preview() {
    // 没配就一个字都不变：内置预览是这一档的缺省（`.scratch/nvim-file-viewer/spec.md` §2）。
    let config = resolve(None, &env(&[])).unwrap();

    assert_eq!(config.file_viewer.kind, FileViewer::Builtin);
    assert_eq!(config.file_viewer.width, DEFAULT_FILE_VIEWER_WIDTH);
}

#[test]
fn the_file_viewer_comes_from_the_ui_table() {
    let config = resolve(
        Some("[ui]\nfile_viewer = \"nvim\"\nfile_viewer_width = 100\n"),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(config.file_viewer.kind, FileViewer::Nvim);
    assert_eq!(config.file_viewer.width, 100);

    // 写回缺省值也合法：宽度不写就跟着缺省走，与查看器那一档分开。
    let config = resolve(Some("[ui]\nfile_viewer = \"builtin\"\n"), &env(&[])).unwrap();
    assert_eq!(config.file_viewer.kind, FileViewer::Builtin);
    assert_eq!(config.file_viewer.width, DEFAULT_FILE_VIEWER_WIDTH);
}

#[test]
fn an_unknown_file_viewer_is_a_startup_error() {
    // 大小写不合、以及「写着自己编辑器的名字」都拒掉：写了 `vim` 的人以为点开文件会进
    // 自己的编辑器，屏幕上却是内置预览 —— 那种「配了等于没配」只有报错说得清。
    for value in ["Nvim", "vim", "less"] {
        let text = format!("[ui]\nfile_viewer = \"{value}\"\n");
        let error = resolve(Some(&text), &env(&[])).unwrap_err().to_string();
        assert!(error.contains("file_viewer"), "{value}: {error}");
        assert!(error.contains("builtin"), "{value}: {error}");
        assert!(error.contains("nvim"), "{value}: {error}");
    }
}

#[test]
fn a_file_viewer_width_that_could_never_hold_nvim_is_a_startup_error() {
    for width in [0, 19] {
        let text = format!("[ui]\nfile_viewer = \"nvim\"\nfile_viewer_width = {width}\n");
        let error = resolve(Some(&text), &env(&[])).unwrap_err().to_string();
        assert!(error.contains("file_viewer_width"), "{width}: {error}");
    }

    // 下界那一格自己合法 —— 边界是「放得下一屏」。
    let config = resolve(
        Some("[ui]\nfile_viewer = \"nvim\"\nfile_viewer_width = 20\n"),
        &env(&[]),
    )
    .unwrap();
    assert_eq!(config.file_viewer.width, 20);
}
