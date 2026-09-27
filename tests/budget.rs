//! 成本与会话的花钱上限（spec §17）。
//!
//! 三条接缝，用的都是 spec 已经点名的那几条：
//!
//! * 纯规则 —— 价目表、闸门、路由规则 —— 直接调用，
//!   没有 provider、也没有事件流；
//! * 每日账本走公开的存储，因为「从会话文件推出来」
//!   正是它的全部意义；
//! * 闸门本身通过库的组装接缝来练，摆在那些已经拥有这些 fixture 的
//!   文件里：轮次循环在
//!   `discussion.rs`，执行者端口在 `executor.rs`，回合循环在
//!   `e2e_single_turn.rs`。

use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use fs_agent::config::{LandingPoint, PriceTable, Pricing, SessionConfig};
use fs_agent::events::{Event, EventPayload, SessionId, SpeakerId, Usage, SCHEMA_VERSION};
use fs_agent::session::ledger;
use fs_agent::session::SessionStore;

fn usage(input: u64, output: u64, cached: u64, miss: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cached_tokens: cached,
        miss_tokens: miss,
        reasoning_tokens: None,
    }
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-12
}

#[test]
fn the_session_allowance_counts_input_and_output_and_never_the_cache_split_twice() {
    // `cached` 与 `miss` 是对 `input` 的一次切分，而两家厂商都已经把
    // 推理 token 算在 `output` 里了；把其中任何一个再叠上去，
    // 都会把闸门读到的那个数字吹大。
    assert_eq!(usage(100, 20, 80, 20).total_tokens(), 120);
}

#[test]
fn a_cache_hit_and_a_cache_miss_are_priced_apart() {
    // 单位是每百万 token 多少美元：一次未命中的价格是一次命中的十倍，
    // 输出又是未命中的十倍。0.1 个 Mtok 未命中 + 0.9 个 Mtok 命中
    // + 0.001 个 Mtok 输出 = 1.0 + 0.9 + 0.1。
    let pricing = Pricing::new(10.0, 1.0, 100.0);
    let mixed = usage(1_000_000, 1_000, 900_000, 100_000);
    assert!(close(pricing.cost(mixed), 2.0));

    let all_miss = usage(1_000_000, 0, 0, 1_000_000);
    let all_cached = usage(1_000_000, 0, 1_000_000, 0);
    assert!(
        pricing.cost(all_miss) > pricing.cost(all_cached),
        "把这个切分分别定价，必须体现在钱上"
    );
}

#[test]
fn a_usage_that_reports_only_a_total_is_billed_as_a_miss_not_as_free() {
    let pricing = Pricing::new(10.0, 1.0, 0.0);
    // 400 个没走缓存的输入 token，完全没有缓存细节。
    assert!(close(pricing.cost(usage(400, 0, 0, 0)), 0.004));
    // 一次部分切分：100 个未命中加 300 个命中。
    assert!(close(pricing.cost(usage(400, 0, 300, 0)), 0.0013));
}

#[test]
fn an_unpriced_model_has_no_cost_rather_than_a_zero_one() {
    let table = PriceTable::new().with("deepseek-flash", Pricing::new(1.0, 0.5, 2.0));

    assert_eq!(
        table.cost("mystery-model", usage(1_000_000, 0, 0, 0)),
        None,
        "「我们不知道这个多少钱」不能渲染成「免费」"
    );
    let priced = table
        .cost("deepseek-flash", usage(1_000_000, 0, 0, 0))
        .unwrap();
    assert!(close(priced, 1.0));
    assert!(PriceTable::default().is_empty());
}

#[test]
fn the_hard_stop_reads_the_summed_usage_and_not_an_estimate() {
    let budget = fs_agent::config::Budget::new().with_limit(1_000);

    assert!(!budget.is_exhausted(999));
    assert!(
        budget.is_exhausted(1_000),
        "正好落在上限上的会话就到此为止"
    );
    assert!(budget.is_exhausted(5_000));
    assert_eq!(budget.remaining(400), Some(600));
    assert_eq!(budget.remaining(5_000), Some(0));

    let uncapped = fs_agent::config::Budget::new();
    assert_eq!(uncapped.remaining(u64::MAX), None);
    assert!(!uncapped.is_exhausted(u64::MAX));
}

