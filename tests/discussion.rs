//! 讨论协议（spec §15）。
//!
//! 这里练两条接缝。纯比较函数直接测 —— spec 的
//! Testing Decisions 把「对归一化结论的相等 / 子串判定」列为
//! 不需要 mock 接缝的纯函数之一。轮次循环本身则走
//! 库的组装入口，配一个脚本化的假 provider，断言
//! JSONL 事件流与
//! 两个渲染 sink。

mod support;

use std::path::PathBuf;

use heng::config::SessionConfig;
use heng::discussion::protocol::{
    answers_agree, conclusion_of, normalize, round_attendance, round_outcome, RoundOutcome,
    CONCLUSION_MARKER,
};
use heng::discussion::{
    debater_identity, pick_pair, plan_after_round, synthesis_prompt, RoundPlan,
};
use heng::events::{
    read_events, ContextSource, Event, EventLog, EventPayload, Role, RoundMode, SessionId,
    SpeakerId, StopReason, Usage, SCHEMA_VERSION,
};
use heng::permissions::{Mode, Policy};
use heng::provider::capability::caps_for;
use heng::provider::{FinishReason, Message, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::{
    assemble, assemble_discussion, AssemblyParts, DebaterParts, DiscussionHarness, DiscussionParts,
    Error, Harness, SessionScaffold, SynthesizerParts,
};
use support::{CaptureBuf, FakeProvider, Reply};

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn deepseek() -> SpeakerId {
    SpeakerId::Debater("deepseek".into())
}

fn executor() -> SpeakerId {
    SpeakerId::Executor("e-1".into())
}

/// 从一个脚本搭出一份日志，并让临时目录与它一起活下来。
fn build_log(script: impl FnOnce(&mut EventLog)) -> (tempfile::TempDir, EventLog) {
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
    script(&mut log);
    (dir, log)
}

fn round_starts(log: &mut EventLog, round: u32, mode: RoundMode) {
    log.append(
        SpeakerId::System,
        EventPayload::RoundStarted { round, mode },
    )
    .unwrap();
}

fn round_ends(log: &mut EventLog, round: u32, reason: StopReason) {
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded { round, reason },
    )
    .unwrap();
}

fn says(log: &mut EventLog, speaker: &SpeakerId, text: &str) {
    log.append(
        speaker.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: Some("private reasoning".to_owned()),
        },
    )
    .unwrap();
}

fn turn_ends(log: &mut EventLog, speaker: &SpeakerId, reason: StopReason) {
    log.append(speaker.clone(), EventPayload::TurnEnded { reason })
        .unwrap();
}

/// 讨论者写下的一个答案：正文，然后是那行标记。
fn answer(body: &str, conclusion: &str) -> String {
    format!("{body}\nCONCLUSION: {conclusion}")
}

#[test]
fn a_pair_is_drawn_from_the_pool_without_repeating_a_member() {
    // 抽取是池子大小与一个种子的纯函数，所以一场讨论可以说清
    // 它跑的是哪一对，而测试能钉住这个选择、而不是分布。
    assert_eq!(pick_pair(2, 0), Some((0, 1)));
    assert_eq!(
        pick_pair(2, 12_345),
        Some((0, 1)),
        "两个的池子不论种子如何都自己跟自己讨论"
    );
    assert_eq!(pick_pair(1, 7), None, "一个成员撑不起一场讨论");
    assert_eq!(pick_pair(0, 7), None);

    // 三个的池子：每次抽取都是两个不同成员、按池子顺序，
    // 而种子真的会挪动这一对（在一个不大的范围里三对都会出现）。
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..40u64 {
        let (first, second) = pick_pair(3, seed).expect("一对");
        assert!(first < second, "种子 {seed}：先 {first} 后 {second}");
        assert!(second < 3, "种子 {seed}：{second} 出了池子");
        seen.insert((first, second));
    }
    assert_eq!(
        seen,
        std::collections::BTreeSet::from([(0, 1), (0, 2), (1, 2)]),
        "三个的池子能产出每一对"
    );
}

#[test]
fn the_conclusion_is_the_text_after_the_marker_on_its_own_line() {
    let text = answer("两个方案都能跑。", "复用已有的共享事件流");
    assert_eq!(conclusion_of(&text), Some("复用已有的共享事件流"));
}

#[test]
fn the_last_marker_line_wins_so_trailing_prose_cannot_hide_the_conclusion() {
    let text = "先说一句。\nCONCLUSION: 一个临时的中间结论\n补充两句解释。\nCONCLUSION: 最终结论";
    assert_eq!(conclusion_of(text), Some("最终结论"));
}

#[test]
fn an_answer_without_the_marker_has_no_conclusion() {
    assert_eq!(conclusion_of("我只说了正文，没有给结论。"), None);
    // 不在行首的标记，是在讲标记本身的正文，
    // 不是结论。
    assert_eq!(conclusion_of("我的 CONCLUSION: 藏在句子中间"), None);
    // 空结论不是结论。
    assert_eq!(conclusion_of("正文\nCONCLUSION:   "), None);
}

#[test]
fn a_decorated_marker_still_reads_as_a_conclusion() {
    // 模型可能把标记包在强调里，或者在它前面加个列表项。把那
    // 读成「没有结论」会是一次假阴性：两侧会看起来
    // 分歧，而协议白白买来第二轮。
    assert_eq!(
        conclusion_of("正文\n**CONCLUSION:** 复用事件流"),
        Some("复用事件流")
    );
    assert_eq!(
        conclusion_of("正文\n- CONCLUSION: 复用事件流"),
        Some("复用事件流")
    );
    assert_eq!(
        conclusion_of("正文\n> `CONCLUSION:` 复用事件流"),
        Some("复用事件流")
    );
}

#[test]
fn emphasis_punctuation_and_case_do_not_make_equal_conclusions_differ() {
    let left = answer("正文 A", "**复用事件流**。");
    let right = answer("正文 B", "复用事件流");
    assert!(answers_agree(&left, &right));
}

#[test]
fn whitespace_and_case_are_normalized_away() {
    let left = answer("正文 A", "  Reuse   the Event Stream  ");
    let right = answer("正文 B", "reuse the event stream");
    assert!(answers_agree(&left, &right));
}

#[test]
fn a_substring_conclusion_counts_as_agreement_in_either_direction() {
    // spec 的那条机械规则：精确相等**或**子串包含，
    // 都先过一次归一化。包含是双向检查的，所以更长那个答案多出来的
    // 限定语不会被读成分歧。
    let short = answer("正文 A", "复用事件流");
    let long = answer("正文 B", "应该复用事件流");
    assert!(answers_agree(&short, &long));
    assert!(answers_agree(&long, &short));
}

#[test]
fn different_conclusions_do_not_agree() {
    let left = answer("正文 A", "复用已有的共享事件流");
    let right = answer("正文 B", "每个 agent 各写一份日志");
    assert!(!answers_agree(&left, &right));
}

#[test]
fn an_answer_without_a_conclusion_never_agrees() {
    // 这是 spec 点名的那次危险误读：一个答案（或者一个都没有）
    // 不许坍缩成「他们一致」。
    let with_marker = answer("正文", "复用事件流");
    let without_marker = "只有正文，没有结论。";
    assert!(!answers_agree(&with_marker, without_marker));
    assert!(!answers_agree(without_marker, &with_marker));
    assert!(!answers_agree(without_marker, without_marker));
}

