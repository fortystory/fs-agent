//! 可观察性那批查询（spec §18）。
//!
//! 每一个视图都是对事件流的一次分组，所以测试手工搭出一条流、
//! 然后断言视图 —— 流是输入，视图才是契约。那两种 `--json`
//! 形态通过 serde 值来断言，因为那正是
//! 一条流水线消费的东西。

use fs_agent::config::{PriceTable, Pricing};
use fs_agent::events::{
    Decision, DecisionSource, EventLog, EventPayload, ParticipantId, Role, RoundMode, SessionId,
    SpeakerId, StopReason, ToolCallId, Usage, SCHEMA_VERSION,
};
use fs_agent::session::observe::{self, CostModel, Entry, Filter};
use fs_agent::session::SessionStore;
use fs_agent::tools::edit::EditError;
use fs_agent::tools::{MATCH_LEVEL_PREFIX, READ_BEFORE_WRITE_PREFIX, WROTE_PATH_PREFIX};

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn deepseek() -> SpeakerId {
    SpeakerId::Debater("deepseek".into())
}

fn log() -> (tempfile::TempDir, EventLog) {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::create(dir.path().join("log.jsonl")).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: SessionId::new("s-1"),
            cwd: "/workspace".to_owned(),
            schema_version: SCHEMA_VERSION,
        },
    )
    .unwrap();
    (dir, log)
}

fn round_starts(log: &mut EventLog, round: u32, mode: RoundMode) {
    log.append(
        SpeakerId::System,
        EventPayload::RoundStarted { round, mode },
    )
    .unwrap();
}

fn turn(log: &mut EventLog, speaker: &SpeakerId, iteration: u32) {
    log.append(
        speaker.clone(),
        EventPayload::TurnStarted {
            agent: speaker.clone(),
            iteration,
        },
    )
    .unwrap();
}

fn says(log: &mut EventLog, speaker: &SpeakerId, text: &str) {
    log.append(
        speaker.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
}

fn call(log: &mut EventLog, speaker: &SpeakerId, id: &str, tool: &str, args: serde_json::Value) {
    log.append(
        speaker.clone(),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    )
    .unwrap();
}

fn result(
    log: &mut EventLog,
    speaker: &SpeakerId,
    id: &str,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) {
    log.append(
        speaker.clone(),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            duration_ms: 3,
        },
    )
    .unwrap();
}

fn usage(log: &mut EventLog, speaker: &SpeakerId, input: u64, cached: u64, output: u64) {
    log.append(
        speaker.clone(),
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                miss_tokens: input - cached,
                reasoning_tokens: None,
            },
        },
    )
    .unwrap();
}