#[test]
fn the_pre_flight_threshold_is_a_multiple_of_what_is_left() {
    // 这个估计是字符数 / 4，会错几十个百分点，所以这道
    // 守卫是故意宽容的：只有估计值超出剩余额度、
    // 且超出那个余量时才拒。
    let budget = fs_agent::config::Budget::new().with_limit(1_000);
    assert!(budget.admits_estimate(0, 1_500));
    assert!(!budget.admits_estimate(0, 1_501));

    // 剩多少，门槛就跟着挪：剩 400，一个 600 的估计装得下。
    assert!(budget.admits_estimate(600, 600));
    assert!(!budget.admits_estimate(600, 601));

    // 余量取 1，就正好等于那条严格的「估计 > 剩余」比较，
    // 而宽容正是为了取代它才存在的。
    let strict = fs_agent::config::Budget::new()
        .with_limit(1_000)
        .with_estimate_margin(1.0);
    assert!(strict.admits_estimate(0, 1_000));
    assert!(!strict.admits_estimate(0, 1_001));

    // 没有上限就什么都放行，不管多大。
    assert!(fs_agent::config::Budget::new().admits_estimate(u64::MAX, u64::MAX));
}

#[test]
fn weak_model_routing_reaches_only_the_two_landing_points() {
    let routed = SessionConfig::new("discussion-model")
        .with_synthesizer_model("cheap-synthesizer")
        .with_executor_model("cheap-executor");

    assert_eq!(
        routed.model_for(LandingPoint::Synthesizer),
        "cheap-synthesizer"
    );
    assert_eq!(routed.model_for(LandingPoint::Executor), "cheap-executor");
    // 讨论者不是落点：它用自己的模型作答。
    assert_eq!(routed.model, "discussion-model");

    // v1 的默认：哪里都不覆盖，所以在有数据可路由之前，
    // 每个参与者都跑讨论本身那个模型。
    let unrouted = SessionConfig::new("discussion-model");
    assert_eq!(
        unrouted.model_for(LandingPoint::Synthesizer),
        "discussion-model"
    );
    assert_eq!(
        unrouted.model_for(LandingPoint::Executor),
        "discussion-model"
    );
}

#[test]
fn a_session_config_carries_the_price_table_and_the_session_budget() {
    let table = PriceTable::new().with("deepseek-flash", Pricing::new(1.0, 0.5, 2.0));
    let config = SessionConfig::new("deepseek-flash")
        .with_pricing(table)
        .with_session_token_limit(500);

    assert_eq!(config.budget.limit, Some(500));
    assert!(
        config.budget.estimate_margin > 1.0,
        "the default has to be the tolerant comparison"
    );
    let cost = config
        .pricing
        .cost("deepseek-flash", usage(1_000_000, 0, 0, 0))
        .unwrap();
    assert!(close(cost, 1.0));
}

// --- 每日账本 --------------------------------------------------------------

fn at(stamp: &str) -> DateTime<Utc> {
    stamp.parse().expect("一个 RFC 3339 时间戳")
}

fn usage_event(seq: u64, stamp: &str, input: u64, output: u64) -> Event {
    Event {
        seq,
        at: at(stamp),
        speaker_id: SpeakerId::Debater("kimi".into()),
        payload: EventPayload::UsageRecorded {
            usage: usage(input, output, 0, 0),
        },
    }
}