#[test]
fn an_empty_conclusion_never_agrees_even_though_it_is_a_substring_of_everything() {
    assert!(!answers_agree(
        "正文\nCONCLUSION:",
        "正文\nCONCLUSION: 复用事件流"
    ));
    assert_eq!(normalize(""), "");
}

#[test]
fn a_round_reports_who_answered_and_who_was_absent() {
    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("正文", "复用事件流"));
        turn_ends(log, &deepseek(), StopReason::Error);
        round_ends(log, 1, StopReason::NoDivergence);
    });

    let attendance = round_attendance(&log.events(), 1);
    assert_eq!(
        attendance.answers,
        vec![(kimi(), answer("正文", "复用事件流"))]
    );
    assert_eq!(attendance.absent, vec![deepseek()]);
}

#[test]
fn a_side_that_answered_is_present_even_if_its_turn_then_failed() {
    // spec 的缺席查询是「以 `Error` 结束**并且**没留下消息」：
    // 在失败之前落地的那条作答仍然是作答，把它读成
    // 缺席会丢掉一个真实的立场。
    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("正文", "复用事件流"));
        turn_ends(log, &kimi(), StopReason::Error);
        round_ends(log, 1, StopReason::NoDivergence);
    });

    let attendance = round_attendance(&log.events(), 1);
    assert_eq!(attendance.answers.len(), 1);
    assert!(attendance.absent.is_empty());
}

#[test]
fn rounds_do_not_leak_into_each_other() {
    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("K1", "复用事件流"));
        says(log, &deepseek(), &answer("D1", "各写一份日志"));
        round_ends(log, 1, StopReason::RoundsExhausted);
        round_starts(log, 2, RoundMode::Targeted);
        says(log, &kimi(), &answer("K2", "复用事件流"));
        turn_ends(log, &deepseek(), StopReason::Error);
        round_ends(log, 2, StopReason::Consensus);
    });

    let first = round_attendance(&log.events(), 1);
    assert_eq!(first.answers.len(), 2);
    assert!(first.absent.is_empty());

    let second = round_attendance(&log.events(), 2);
    assert_eq!(second.answers, vec![(kimi(), answer("K2", "复用事件流"))]);
    assert_eq!(second.absent, vec![deepseek()]);
}

#[test]
fn executors_and_the_synthesizer_are_not_debaters_in_a_round() {
    // 执行者的回合落在派出它的那一轮里面，走的是同一条
    // 流（spec §16），而合成器的产物是 `System`（spec §2）。
    // 两者都不是讨论者的作答，所以两者都不许被算作
    // 作答 —— 否则协议会在一个讨论者与它自己的执行者之间找到一致。
    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("正文", "复用事件流"));
        says(log, &executor(), "执行者的摘要");
        turn_ends(log, &executor(), StopReason::Error);
        round_ends(log, 1, StopReason::NoDivergence);
        round_starts(log, 2, RoundMode::Synthesis);
        says(log, &SpeakerId::System, "共识 / 分歧 / 未决");
        round_ends(log, 2, StopReason::Completed);
    });

    let first = round_attendance(&log.events(), 1);
    assert_eq!(first.answers, vec![(kimi(), answer("正文", "复用事件流"))]);
    assert!(first.absent.is_empty());

    let synthesis = round_attendance(&log.events(), 2);
    assert!(synthesis.answers.is_empty());
    assert!(synthesis.absent.is_empty());
}

#[test]
fn an_error_outside_a_round_is_not_an_absence_in_one() {
    let (_dir, log) = build_log(|log| {
        turn_ends(log, &deepseek(), StopReason::Error);
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("正文", "复用事件流"));
        round_ends(log, 1, StopReason::NoDivergence);
    });

    let attendance = round_attendance(&log.events(), 1);
    assert_eq!(attendance.answers.len(), 1);
    assert!(attendance.absent.is_empty());
}

#[test]
fn the_mechanical_verdict_reads_agreement_out_of_the_two_conclusions() {
    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("K", "复用事件流"));
        says(log, &deepseek(), &answer("D", "应该复用事件流"));
        round_ends(log, 1, StopReason::NoDivergence);
    });
    assert_eq!(
        round_outcome(&round_attendance(&log.events(), 1)),
        RoundOutcome::Agreed
    );

    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("K", "复用事件流"));
        says(log, &deepseek(), &answer("D", "每个 agent 各写一份日志"));
        round_ends(log, 1, StopReason::NoDivergence);
    });
    assert_eq!(
        round_outcome(&round_attendance(&log.events(), 1)),
        RoundOutcome::Diverged
    );

    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("K", "复用事件流"));
        turn_ends(log, &deepseek(), StopReason::Error);
        round_ends(log, 1, StopReason::NoDivergence);
    });
    assert_eq!(
        round_outcome(&round_attendance(&log.events(), 1)),
        RoundOutcome::Incomplete
    );
}

#[test]
fn agreement_in_the_first_round_ends_the_debate_without_divergence() {
    // 一轮，没有人分歧：协议从不需要第二轮，
    // 而原因说的正是这个。
    assert_eq!(
        plan_after_round(RoundOutcome::Agreed, 1, 2),
        RoundPlan::Stop(StopReason::NoDivergence)
    );
}

#[test]
fn agreement_after_a_targeted_round_is_consensus() {
    assert_eq!(
        plan_after_round(RoundOutcome::Agreed, 2, 2),
        RoundPlan::Stop(StopReason::Consensus)
    );
}

#[test]
fn a_conflict_opens_exactly_one_targeted_round() {
    assert_eq!(
        plan_after_round(RoundOutcome::Diverged, 1, 2),
        RoundPlan::TargetedRound
    );
}

#[test]
fn a_conflict_at_the_cap_is_rounds_exhausted() {
    assert_eq!(
        plan_after_round(RoundOutcome::Diverged, 2, 2),
        RoundPlan::Stop(StopReason::RoundsExhausted)
    );
}

#[test]
fn a_one_sided_round_stops_debating_and_leaves_the_absence_to_the_stream() {
    // 什么都没比较，所以什么都没一致。这一轮照样结束 —— 用
    // 主张最少的那条原因 —— 而缺席的那一方仍然以它自己的
    // `TurnEnded { Error }` 可见，而不是作为这条事件上的一个字段。
    assert_eq!(
        plan_after_round(RoundOutcome::Incomplete, 1, 2),
        RoundPlan::Stop(StopReason::NoDivergence)
    );
}

#[test]
fn the_debater_identity_teaches_the_marker_the_parser_reads() {
    // 提示词与解析器共用同一个常量；一条指令教了、解析器
    // 却不认识其标记，会让每一轮都看起来分歧。
    let identity = debater_identity("kimi");
    assert!(identity.contains("kimi"));
    assert!(identity.contains(CONCLUSION_MARKER));
}