#[test]
fn the_timeline_groups_by_round_and_merges_a_tool_call_with_its_result_and_feedback() {
    let (_dir, mut log) = log();
    log.append(
        SpeakerId::User,
        EventPayload::ContextInjected {
            source: fs_agent::events::ContextSource::AgentsMd,
            content: "rules".to_owned(),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "怎么共享状态？".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    round_starts(&mut log, 1, RoundMode::Independent);
    turn(&mut log, &kimi(), 1);
    says(&mut log, &kimi(), "用事件流。\nCONCLUSION: 事件流");
    call(
        &mut log,
        &kimi(),
        "call-1",
        "edit_file",
        serde_json::json!({"file_path": "/workspace/a.rs"}),
    );
    result(
        &mut log,
        &kimi(),
        "call-1",
        true,
        Some(&format!(
            "{WROTE_PATH_PREFIX}/workspace/a.rs\nedit file: 1 replacement"
        )),
        None,
    );
    log.append(
        kimi(),
        EventPayload::HookExecuted {
            point: fs_agent::events::hook_format::POINT_POST.to_owned(),
            command: "fmt".to_owned(),
            outcome: fs_agent::events::hook_format::feedback("looks fine"),
        },
    )
    .unwrap();
    log.append(
        kimi(),
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();

    let timeline = observe::timeline(&log.events());
    assert_eq!(timeline.session_id, Some(SessionId::new("s-1")));
    assert_eq!(timeline.cwd.as_deref(), Some("/workspace"));
    assert_eq!(timeline.groups.len(), 2, "一段前奏加第一轮");

    let prelude = &timeline.groups[0];
    assert_eq!(prelude.round, None);
    assert!(prelude
        .entries
        .iter()
        .any(|entry| matches!(entry, Entry::Message { text, .. } if text == "怎么共享状态？")));

    let round = &timeline.groups[1];
    assert_eq!(round.round, Some(1));
    assert_eq!(round.mode, Some(RoundMode::Independent));
    assert_eq!(round.ended, Some(StopReason::NoDivergence));

    // 工具调用是一行：开始、结果与后置钩子的反馈合在一起。
    let tools: Vec<&Entry> = round
        .entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Tool { .. }))
        .collect();
    assert_eq!(tools.len(), 1, "一次调用一行，而不是三行：{tools:?}");
    match tools[0] {
        Entry::Tool {
            ok,
            tool,
            hook,
            duration_ms,
            ..
        } => {
            assert_eq!(*ok, Some(true));
            assert_eq!(tool, "edit_file");
            assert_eq!(duration_ms, &Some(3));
            assert_eq!(hook.as_deref(), Some("feedback: looks fine"));
        }
        other => panic!("期望一行工具，实际得到 {other:?}"),
    }
    // 后置钩子被合进去了，而不是自己再占一行。
    assert!(
        !round
            .entries
            .iter()
            .any(|entry| matches!(entry, Entry::Hook { .. })),
        "后置钩子的反馈属于它注解的那次调用"
    );
}

#[test]
fn a_filter_keeps_only_what_it_names() {
    let (_dir, mut log) = log();
    round_starts(&mut log, 1, RoundMode::Independent);
    turn(&mut log, &kimi(), 1);
    says(&mut log, &kimi(), "kimi's answer");
    turn(&mut log, &deepseek(), 1);
    says(&mut log, &deepseek(), "deepseek's answer");
    call(
        &mut log,
        &kimi(),
        "call-1",
        "edit_file",
        serde_json::json!({}),
    );
    let no_match = format!("/workspace/a.rs：{}", EditError::NoMatch);
    result(&mut log, &kimi(), "call-1", false, None, Some(&no_match));
    call(
        &mut log,
        &deepseek(),
        "call-2",
        "read_file",
        serde_json::json!({}),
    );
    result(
        &mut log,
        &deepseek(),
        "call-2",
        true,
        Some("contents"),
        None,
    );
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();

    let timeline = observe::timeline(&log.events());

    let by_speaker = timeline.filtered(&Filter {
        speaker: Some(kimi()),
        ..Filter::default()
    });
    assert!(by_speaker
        .groups
        .iter()
        .flat_map(|group| &group.entries)
        .all(|entry| entry.speaker() == Some(&kimi()) || entry.speaker().is_none()));

    let by_tool = timeline.filtered(&Filter {
        tool: Some("read_file".to_owned()),
        ..Filter::default()
    });
    let rows: Vec<&Entry> = by_tool.groups.iter().flat_map(|g| &g.entries).collect();
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows[0], Entry::Tool { tool, .. } if tool == "read_file"));

    // 合并后的工具行，对它取代的那两个事件名都答话。
    let by_kind = timeline.filtered(&Filter {
        kind: Some("ToolCallCompleted".to_owned()),
        ..Filter::default()
    });
    let rows: Vec<&Entry> = by_kind.groups.iter().flat_map(|g| &g.entries).collect();
    assert_eq!(rows.len(), 2, "两次调用都在，不只是后面被点名的那一次");

    let errors = timeline.filtered(&Filter {
        only_error: true,
        ..Filter::default()
    });
    let rows: Vec<&Entry> = errors.groups.iter().flat_map(|g| &g.entries).collect();
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0],
        Entry::Tool {
            ok: Some(false),
            ..
        }
    ));
}

#[test]
fn the_file_history_names_the_file_and_the_round_that_changed_it() {
    let (_dir, mut log) = log();
    round_starts(&mut log, 1, RoundMode::Independent);
    call(
        &mut log,
        &kimi(),
        "call-1",
        "write_file",
        serde_json::json!({}),
    );
    result(
        &mut log,
        &kimi(),
        "call-1",
        true,
        Some(&format!(
            "{WROTE_PATH_PREFIX}/workspace/a.rs\nwrite_file: created"
        )),
        None,
    );
    round_starts(&mut log, 2, RoundMode::Targeted);
    call(
        &mut log,
        &deepseek(),
        "call-2",
        "edit_file",
        serde_json::json!({}),
    );
    result(
        &mut log,
        &deepseek(),
        "call-2",
        true,
        Some(&format!(
            "{WROTE_PATH_PREFIX}/workspace/a.rs\n{MATCH_LEVEL_PREFIX}line-trim: 1 处替换"
        )),
        None,
    );
    // 失败的调用不算一次改动。
    call(
        &mut log,
        &deepseek(),
        "call-3",
        "edit_file",
        serde_json::json!({}),
    );
    result(&mut log, &deepseek(), "call-3", false, None, Some("boom"));

    let changes = observe::file_history(&log.events());
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes[0].path, "/workspace/a.rs");
    assert_eq!(changes[0].tool, "write_file");
    assert_eq!(changes[0].speaker, kimi());
    assert_eq!(changes[0].round, Some(1));
    assert_eq!(changes[1].tool, "edit_file");
    assert_eq!(changes[1].speaker, deepseek());
    assert_eq!(changes[1].round, Some(2));
}