/// 手工写出一条流，这样测试就能把用量放在挑好的某个 UTC 日上：
/// 活着的日志是拿时钟给 `at` 盖章的。
fn write_log(path: &Path, events: &[Event]) {
    let mut text = String::new();
    text.push_str(
        &serde_json::to_string(&Event {
            seq: 0,
            at: at("2026-09-21T00:00:00Z"),
            speaker_id: SpeakerId::System,
            payload: EventPayload::SessionStarted {
                session_id: SessionId::new("s-ledger"),
                cwd: "/workspace".to_owned(),
                schema_version: SCHEMA_VERSION,
            },
        })
        .unwrap(),
    );
    text.push('\n');
    for event in events {
        text.push_str(&serde_json::to_string(event).unwrap());
        text.push('\n');
    }
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_daily_ledger_is_a_query_over_the_session_files_not_a_new_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path().join("sessions"));
    let today = NaiveDate::from_ymd_opt(2026, 9, 21).unwrap();

    let first = store.create(Path::new("/workspace/one")).unwrap();
    write_log(
        &first.log_path,
        &[
            usage_event(1, "2026-09-21T01:00:00Z", 100, 10),
            usage_event(2, "2026-09-21T23:59:59Z", 200, 20),
            usage_event(3, "2026-09-20T23:59:59Z", 999, 999),
        ],
    );
    let second = store.create(Path::new("/workspace/two")).unwrap();
    write_log(
        &second.log_path,
        &[usage_event(1, "2026-09-20T12:00:00Z", 7, 7)],
    );

    let today_ledger = ledger::for_day(&store, today).unwrap();
    assert_eq!(today_ledger.day, today);
    assert_eq!(
        today_ledger.sessions, 1,
        "只有那天有用量的会话才算进去"
    );
    assert_eq!(today_ledger.calls, 2);
    assert_eq!(today_ledger.usage.input_tokens, 300);
    assert_eq!(today_ledger.usage.output_tokens, 30);
    assert_eq!(today_ledger.tokens(), 330);

    // 昨天是它自己那一行，而且它跨两个桶：账本是一次查询，
    // 而不是一个按工作区算出来的数。
    let yesterday = ledger::for_day(&store, NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()).unwrap();
    assert_eq!(yesterday.sessions, 2);
    assert_eq!(yesterday.calls, 2);
    assert_eq!(yesterday.tokens(), 999 + 999 + 7 + 7);

    // 没有用量的一天是空的，而不是缺失。
    let idle = ledger::for_day(&store, NaiveDate::from_ymd_opt(2026, 9, 19).unwrap()).unwrap();
    assert_eq!(idle.sessions, 0);
    assert_eq!(idle.tokens(), 0);
}

#[test]
fn listing_the_store_reaches_every_bucket_and_skips_directories_without_a_stream() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path().join("sessions"));
    let mut sessions = Vec::new();
    for cwd in ["/workspace/one", "/workspace/two"] {
        let stored = store.create(Path::new(cwd)).unwrap();
        // 建流的是组装点，不是存储（spec §11），所以一个
        // 会话只有在它的日志存在之后才列得出来。
        fs_agent::events::EventLog::create(&stored.log_path).unwrap();
        sessions.push(stored);
    }
    // 在 `create` 与第一条 `SessionStarted` 之间，某个进程留下的
    // 目录里没有流，所以它不是一个会话。
    std::fs::create_dir_all(dir.path().join("sessions").join("stray")).unwrap();

    let listed = store.list_all().unwrap();
    assert_eq!(listed.len(), 2);
    let mut ids: Vec<&str> = listed.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    let mut want: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    want.sort_unstable();
    assert_eq!(ids, want);
}

#[test]
fn the_gate_owns_the_sentence_every_site_narrates() {
    let budget = fs_agent::config::Budget::new().with_limit(1_000);
    assert_eq!(budget.exhausted_note(999), None);
    let note = budget.exhausted_note(1_200).unwrap();
    assert!(note.contains("1200"), "{note}");
    assert!(note.contains("1000"), "{note}");

    let refusal = budget.estimate_refusal_note(9_000);
    assert!(refusal.contains("9000"), "{refusal}");
    assert!(refusal.contains("1000"), "{refusal}");

    // 没有上限：永远没有那句话。
    assert_eq!(
        fs_agent::config::Budget::new().exhausted_note(u64::MAX),
        None
    );
}