#[test]
fn the_synthesizer_prompt_reveals_every_answer_and_names_every_absence() {
    let (_dir, log) = build_log(|log| {
        round_starts(log, 1, RoundMode::Independent);
        says(log, &kimi(), &answer("KIMI 的作答正文", "复用事件流"));
        turn_ends(log, &deepseek(), StopReason::Error);
        round_ends(log, 1, StopReason::NoDivergence);
    });

    let prompt = synthesis_prompt("该不该复用事件流？", &log.events(), 1);

    assert!(prompt.contains("该不该复用事件流？"));
    assert!(prompt.contains("KIMI 的作答正文"));
    assert!(prompt.contains("独立"));
    // 那次危险的误读：只回来一个答案，所以必须告诉合成器
    // 另一方缺席，而不是把那唯一一个答案读成
    // 共识。
    assert!(prompt.contains("deepseek"));
    assert!(prompt.contains("缺席"));
    // 发言会被揭示；私有推理永远不。
    assert!(!prompt.contains("private reasoning"));
}

// ---------------------------------------------------------------------------
// 轮次循环，走组装接缝。
// ---------------------------------------------------------------------------

struct Fixture {
    harness: DiscussionHarness,
    kimi: FakeProvider,
    deepseek: FakeProvider,
    synthesizer: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(
    kimi_replies: Vec<Reply>,
    deepseek_replies: Vec<Reply>,
    synthesizer_replies: Vec<Reply>,
    max_rounds: Option<u32>,
) -> Fixture {
    fixture_with(
        FakeProvider::new(kimi_replies),
        FakeProvider::new(deepseek_replies),
        FakeProvider::new(synthesizer_replies),
        max_rounds,
    )
    .await
}

async fn fixture_with(
    kimi_provider: FakeProvider,
    deepseek_provider: FakeProvider,
    synthesizer_provider: FakeProvider,
    max_rounds: Option<u32>,
) -> Fixture {
    let config = SessionConfig::new("fake-model");
    fixture_with_configs(
        kimi_provider,
        deepseek_provider,
        synthesizer_provider,
        max_rounds,
        [config.clone(), config.clone(), config],
    )
    .await
}

/// 同上，只是每个参与者各带一份配置：一个讨论者、另一个、
/// 合成器。一场讨论共用一个 token 额度（spec §17），所以
/// 这些配置只在测试与改派有关的地方不同。
async fn fixture_with_configs(
    kimi_provider: FakeProvider,
    deepseek_provider: FakeProvider,
    synthesizer_provider: FakeProvider,
    max_rounds: Option<u32>,
    configs: [SessionConfig; 3],
) -> Fixture {
    let [kimi_config, deepseek_config, synthesizer_config] = configs;
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();

    let harness = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-discussion"),
            tools: heng::tools::builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
        debaters: vec![
            DebaterParts {
                speaker: kimi(),
                config: kimi_config,
                provider: Box::new(kimi_provider.clone()),
                soul: None,
            },
            DebaterParts {
                speaker: deepseek(),
                config: deepseek_config,
                provider: Box::new(deepseek_provider.clone()),
                soul: None,
            },
        ],
        synthesizer: SynthesizerParts {
            config: synthesizer_config,
            provider: Box::new(synthesizer_provider.clone()),
        },
        max_rounds,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
    })
    .await
    .unwrap();

    Fixture {
        harness,
        kimi: kimi_provider,
        deepseek: deepseek_provider,
        synthesizer: synthesizer_provider,
        stdout,
        stderr,
        log_path,
        _dir: dir,
    }
}

/// 讨论者写下的一个答案。
fn answered(body: &str, conclusion: &str) -> Reply {
    Reply::text(&answer(body, conclusion))
}

fn kinds(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(|event| event.payload.kind()).collect()
}

/// 每条 `RoundEnded` 的 `(round, reason)`，按顺序。
fn round_endings(events: &[Event]) -> Vec<(u32, StopReason)> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::RoundEnded { round, reason } => Some((*round, *reason)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_discussion_without_divergence_takes_three_calls() {
    let mut fixture = fixture(
        vec![answered("KIMI 的正文", "复用事件流")],
        vec![answered("DEEPSEEK 的正文", "应该复用事件流")],
        vec![Reply::text("共识：复用事件流\n分歧：无\n未决：无")],
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    // 两个讨论者加一次收尾调用。机械判定不花什么。
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    assert_eq!(outcome.reason, StopReason::NoDivergence);
    assert_eq!(outcome.rounds, 1);
    assert!(outcome.absent.is_empty());
    assert_eq!(outcome.synthesis, "共识：复用事件流\n分歧：无\n未决：无");

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::NoDivergence), (2, StopReason::Completed)]
    );
    // 合成器的产物是 harness 自己的声音（spec §2），
    // 而问题是用用户的。
    let products: Vec<&EventPayload> = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventPayload::MessageCompleted {
                    role: Role::Assistant,
                    ..
                }
            )
        })
        .map(|event| &event.payload)
        .collect();
    assert!(products.iter().any(|payload| matches!(
        payload,
        EventPayload::MessageCompleted { text, .. } if text == "共识：复用事件流\n分歧：无\n未决：无"
    )));
}

#[tokio::test]
async fn a_discussion_that_stays_divergent_takes_five_calls_and_ends_exhausted() {
    let mut fixture = fixture(
        vec![
            answered("KIMI 第一轮", "复用事件流"),
            answered("KIMI 第二轮", "复用事件流"),
        ],
        vec![
            answered("DEEPSEEK 第一轮", "每个 agent 各写一份日志"),
            answered("DEEPSEEK 第二轮", "每个 agent 各写一份日志"),
        ],
        vec![Reply::text("共识：无\n分歧：日志归属\n未决：性能")],
        Some(2),
    )
    .await;

    let outcome = fixture
        .harness
        .discuss("日志该复用还是各写一份？")
        .await
        .unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(fixture.kimi.requests().len(), 2);
    assert_eq!(fixture.deepseek.requests().len(), 2);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    assert_eq!(outcome.reason, StopReason::RoundsExhausted);
    assert_eq!(outcome.rounds, 2);

    let events = read_events(&fixture.log_path).unwrap();
    // 只有结束这场辩论的那一轮带 `RoundEnded`；第二轮的
    // 边界是合成轮的 `RoundStarted`。
    assert_eq!(
        round_endings(&events),
        vec![(2, StopReason::RoundsExhausted), (3, StopReason::Completed)]
    );
    let divergences: Vec<(u32, String, Vec<String>)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::DivergenceRecorded {
                round,
                topic,
                positions,
            } => Some((*round, topic.clone(), positions.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        divergences,
        vec![
            (
                1,
                "日志该复用还是各写一份？".to_owned(),
                vec![
                    "复用事件流".to_owned(),
                    "每个 agent 各写一份日志".to_owned()
                ]
            ),
            (
                2,
                "日志该复用还是各写一份？".to_owned(),
                vec![
                    "复用事件流".to_owned(),
                    "每个 agent 各写一份日志".to_owned()
                ]
            ),
        ]
    );
    assert!(kinds(&events).contains(&"RoundStarted"));
}

#[tokio::test]
async fn agreement_in_the_targeted_round_is_consensus() {
    let mut fixture = fixture(
        vec![
            answered("KIMI 第一轮", "复用事件流"),
            answered("KIMI 第二轮被说服", "复用事件流"),
        ],
        vec![
            answered("DEEPSEEK 第一轮", "每个 agent 各写一份日志"),
            answered("DEEPSEEK 第二轮改口", "复用事件流"),
        ],
        vec![Reply::text("共识：复用事件流")],
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("日志该怎么放？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Consensus);
    assert_eq!(outcome.rounds, 2);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        round_endings(&events),
        vec![(2, StopReason::Consensus), (3, StopReason::Completed)]
    );
}

#[tokio::test]
async fn a_discussion_refuses_a_roster_that_is_not_two_debaters() {
    // N = 2 不是实现细节：「N = 2 不做仲裁」这条决定
    // 正是机械判定够用的原因，所以第三个讨论者必须重新
    // 打开那条决定，而不是溜进来（spec §15，明确不做）。
    let dir = tempfile::tempdir().unwrap();
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-discussion"),
            tools: heng::tools::builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
        debaters: vec![DebaterParts {
            speaker: kimi(),
            config: SessionConfig::new("fake-model"),
            provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
            soul: None,
        }],
        synthesizer: SynthesizerParts {
            config: SessionConfig::new("fake-model"),
            provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
        },
        max_rounds: Some(2),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
    })
    .await;

    let error = match assembled {
        Ok(_) => panic!("只有一个讨论者的名册必须被拒"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Discussion(_)), "got {error:?}");
}