#[test]
fn stats_report_the_two_silent_quantities_the_edit_ladder_and_absence() {
    let (_dir, mut log) = log();
    round_starts(&mut log, 1, RoundMode::Independent);
    says(&mut log, &kimi(), "kimi answered");
    log.append(
        kimi(),
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    )
    .unwrap();
    // 另一边以错误出局：单侧的缺席。
    log.append(
        deepseek(),
        EventPayload::TurnEnded {
            reason: StopReason::Error,
        },
    )
    .unwrap();

    // 一次降级的编辑、一次失败的匹配（它收回了读权限），
    // 以及一次写前先读的拒绝。
    call(
        &mut log,
        &kimi(),
        "call-1",
        "edit_file",
        serde_json::json!({}),
    );
    result(
        &mut log,
        &kimi(),
        "call-1",
        true,
        Some(&format!(
            "{WROTE_PATH_PREFIX}/workspace/a.rs\n{MATCH_LEVEL_PREFIX}line-trim: 1 处替换"
        )),
        None,
    );
    call(
        &mut log,
        &kimi(),
        "call-2",
        "edit_file",
        serde_json::json!({}),
    );
    // 生产者给路径加了前缀（`edit_file` 报的是 `"<path>: {error}"`）；
    // 指标必须透过这层去读，否则它会悄悄数成零。
    let no_match = format!("/workspace/a.rs：{}", EditError::NoMatch);
    result(&mut log, &kimi(), "call-2", false, None, Some(&no_match));
    call(
        &mut log,
        &kimi(),
        "call-3",
        "write_file",
        serde_json::json!({}),
    );
    result(
        &mut log,
        &kimi(),
        "call-3",
        false,
        None,
        Some(&format!(
            "{READ_BEFORE_WRITE_PREFIX}/workspace/a.rs 已存在，但本次会话里还没读过它；\
             先读它一遍再改"
        )),
    );
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();

    let stats = observe::stats(&log.events(), None);

    assert_eq!(stats.edits.succeeded, 1);
    assert_eq!(stats.edits.levels.len(), 1);
    assert_eq!(stats.edits.levels[0].name, "line-trim");
    assert_eq!(stats.edits.levels[0].count, 1);
    assert_eq!(stats.edits.failed_matches, 1);
    assert_eq!(stats.guards.invalidated_reads, 1);
    assert_eq!(stats.guards.read_before_write, 1);

    assert_eq!(stats.absence.rounds, 1);
    assert_eq!(stats.absence.one_sided, 1);
    assert_eq!(stats.absence.rate, Some(1.0));
    assert_eq!(stats.absence.per_speaker[0].name, "deepseek");

    assert_eq!(stats.stops[0].name, "NoDivergence");
    assert_eq!(stats.stops[0].count, 1);
}

