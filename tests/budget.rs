//! Cost and the session's spend cap (spec §17).
//!
//! Three seams, all of them the ones the spec already names:
//!
//! * the pure rules — the price table, the gate, the routing rule — are called
//!   directly, with no provider and no event stream;
//! * the daily ledger goes through the public store, because "derived from the
//!   session files" is the whole point of it;
//! * the gates themselves are exercised through the library assembly seam, in
//!   the files that already own those fixtures: the round loop in
//!   `discussion.rs`, the executor port in `executor.rs`, and the turn loop in
//!   `e2e_single_turn.rs`.

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
    // `cached` and `miss` are a split of `input`, and both vendors already count
    // reasoning tokens inside `output`; adding either on top would inflate the
    // number the gate reads.
    assert_eq!(usage(100, 20, 80, 20).total_tokens(), 120);
}

#[test]
fn a_cache_hit_and_a_cache_miss_are_priced_apart() {
    // USD per million tokens: a miss costs ten times a hit, output costs ten
    // times a miss. 0.1 Mtok miss + 0.9 Mtok hit + 0.001 Mtok out =
    // 1.0 + 0.9 + 0.1.
    let pricing = Pricing::new(10.0, 1.0, 100.0);
    let mixed = usage(1_000_000, 1_000, 900_000, 100_000);
    assert!(close(pricing.cost(mixed), 2.0));

    let all_miss = usage(1_000_000, 0, 0, 1_000_000);
    let all_cached = usage(1_000_000, 0, 1_000_000, 0);
    assert!(
        pricing.cost(all_miss) > pricing.cost(all_cached),
        "pricing the split apart has to show up in the money"
    );
}

#[test]
fn a_usage_that_reports_only_a_total_is_billed_as_a_miss_not_as_free() {
    let pricing = Pricing::new(10.0, 1.0, 0.0);
    // 400 uncached input tokens, no cache detail at all.
    assert!(close(pricing.cost(usage(400, 0, 0, 0)), 0.004));
    // A partial split: 100 misses and 300 hits.
    assert!(close(pricing.cost(usage(400, 0, 300, 0)), 0.0013));
}

#[test]
fn an_unpriced_model_has_no_cost_rather_than_a_zero_one() {
    let table = PriceTable::new().with("deepseek-flash", Pricing::new(1.0, 0.5, 2.0));

    assert_eq!(
        table.cost("mystery-model", usage(1_000_000, 0, 0, 0)),
        None,
        "\"we do not know what this costs\" must not render as \"free\""
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
        "a session that lands exactly on its cap is done"
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
    // The estimate is chars / 4, wrong by tens of percent, so the guard is
    // deliberately tolerant: it refuses only once the estimate exceeds the
    // remaining allowance by the margin.
    let budget = fs_agent::config::Budget::new().with_limit(1_000);
    assert!(budget.admits_estimate(0, 1_500));
    assert!(!budget.admits_estimate(0, 1_501));

    // What is left moves the threshold with it: 400 left, a 600 estimate fits.
    assert!(budget.admits_estimate(600, 600));
    assert!(!budget.admits_estimate(600, 601));

    // A margin of one is exactly the strict "estimate > remaining" comparison
    // the tolerance exists to replace.
    let strict = fs_agent::config::Budget::new()
        .with_limit(1_000)
        .with_estimate_margin(1.0);
    assert!(strict.admits_estimate(0, 1_000));
    assert!(!strict.admits_estimate(0, 1_001));

    // No cap admits everything, at any size.
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
    // A debater is not a landing point: its own model is what it answers with.
    assert_eq!(routed.model, "discussion-model");

    // v1's default: no override anywhere, so every participant runs the
    // discussion's model until there is data to route on.
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

// --- the daily ledger ------------------------------------------------------

fn at(stamp: &str) -> DateTime<Utc> {
    stamp.parse().expect("an RFC 3339 stamp")
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

/// Write a stream by hand, so a test can place usage on a chosen UTC day: the
/// live log stamps `at` with the clock.
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
        "only a session with usage that day is counted"
    );
    assert_eq!(today_ledger.calls, 2);
    assert_eq!(today_ledger.usage.input_tokens, 300);
    assert_eq!(today_ledger.usage.output_tokens, 30);
    assert_eq!(today_ledger.tokens(), 330);

    // Yesterday is its own row, and it spans both buckets: a ledger is a query,
    // not a per-workspace number.
    let yesterday = ledger::for_day(&store, NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()).unwrap();
    assert_eq!(yesterday.sessions, 2);
    assert_eq!(yesterday.calls, 2);
    assert_eq!(yesterday.tokens(), 999 + 999 + 7 + 7);

    // A day with no usage is empty, not missing.
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
        // The assembly point creates the stream, not the store (spec §11), so a
        // session is only listable once its log exists.
        fs_agent::events::EventLog::create(&stored.log_path).unwrap();
        sessions.push(stored);
    }
    // A directory a process left between `create` and the first `SessionStarted`
    // holds no stream, so it is not a session.
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

    // No cap: no note, ever.
    assert_eq!(
        fs_agent::config::Budget::new().exhausted_note(u64::MAX),
        None
    );
}