#[tokio::test]
async fn a_discussion_refuses_two_debaters_that_share_one_identity() {
    // 两个讨论者可以是同一个*模型* —— 一份订阅不是「干脆不讨论」
    // 的理由 —— 但它们不许是同一个*参与者*：每一次投影都是
    // `speaker_id` 的函数，所以同一个名字会在定向轮里把
    // 另一方的答案当作自己的交给各方（spec §5）。
    let dir = tempfile::tempdir().unwrap();
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-discussion"),
            tools: heng::tools::builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
        debaters: vec![
            DebaterParts {
                speaker: kimi(),
                config: SessionConfig::new("fake-model"),
                provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
                soul: None,
            },
            DebaterParts {
                speaker: kimi(),
                config: SessionConfig::new("fake-model"),
                provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
                soul: None,
            },
        ],
        synthesizer: SynthesizerParts {
            config: SessionConfig::new("fake-model"),
            provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
        },
        max_rounds: Some(2),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
    })
    .await;

    let error = match assembled {
        Ok(_) => panic!("两个讨论者共用一个身份必须被拒"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, Error::Discussion(message) if message.contains("两个身份")),
        "got {error:?}"
    );
}

#[tokio::test]
async fn a_discussion_refuses_a_roster_whose_token_budget_disagrees() {
    // 额度是流上每个参与者共用的一份
    // （spec §17），所以两个不同的上限是组装错误，
    // 而不是一场「闸门读谁的数字」的竞赛。
    let dir = tempfile::tempdir().unwrap();
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-discussion"),
            tools: heng::tools::builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
        debaters: vec![
            DebaterParts {
                speaker: kimi(),
                config: budgeted(1_000),
                provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
                soul: None,
            },
            DebaterParts {
                speaker: deepseek(),
                config: budgeted(2_000),
                provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
                soul: None,
            },
        ],
        synthesizer: SynthesizerParts {
            config: budgeted(1_000),
            provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
        },
        max_rounds: Some(2),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
    })
    .await;

    let error = match assembled {
        Ok(_) => panic!("对会话预算意见不一致的名册必须被拒"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, Error::Discussion(message) if message.contains("token 额度")),
        "got {error:?}"
    );
}

// ---------------------------------------------------------------------------
// 独立、揭示，以及真正的并发。
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_first_round_hides_the_other_debater_and_the_targeted_round_reveals_it() {
    let mut fixture = fixture(
        vec![
            answered("KIMI-R1 正文", "复用事件流"),
            answered("KIMI-R2 正文", "复用事件流"),
        ],
        vec![
            answered("DEEPSEEK-R1 正文", "每个 agent 各写一份日志"),
            answered("DEEPSEEK-R2 正文", "复用事件流"),
        ],
        vec![Reply::text("共识：复用事件流")],
        Some(2),
    )
    .await;

    fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    let kimi_rounds = fixture.kimi.requests();
    let deepseek_rounds = fixture.deepseek.requests();
    assert_eq!(kimi_rounds.len(), 2);

    let asked = |request: &heng::provider::ChatRequest, needle: &str| {
        request.messages.iter().any(|message| match message {
            heng::provider::Message::User { content, .. }
            | heng::provider::Message::System { content, .. } => content.contains(needle),
            heng::provider::Message::Assistant { content, .. } => {
                content.as_deref().is_some_and(|text| text.contains(needle))
            }
            heng::provider::Message::Tool { content, .. } => content.contains(needle),
        })
    };

    // 第一轮按构造就是独立的，不是碰巧：KIMI 的第一次请求
    // 看不到 DEEPSEEK 的答案，哪怕 DEEPSEEK 的回合可能
    // 先跑完。
    assert!(asked(&kimi_rounds[0], "该不该复用事件流？"));
    assert!(!asked(&kimi_rounds[0], "DEEPSEEK-R1"));
    assert!(!asked(&deepseek_rounds[0], "KIMI-R1"));

    // 定向轮就是揭示：每一方看到对方的第一轮。
    assert!(asked(&kimi_rounds[1], "DEEPSEEK-R1"));
    assert!(asked(&deepseek_rounds[1], "KIMI-R1"));
}

#[tokio::test]
async fn the_protocol_instruction_is_a_private_identity_and_never_enters_the_stream() {
    let mut fixture = fixture(
        vec![answered("KIMI 正文", "复用事件流")],
        vec![answered("DEEPSEEK 正文", "复用事件流")],
        vec![Reply::text("共识：复用事件流")],
        Some(2),
    )
    .await;

    fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    let kimi_requests = fixture.kimi.requests();
    let identity = debater_identity("kimi");
    // 这条指令以领头的 `system` 消息到达模型……
    match &kimi_requests[0].messages[0] {
        heng::provider::Message::System { content, .. } => assert_eq!(content, &identity),
        other => panic!("要的是领头那条 system 消息，得到 {other:?}"),
    }
    // ……而且从不进流，否则之后某一轮的 `messages` 就没法
    // 只从流上重算（spec §15）。
    assert!(identity.contains("作答规则"));
    let raw = std::fs::read_to_string(&fixture.log_path).unwrap();
    assert!(!raw.contains("作答规则"));
}

#[tokio::test]
async fn the_two_debaters_are_in_flight_at_once() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    // 两方：每个 provider 的 `send` 都等另一方到达。一个
    // 一个接一个跑回合的循环会在这里阻塞，撞上会合的
    // 超时，而不是产出一条流。
    let barrier = Arc::new(Barrier::new(2));
    let mut fixture = fixture_with(
        FakeProvider::meeting_at(
            vec![answered("KIMI 正文", "复用事件流")],
            Arc::clone(&barrier),
        ),
        FakeProvider::meeting_at(
            vec![answered("DEEPSEEK 正文", "复用事件流")],
            Arc::clone(&barrier),
        ),
        FakeProvider::new(vec![Reply::text("共识：复用事件流")]),
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::NoDivergence);
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 1);
}