#[test]
fn stats_report_per_agent_tokens_hit_rate_rounds_and_permissions() {
    let (_dir, mut log) = log();
    round_starts(&mut log, 1, RoundMode::Independent);
    usage(&mut log, &kimi(), 100, 80, 10);
    usage(&mut log, &deepseek(), 100, 0, 20);
    log.append(
        kimi(),
        EventPayload::PermissionAsked {
            request_id: "r-1".to_owned(),
            tool_call_id: ToolCallId::new("call-1"),
            request: serde_json::json!({"tool": "bash"}),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::PermissionDecided {
            request_id: "r-1".to_owned(),
            decision: Decision::Ask,
            source: DecisionSource::Policy,
            reason: None,
        },
    )
    .unwrap();
    log.append(
        kimi(),
        EventPayload::HookExecuted {
            point: fs_agent::events::hook_format::POINT_PRE.to_owned(),
            command: "guard".to_owned(),
            outcome: fs_agent::events::hook_format::OUTCOME_CONTINUE.to_owned(),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::ExecutorSpawned {
            executor_id: ParticipantId::new("kimi-1"),
            parent: ParticipantId::new("kimi"),
            brief: "do it".to_owned(),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::ExecutorFinished {
            executor_id: ParticipantId::new("kimi-1"),
            reason: StopReason::MaxIterations,
            summary: "ran out".to_owned(),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();

    let stats = observe::stats(&log.events(), None);
    assert_eq!(stats.session.calls, 2);
    assert_eq!(stats.session.rounds, 1);
    assert_eq!(stats.session.tokens.total_tokens(), 230);
    assert_eq!(stats.rounds[0].calls, 2, "两次调用都归第一轮");

    let kimi_stats = stats
        .speakers
        .iter()
        .find(|agent| agent.speaker == kimi())
        .unwrap();
    assert_eq!(kimi_stats.tokens.cached_tokens, 80);
    assert_eq!(kimi_stats.hit_rate, Some(0.8));
    assert_eq!(kimi_stats.calls, 1);

    assert_eq!(stats.permissions.asked, 1);
    assert_eq!(stats.permissions.decided[0].name, "ask");
    assert_eq!(stats.hooks.executed, 1);
    assert_eq!(stats.hooks.pre, 1);
    assert_eq!(stats.executors.spawned, 1);
    assert_eq!(stats.executors.finished, 1);
    assert_eq!(stats.executors.by_reason[0].name, "MaxIterations");

    // 没点名模型就没有钱可看 —— 流里本来也没带。
    assert_eq!(stats.session.cost, None);
    assert!(stats.speakers.iter().all(|agent| agent.cost.is_none()));
}

#[test]
fn stats_price_the_session_only_when_the_caller_names_the_model() {
    let (_dir, mut log) = log();
    round_starts(&mut log, 1, RoundMode::Independent);
    usage(&mut log, &kimi(), 1_000_000, 400_000, 100_000);
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();

    let mut pricing = PriceTable::new();
    pricing.set("model-x", Pricing::new(1.0, 0.1, 2.0));
    let cost = CostModel::new("model-x", pricing);

    let stats = observe::stats(&log.events(), Some(&cost));
    let expected = 600_000.0 * 1.0 / 1e6 + 400_000.0 * 0.1 / 1e6 + 100_000.0 * 2.0 / 1e6;
    let actual = stats.session.cost.expect("模型被点了名，所以有成本");
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
    assert_eq!(stats.speakers[0].speaker, kimi());
}

#[test]
fn list_summarizes_a_stored_session_from_its_own_stream() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path().join("sessions"));
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let stored = store.create(&cwd).unwrap();
    let mut log = EventLog::create(&stored.log_path).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: stored.id.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            schema_version: SCHEMA_VERSION,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "hi".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionEnded {
            reason: StopReason::Completed,
        },
    )
    .unwrap();

    let listings = observe::list(&store, Some(&cwd)).unwrap();
    assert_eq!(listings.len(), 1);
    assert_eq!(listings[0].id, stored.id);
    assert_eq!(
        listings[0].cwd.as_deref(),
        Some(cwd.to_string_lossy().as_ref())
    );
    assert_eq!(listings[0].ended, Some(StopReason::Completed));
    assert_eq!(listings[0].messages, 1);
    assert!(listings[0].written.is_some());

    // 调用者不分桶时，就是每一个桶。
    let all = observe::list(&store, None).unwrap();
    assert_eq!(all.len(), 1);
}

#[test]
fn a_view_serializes_to_json_a_pipeline_can_read() {
    let (_dir, mut log) = log();
    round_starts(&mut log, 1, RoundMode::Independent);
    says(&mut log, &kimi(), "hello");
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();

    let timeline = observe::timeline(&log.events());
    let value = serde_json::to_value(&timeline).unwrap();
    let round = &value["groups"][1];
    assert_eq!(round["round"], 1);
    assert_eq!(round["entries"][0]["entry"], "round_started");
    assert_eq!(round["entries"][1]["entry"], "message");
    assert_eq!(round["entries"][1]["text"], "hello");

    let stats = serde_json::to_value(observe::stats(&log.events(), None)).unwrap();
    assert!(stats["absence"]["rounds"].is_number());
    assert!(stats["edits"]["levels"].is_array());
}

#[test]
fn an_empty_filter_is_recognized() {
    let filter = Filter::default();
    assert!(filter.is_empty());
    assert!(!Filter {
        only_error: true,
        ..Filter::default()
    }
    .is_empty());
}