// ---------------------------------------------------------------------------
// 失败：一方缺席不是共识，而任何失败都不会重跑。
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_failed_side_is_absent_the_discussion_continues_and_the_absence_is_queryable() {
    let mut fixture = fixture(
        vec![answered("KIMI 正文", "复用事件流")],
        vec![Reply::Fail(heng::provider::ProviderError::Transport {
            detail: "stream died".to_owned(),
        })],
        vec![Reply::text("共识：无（只有一方作答）")],
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    // 失败那一方的一次尝试、一条作答、一次收尾调用。
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    assert_eq!(outcome.reason, StopReason::NoDivergence);
    assert_eq!(outcome.rounds, 1);
    assert_eq!(outcome.absent, vec![deepseek()]);

    // 缺席是在流上做的查询，不是某个字段：失败那一方自己的
    // `TurnEnded { Error }` 加上缺失的那条 `MessageCompleted`
    // 就是全部记录（spec §15）。
    let events = read_events(&fixture.log_path).unwrap();
    let attendance = round_attendance(&events, 1);
    assert_eq!(attendance.answers.len(), 1);
    assert_eq!(attendance.absent, vec![deepseek()]);
    assert!(events.iter().any(|event| event.speaker_id == deepseek()
        && matches!(
            event.payload,
            EventPayload::TurnEnded {
                reason: StopReason::Error
            }
        )));

    // 那次危险的误读：告诉合成器缺的是哪一方，而不是
    // 交给它一个答案去读成共识。
    let closing = &fixture.synthesizer.requests()[0].messages;
    let prompt = closing
        .iter()
        .find_map(|message| match message {
            heng::provider::Message::User { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("给了合成器一条 user 消息");
    assert!(prompt.contains("deepseek"));
    assert!(prompt.contains("缺席"));
}

#[tokio::test]
async fn both_sides_failing_ends_the_session_without_a_closing_call() {
    let mut fixture = fixture(
        vec![Reply::Fail(heng::provider::ProviderError::QuotaExhausted {
            detail: "no quota".to_owned(),
        })],
        vec![Reply::Fail(heng::provider::ProviderError::Transport {
            detail: "stream died".to_owned(),
        })],
        vec![Reply::text("这一条不该被用到")],
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Error);
    assert_eq!(outcome.synthesis, "");
    assert_eq!(outcome.rounds, 1);
    assert!(outcome.absent.contains(&kimi()));
    assert!(outcome.absent.contains(&deepseek()));
    // 不重跑，也没有收尾调用：合成是唯一一次不可以跳过的
    // 调用，而在没有东西可合成时它做不出来。
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 0);

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(round_endings(&events), vec![(1, StopReason::Error)]);
    assert!(kinds(&events).contains(&"SessionError"));
}

// ---------------------------------------------------------------------------
// 渲染（spec §19 / Testing Decisions 的渲染那一类）：headless
// 渲染器把最终产物写到 stdout，别的都写到 stderr。
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_discussion_puts_only_the_synthesis_on_stdout() {
    let mut fixture = fixture(
        vec![answered("KIMI 正文不该出现在 stdout", "复用事件流")],
        vec![answered("DEEPSEEK 正文也不该出现", "复用事件流")],
        vec![Reply::text("共识：复用事件流\n分歧：无\n未决：无")],
        Some(2),
    )
    .await;

    fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    // 讨论者的每个回合也都以 `Completed` 收尾，所以「最终产物」在一场讨论里
    // 不可能指「最后那个完成的回合」：收尾调用才是产物。
    let stdout = fixture.stdout.text();
    assert!(stdout.contains("共识：复用事件流"));
    assert!(!stdout.contains("KIMI 正文不该出现在 stdout"));
    assert!(!stdout.contains("DEEPSEEK 正文也不该出现"));
    // 讨论者的那些作答仍然被叙述，叙述在诊断 sink 上。
    assert!(fixture.stderr.text().contains("KIMI 正文不该出现在 stdout"));
}

#[tokio::test]
async fn the_four_round_reasons_render_distinguishably() {
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let (render, receiver) = heng::render::channel();
    let task = Renderer::headless(RenderSinks {
        stdout_result: Box::new(stdout.clone()),
        stderr_diagnostic: Box::new(stderr.clone()),
    })
    .spawn(receiver);

    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::create(dir.path().join("log.jsonl")).unwrap();
    let reasons = [
        StopReason::NoDivergence,
        StopReason::Consensus,
        StopReason::RoundsExhausted,
        StopReason::BudgetExhausted,
    ];
    for reason in reasons {
        let event = log
            .append(
                SpeakerId::System,
                EventPayload::RoundEnded { round: 2, reason },
            )
            .unwrap();
        render.logged(&event);
    }
    drop(render);
    let _ = task.await;

    let rendered = stderr.text();
    // 每条原因都有自己的一段叙述，没有两条读起来一样：短语
    // 来自唯一那个措辞来源，所以这条把接线与
    // 互不雷同一起钉住。
    let expected: Vec<String> = reasons
        .iter()
        .map(|reason| heng::render::wording::round_ended(2, *reason))
        .collect();
    for line in &expected {
        assert!(rendered.contains(line.as_str()), "{rendered} 里缺了 {line}");
    }
    let distinct: std::collections::BTreeSet<&String> = expected.iter().collect();
    assert_eq!(
        distinct.len(),
        reasons.len(),
        "这四条原因不许坍缩成同一种渲染：{rendered}"
    );
    // 一轮结束是叙述，绝不是最终产物。
    assert_eq!(stdout.text(), "");
}

#[tokio::test]
async fn a_one_round_cap_never_opens_a_targeted_round() {
    // 上限是配置，不是常量：只允许一轮时，冲突
    // 就在那里结束辩论，而不是再买一轮。
    let mut fixture = fixture(
        vec![answered("KIMI 正文", "复用事件流")],
        vec![answered("DEEPSEEK 正文", "每个 agent 各写一份日志")],
        vec![Reply::text("共识：无\n分歧：日志归属")],
        Some(1),
    )
    .await;

    let outcome = fixture.harness.discuss("日志该怎么放？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::RoundsExhausted);
    assert_eq!(outcome.rounds, 1);
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::RoundsExhausted), (2, StopReason::Completed)]
    );
}

#[tokio::test]
async fn omitting_the_round_cap_takes_the_protocol_default() {
    // 「上限 2 轮（可配）」：上限有默认值，接缝也能点名它。
    let mut fixture = fixture(
        vec![
            answered("KIMI 第一轮", "复用事件流"),
            answered("KIMI 第二轮", "复用事件流"),
        ],
        vec![
            answered("DEEPSEEK 第一轮", "每个 agent 各写一份日志"),
            answered("DEEPSEEK 第二轮", "每个 agent 各写一份日志"),
        ],
        vec![Reply::text("共识：无\n分歧：日志归属")],
        None,
    )
    .await;

    let outcome = fixture.harness.discuss("日志该怎么放？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(outcome.rounds, 2);
    assert_eq!(outcome.reason, StopReason::RoundsExhausted);
}

#[tokio::test]
async fn a_debater_dispatches_an_executor_and_only_its_summary_reaches_the_discussion() {
    // `task` 的全部要点（spec §16）：讨论者可以让真实的工作被做完，
    // 而另一个讨论者看到的是摘要 —— 绝不是执行者的过程。
    let delegated = Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: "call-1".to_owned(),
            name: "task".to_owned(),
            arguments: serde_json::json!({"brief": "count the modules under src"}).to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ]);
    let mut fixture = fixture(
        vec![
            delegated,
            Reply::text("EXECUTOR-ONLY: 12 modules"),
            answered("KIMI 第一轮正文（我派了执行者去数）", "复用事件流"),
            answered("KIMI 第二轮正文", "复用事件流"),
        ],
        vec![
            answered("DEEPSEEK 第一轮正文", "每个 agent 各写一份日志"),
            answered("DEEPSEEK 第二轮正文", "复用事件流"),
        ],
        vec![Reply::text("共识：复用事件流")],
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("日志该怎么放？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Consensus);
    assert_eq!(outcome.rounds, 2);

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::ExecutorFinished { .. }))
            .count(),
        1
    );
    // 派发的那个讨论者以自己的 `task` 结果拿到摘要。
    let result = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: true,
                output: Some(output),
                ..
            } => Some(output.clone()),
            _ => None,
        })
        .expect("task 调用的结果");
    assert!(result.contains("EXECUTOR-ONLY: 12 modules"), "{result}");

    // 执行者的过程进不了两个讨论者的窗口；另一个讨论者
    // 至多看到 `task` 调用自身那一行工具摘要。
    let kimi_round_two = &fixture.kimi.requests()[3];
    let summary = kimi_round_two
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Tool { content, .. } if content.contains("EXECUTOR-ONLY") => Some(content),
            _ => None,
        })
        .expect("派发者自己的工具结果在它的窗口里");
    assert!(summary.contains("改动文件：无"), "{summary}");

    // 执行者的过程以任何形态都进不了另一个讨论者的窗口
    // —— 既不是发言（一块 `executor:kimi-1`），也不是正文。
    // 另一个讨论者只通过 KIMI 自己的话知道有这件工作。
    for request in fixture.deepseek.requests() {
        for message in &request.messages {
            let content = message_content(message);
            assert!(
                !content.contains("EXECUTOR-ONLY"),
                "执行者的过程漏进了另一个讨论者的窗口：{content}"
            );
            assert!(
                !content.contains("executor:kimi-1"),
                "执行者的事件漏进了另一个讨论者的窗口：{content}"
            );
        }
    }

    // 派发者自己的窗口恰好带着摘要一次，而且是以 `task`
    // 调用的工具结果的形式 —— 绝不是一次投影进来的执行者回合。
    let dispatcher = &fixture.kimi.requests()[3];
    assert!(dispatcher.messages.iter().any(
        |message| matches!(message, Message::Tool { content, .. } if content.contains("EXECUTOR-ONLY"))
    ));
    for message in &dispatcher.messages {
        if let Message::User { content, .. } = message {
            assert!(
                !content.contains("EXECUTOR-ONLY") && !content.contains("executor:kimi-1"),
                "执行者不是讨论者窗口里的一个发言者：{content}"
            );
        }
    }
    // 而第一轮那份在执行者存在之前取下的投影，
    // 以上一切都没碰到它。
    for message in &fixture.kimi.requests()[0].messages {
        assert!(!message_content(message).contains("EXECUTOR-ONLY"));
    }
    // 另一个讨论者的第二轮存在（这一轮是定向的），而它对执行者的
    // 了解不超过 KIMI 说过的话。
    assert_eq!(fixture.deepseek.requests().len(), 2);
}

/// 一条线上消息的文本，不论它是什么形状。
fn message_content(message: &Message) -> &str {
    match message {
        Message::System { content, .. }
        | Message::User { content, .. }
        | Message::Tool { content, .. } => content,
        Message::Assistant { content, .. } => content.as_deref().unwrap_or_default(),
    }
}

/// 一个仅用量就撑爆预算的答案：600 输入 token、没有输出。
fn answered_expensive(body: &str, conclusion: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::TextDelta(answer(body, conclusion)),
        StreamEvent::Usage(Usage {
            input_tokens: 600,
            output_tokens: 0,
            cached_tokens: 0,
            miss_tokens: 600,
            reasoning_tokens: None,
        }),
        StreamEvent::Finished {
            finish_reason: FinishReason::Stop,
        },
    ])
}

/// 测试讨论里每个参与者分到的会话额度。
fn budgeted(tokens: u64) -> SessionConfig {
    SessionConfig::new("fake-model").with_session_token_limit(tokens)
}

#[tokio::test]
async fn an_exhausted_session_opens_no_second_round_and_goes_straight_to_synthesis() {
    // 讨论者的第一批答案很贵，而它们分歧了，所以协议
    // 自己的计划会买来一个定向的第二轮。到那时额度
    // 已经花完，所以硬停拿掉了那一轮，讨论降级成
    // 它唯一不可跳过的那个调用（spec §17）。
    let config = budgeted(1_000);
    let mut fixture = fixture_with_configs(
        FakeProvider::new(vec![answered_expensive("KIMI 正文", "复用事件流")]),
        FakeProvider::new(vec![answered_expensive(
            "DEEPSEEK 正文",
            "每个 agent 各写一份日志",
        )]),
        FakeProvider::new(vec![Reply::text("共识：无\n分歧：日志归属\n未决：无")]),
        Some(2),
        [config.clone(), config.clone(), config],
    )
    .await;

    let outcome = fixture.harness.discuss("日志该怎么放？").await.unwrap();
    fixture.harness.shutdown().await;

    // 各一次调用，而收尾那一次仍然发生了。
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    assert_eq!(outcome.reason, StopReason::BudgetExhausted);
    assert_eq!(outcome.rounds, 1);
    assert_eq!(outcome.synthesis, "共识：无\n分歧：日志归属\n未决：无");

    let events = read_events(&fixture.log_path).unwrap();
    assert!(
        !events.iter().any(|event| matches!(
            &event.payload,
            EventPayload::RoundStarted {
                round: 2,
                mode: RoundMode::Targeted
            }
        )),
        "硬停不许开启定向轮"
    );
    // 被预算关掉的那一轮会说出来，而合成轮仍然
    // 以 `Completed` 收尾：收尾调用不是预算的伤亡。
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::BudgetExhausted), (2, StopReason::Completed)]
    );
    // 这条原因在线上、在人读的叙述里，都与协议自己那三条
    // 区分得开。
    let rendered = fixture.stderr.text();
    assert!(
        rendered.contains(&heng::render::wording::round_ended(
            1,
            StopReason::BudgetExhausted
        )),
        "{rendered}"
    );
    assert!(
        !rendered.contains(&heng::render::wording::round_ended(
            1,
            StopReason::RoundsExhausted
        )),
        "{rendered}"
    );
}

#[tokio::test]
async fn the_synthesizer_is_the_one_call_the_budget_cannot_skip() {
    // 额度为零会在第一次 provider 调用之前就停下，于是没有讨论者
    // 说过话。收尾调用仍然做：降级到合成**之后**就会
    // 把整场讨论丢掉（spec §17）。
    let config = budgeted(0);
    let mut fixture = fixture_with_configs(
        FakeProvider::new(vec![]),
        FakeProvider::new(vec![]),
        FakeProvider::new(vec![Reply::text("没有作答可合成")]),
        Some(2),
        [config.clone(), config.clone(), config],
    )
    .await;

    let outcome = fixture.harness.discuss("还讨论吗？").await.unwrap();
    fixture.harness.shutdown().await;

    assert!(fixture.kimi.requests().is_empty());
    assert!(fixture.deepseek.requests().is_empty());
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    assert_eq!(outcome.reason, StopReason::BudgetExhausted);
    assert_eq!(outcome.rounds, 0);

    let events = read_events(&fixture.log_path).unwrap();
    // 没有开过辩论轮，所以没有轮的边界要关：唯一那条
    // `RoundEnded` 属于合成轮。
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::Completed)],
        "没跑过任何一轮的辩论阶段不关任何一轮"
    );
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventPayload::RoundStarted {
            mode: RoundMode::Independent,
            ..
        }
    )));
}

#[tokio::test]
async fn routing_a_cheap_synthesizer_moves_neither_debater() {
    // 更便宜的模型可以被改派到的两个落点是
    // 合成器与执行者（spec §17）。改派第一个不许碰到
    // 讨论者：异构是协议最强的杠杆。
    let synthesizer_config =
        SessionConfig::new("fake-model").with_synthesizer_model("cheap-synthesizer");
    let mut fixture = fixture_with_configs(
        FakeProvider::new(vec![answered("KIMI 正文", "复用事件流")]),
        FakeProvider::new(vec![answered("DEEPSEEK 正文", "复用事件流")]),
        FakeProvider::new(vec![Reply::text("共识：复用事件流")]),
        Some(2),
        [
            SessionConfig::new("fake-model"),
            SessionConfig::new("fake-model"),
            synthesizer_config,
        ],
    )
    .await;

    fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    assert_eq!(fixture.synthesizer.requests()[0].model, "cheap-synthesizer");
    for request in fixture.kimi.requests() {
        assert_eq!(request.model, "fake-model");
    }
    for request in fixture.deepseek.requests() {
        assert_eq!(request.model, "fake-model");
    }
}

// ---------------------------------------------------------------------------
// 在活着的会话上跑一场讨论（`Harness::discuss`，即 `/discuss` 命令）
// ---------------------------------------------------------------------------

/// 一条临时日志上的单 agent 会话，配一个脚本化的 provider —— 一次
/// `/discuss` 跑在里面的那个 harness。
struct SessionFixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn session_fixture(replies: Vec<Reply>) -> SessionFixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    std::fs::create_dir_all(&session).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::with_caps(replies, caps_for("deepseek-flash").unwrap());
    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-live"),
            tools: heng::tools::builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();
    SessionFixture {
        harness: Some(harness),
        log_path,
        _dir: dir,
    }
}

impl SessionFixture {
    fn harness(&mut self) -> &mut Harness {
        self.harness.as_mut().expect("harness 已经关掉了")
    }

    /// 两个讨论者加一个合成器，各带自己那个脚本化的 provider。
    fn parts(
        &self,
        first: Vec<Reply>,
        second: Vec<Reply>,
        synthesizer: Vec<Reply>,
    ) -> (Vec<DebaterParts>, SynthesizerParts) {
        (
            vec![
                DebaterParts {
                    speaker: SpeakerId::Debater("kimi-k3".into()),
                    config: SessionConfig::new("fake-model"),
                    provider: Box::new(FakeProvider::with_caps(
                        first,
                        caps_for("deepseek-flash").unwrap(),
                    )),
                    soul: None,
                },
                DebaterParts {
                    speaker: SpeakerId::Debater("kimi-k3#2".into()),
                    config: SessionConfig::new("fake-model"),
                    provider: Box::new(FakeProvider::with_caps(
                        second,
                        caps_for("deepseek-flash").unwrap(),
                    )),
                    soul: None,
                },
            ],
            SynthesizerParts {
                config: SessionConfig::new("fake-model"),
                provider: Box::new(FakeProvider::with_caps(
                    synthesizer,
                    caps_for("deepseek-flash").unwrap(),
                )),
            },
        )
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    async fn shutdown(&mut self) {
        if let Some(harness) = self.harness.take() {
            harness.shutdown().await;
        }
    }
}

fn round_starts_in(events: &[Event], mode: RoundMode) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::RoundStarted {
                round,
                mode: started,
            } if *started == mode => Some(*round),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_discussion_on_a_live_session_inherits_its_context() {
    // `/discuss` 的要点：讨论者是用户所在那个会话的兄弟，
    // 所以它们的投影把*它的*回合变成 `user` 消息 —— 它们争论的是
    // 这个会话关于什么，而不是真空里的一个问题（spec §5、§15）。
    let mut fixture = session_fixture(vec![Reply::text("应该复用。"), Reply::text("继续。")]).await;
    fixture
        .harness()
        .run_turn("我们该不该复用事件流？")
        .await
        .unwrap();

    // 两侧都得出同一个结论，所以这场讨论只有一轮：轮次的
    // 切分、流，以及合成器拿到的材料才是这条测试关心的。
    let first = FakeProvider::with_caps(
        vec![Reply::text(&answer("甲的看法", "复用"))],
        caps_for("deepseek-flash").unwrap(),
    );
    let second = FakeProvider::with_caps(
        vec![Reply::text(&answer("乙的看法", "复用"))],
        caps_for("deepseek-flash").unwrap(),
    );
    let synthesizer = FakeProvider::with_caps(
        vec![Reply::text("共识：复用。")],
        caps_for("deepseek-flash").unwrap(),
    );
    let (mut debaters, synthesizer_parts) = fixture.parts(vec![], vec![], vec![]);
    // 换成这条测试能读回来的 provider。
    debaters[0].provider = Box::new(first.clone());
    debaters[1].provider = Box::new(second.clone());
    let synthesizer_parts = SynthesizerParts {
        config: synthesizer_parts.config,
        provider: Box::new(synthesizer.clone()),
    };

    let outcome = fixture
        .harness()
        .discuss("换个角度再看一次", debaters, synthesizer_parts, None)
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::NoDivergence);
    assert_eq!(outcome.rounds, 1);

    // 第一个讨论者实际被发到了什么：会话自己的历史，作为 `user`。
    let first_request = &first.requests()[0].messages;
    assert!(
        first_request.iter().any(|message| matches!(
            message,
            Message::User { content, .. } if content.contains("我们该不该复用事件流？")
        )),
        "会话的问题到达了讨论者：{first_request:?}"
    );
    assert!(
        first_request.iter().any(|message| matches!(
            message,
            Message::User { content, .. } if content.contains("应该复用。")
        )),
        "而这个会话已经给过的答案也一样，作为另一个发言者的话：\
         {first_request:?}"
    );

    // 一条流：会话自己的回合，然后是讨论的那些轮。
    let events = fixture.events();
    assert_eq!(
        round_starts_in(&events, RoundMode::Independent),
        vec![1],
        "这条流上的第一场讨论从一轮开始编号"
    );
    assert_eq!(round_starts_in(&events, RoundMode::Synthesis), vec![2]);

    // 而之后会话仍然可用：讨论是追加到它的流上，
    // 而不是接管它。
    fixture
        .harness()
        .run_turn("讨论完了，继续。")
        .await
        .unwrap();
    assert!(fixture.events().iter().any(|event| matches!(
        &event.payload,
        EventPayload::MessageCompleted { role: Role::Assistant, text, .. }
            if text.contains("应该复用。")
    )));
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_second_discussion_on_one_session_numbers_after_the_first_and_synthesizes_only_itself() {
    // 一个会话里跑两场讨论，正是 `/discuss` 让它成为可能的，所以
    // 轮次号必须在流上保持唯一 —— 而第二个合成器也不许
    // 被交给第一场讨论的答案去一起合成。
    let mut fixture = session_fixture(vec![Reply::text("开场。")]).await;
    fixture.harness().run_turn("第一个问题").await.unwrap();

    let (debaters, synthesizer) = fixture.parts(
        vec![Reply::text(&answer("第一场的甲", "甲结论"))],
        vec![Reply::text(&answer("第一场的乙", "甲结论"))],
        vec![Reply::text("第一场的合成")],
    );
    fixture
        .harness()
        .discuss("第一个讨论", debaters, synthesizer, None)
        .await
        .unwrap();

    // 第二场讨论用全新的 provider 跑，所以它的请求可以
    // 单独读回来。
    let second_debaters = FakeProvider::with_caps(
        vec![Reply::text(&answer("第二场的甲", "甲结论"))],
        caps_for("deepseek-flash").unwrap(),
    );
    let second_other = FakeProvider::with_caps(
        vec![Reply::text(&answer("第二场的乙", "甲结论"))],
        caps_for("deepseek-flash").unwrap(),
    );
    let second_synthesizer = FakeProvider::with_caps(
        vec![Reply::text("第二场的合成")],
        caps_for("deepseek-flash").unwrap(),
    );
    let (mut debaters, mut synthesizer) = fixture.parts(vec![], vec![], vec![]);
    debaters[0].provider = Box::new(second_debaters.clone());
    debaters[1].provider = Box::new(second_other.clone());
    synthesizer.provider = Box::new(second_synthesizer.clone());
    fixture
        .harness()
        .discuss("第二个讨论", debaters, synthesizer, None)
        .await
        .unwrap();
    fixture.shutdown().await;

    let events = fixture.events();
    assert_eq!(
        round_starts_in(&events, RoundMode::Independent),
        vec![1, 3],
        "第二场讨论接着第一场的轮次编号"
    );
    assert_eq!(
        round_starts_in(&events, RoundMode::Synthesis),
        vec![2, 4],
        "它的合成也一样"
    );

    // 第二个合成器的提示词只装着第二场讨论。
    let prompt = second_synthesizer
        .requests()
        .last()
        .expect("合成器被调用过")
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("一条 user 消息");
    assert!(prompt.contains("第二场的甲"), "{prompt}");
    assert!(prompt.contains("第二个讨论"), "{prompt}");
    assert!(
        !prompt.contains("第一场的甲") && !prompt.contains("第一个讨论"),
        "第一场讨论不是第二场合成的材料：{prompt}"
    );
    // 而重放出来的提示词就是当时发出去的那一份：整套
    // 事件流设计所依靠的那条不变量，现在一条流上有两场讨论。
    let replayed = heng::agent::replay::replay(
        &events,
        &SpeakerId::System,
        Some(4),
        &caps_for("deepseek-flash").unwrap(),
    )
    .unwrap();
    let replayed_prompt = replayed
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User { content, .. } => Some(content.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(replayed_prompt, prompt, "重放 == 当时发出去的东西");
}

#[tokio::test]
async fn a_persona_reaches_its_own_side_only_and_the_call_stays_recomputable() {
    // 灵魂是一个讨论者身上唯一*不能*从名字推出来的东西，
    // 所以它落在流上：记成一次归属给那个讨论者的注入、
    // 对它私有，因此也是 `sessions replay` 会重算的一部分（spec §5、
    // §15）。
    let mut fixture = session_fixture(vec![Reply::text("开场。")]).await;
    fixture.harness().run_turn("第一个问题").await.unwrap();

    let first = FakeProvider::with_caps(
        vec![Reply::text(&answer("张三的作答", "甲结论"))],
        caps_for("deepseek-flash").unwrap(),
    );
    let second = FakeProvider::with_caps(
        vec![Reply::text(&answer("李四的作答", "甲结论"))],
        caps_for("deepseek-flash").unwrap(),
    );
    let synthesizer = FakeProvider::with_caps(
        vec![Reply::text("合成")],
        caps_for("deepseek-flash").unwrap(),
    );
    let (mut debaters, mut synthesizer_parts) = fixture.parts(vec![], vec![], vec![]);
    debaters[0].speaker = SpeakerId::Debater("张三".into());
    debaters[0].soul = Some("法外狂徒，思路不受限制".to_owned());
    debaters[1].speaker = SpeakerId::Debater("李四".into());
    debaters[1].soul = Some("守法好公民".to_owned());
    debaters[0].provider = Box::new(first.clone());
    debaters[1].provider = Box::new(second.clone());
    synthesizer_parts.provider = Box::new(synthesizer.clone());

    fixture
        .harness()
        .discuss("该不该复用它？", debaters, synthesizer_parts, None)
        .await
        .unwrap();

    // 每一方被告知自己的性格，而不是对方的。
    let sent = |provider: &FakeProvider| {
        provider.requests()[0]
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::User { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let zhang = sent(&first);
    assert!(zhang.contains("法外狂徒"), "{zhang}");
    assert!(!zhang.contains("守法好公民"), "{zhang}");
    let li = sent(&second);
    assert!(li.contains("守法好公民"), "{li}");
    assert!(!li.contains("法外狂徒"), "{li}");

    // 它在流上，归属给它所描述的那个讨论者。
    let events = fixture.events();
    let personas: Vec<(String, String)> = events
        .iter()
        .filter_map(|event| match (&event.payload, &event.speaker_id) {
            (
                EventPayload::ContextInjected {
                    source: ContextSource::Persona(name),
                    content,
                },
                SpeakerId::Debater(speaker),
            ) if name.as_str() == speaker.as_str() => Some((name.to_string(), content.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(personas.len(), 2, "每个讨论者一次注入：{personas:?}");
    assert!(personas[0].1.contains("法外狂徒"), "{personas:?}");

    // 而因为它在流上，那次调用的重放就是当时发出去的东西。
    let replayed = heng::agent::replay::replay(
        &events,
        &SpeakerId::Debater("张三".into()),
        Some(1),
        &caps_for("deepseek-flash").unwrap(),
    )
    .unwrap();
    assert_eq!(
        replayed.len(),
        first.requests()[0].messages.len(),
        "形状一样：{replayed:?}"
    );
    let replayed_text = replayed
        .iter()
        .filter_map(|message| match message {
            Message::User { content, .. } => Some(content.clone()),
            Message::System { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(replayed_text.contains("法外狂徒"), "{replayed_text}");
    assert!(replayed_text.contains("该不该复用它？"), "{replayed_text}");
    fixture.shutdown().await;
}
