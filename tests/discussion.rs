//! The discussion protocol (spec §15).
//!
//! Two seams are exercised here. The pure comparison functions are tested
//! directly — the spec's Testing Decisions name "the equality / substring
//! verdict on normalized conclusions" as one of the pure functions that needs no
//! mock seam. The round loop itself is tested through the library assembly entry
//! with a scripted fake provider, asserting the JSONL event stream and the two
//! render sinks.

mod support;

use std::path::PathBuf;

use fs_agent::config::SessionConfig;
use fs_agent::discussion::protocol::{
    answers_agree, conclusion_of, normalize, round_attendance, round_outcome, RoundOutcome,
    CONCLUSION_MARKER,
};
use fs_agent::discussion::{
    debater_identity, pick_pair, plan_after_round, synthesis_prompt, RoundPlan,
};
use fs_agent::events::{
    read_events, ContextSource, Event, EventLog, EventPayload, Role, RoundMode, SessionId,
    SpeakerId, StopReason, Usage, SCHEMA_VERSION,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::capability::caps_for;
use fs_agent::provider::{FinishReason, Message, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{
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

/// Build a log from a script, keeping the temp directory alive with it.
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

/// An answer as a debater writes it: prose, then the marker line.
fn answer(body: &str, conclusion: &str) -> String {
    format!("{body}\nCONCLUSION: {conclusion}")
}

#[test]
fn a_pair_is_drawn_from_the_pool_without_repeating_a_member() {
    // The draw is a pure function of the pool size and a seed, so a discussion can say
    // which pair it ran and a test can pin the choice instead of the distribution.
    assert_eq!(pick_pair(2, 0), Some((0, 1)));
    assert_eq!(
        pick_pair(2, 12_345),
        Some((0, 1)),
        "a pool of two debates as itself whatever the seed"
    );
    assert_eq!(pick_pair(1, 7), None, "one member cannot hold a discussion");
    assert_eq!(pick_pair(0, 7), None);

    // A pool of three: every draw is two distinct members, in pool order, and the seed
    // really moves the pair (all three pairs come up over a modest range).
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..40u64 {
        let (first, second) = pick_pair(3, seed).expect("a pair");
        assert!(first < second, "seed {seed}: {first} then {second}");
        assert!(second < 3, "seed {seed}: {second} is out of the pool");
        seen.insert((first, second));
    }
    assert_eq!(
        seen,
        std::collections::BTreeSet::from([(0, 1), (0, 2), (1, 2)]),
        "a pool of three can produce every pair"
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
    // A marker that is not at the start of its line is prose about the marker,
    // not a conclusion.
    assert_eq!(conclusion_of("我的 CONCLUSION: 藏在句子中间"), None);
    // An empty conclusion is not a conclusion.
    assert_eq!(conclusion_of("正文\nCONCLUSION:   "), None);
}

#[test]
fn a_decorated_marker_still_reads_as_a_conclusion() {
    // The model may wrap the marker in emphasis or a list bullet. Treating that
    // as "no conclusion" would be a false negative: the two sides would look
    // divergent and the protocol would buy a second round for nothing.
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
    // The spec's mechanical rule: exact equality **or** substring containment,
    // normalized. Containment is checked both ways, so the longer answer's extra
    // qualifier does not read as a disagreement.
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
    // This is the dangerous misread the spec calls out: one answer (or none)
    // must not collapse into "they agree".
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
    // The spec's absence query is "ended in `Error` **and** left no message":
    // an answer that landed before the failure is still an answer, and reading
    // it as absent would throw away a real position.
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
    // An executor's turn lands inside the round that spawned it on the same
    // stream (spec §16), and the synthesizer's product is `System` (spec §2).
    // Neither is a debater's answer, so neither may be counted as one — or the
    // protocol would find agreement between a debater and its own executor.
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
    // One round, nobody disagreed: the protocol never needed a second round, and
    // the reason says exactly that.
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
    // Nothing was compared, so nothing was agreed. The round still ends — with
    // the reason that claims the least — and the absent side stays visible as its
    // own `TurnEnded { Error }` rather than as a field on this event.
    assert_eq!(
        plan_after_round(RoundOutcome::Incomplete, 1, 2),
        RoundPlan::Stop(StopReason::NoDivergence)
    );
}

#[test]
fn the_debater_identity_teaches_the_marker_the_parser_reads() {
    // Prompt and parser share one constant; a marker the instruction teaches and
    // the parser does not know would make every round look divergent.
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
    // The dangerous misread: only one answer came back, so the synthesizer must
    // be told the other side is absent instead of reading the single answer as
    // the consensus.
    assert!(prompt.contains("deepseek"));
    assert!(prompt.contains("缺席"));
    // Speech is revealed; private reasoning never is.
    assert!(!prompt.contains("private reasoning"));
}

// ---------------------------------------------------------------------------
// The round loop, through the assembly seam.
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

/// The same, with a configuration per participant: one debater, the other, the
/// synthesizer. A discussion shares its token allowance (spec §17), so the
/// configurations differ only where a test is about routing.
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
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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

/// An answer as a debater writes it.
fn answered(body: &str, conclusion: &str) -> Reply {
    Reply::text(&answer(body, conclusion))
}

fn kinds(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(|event| event.payload.kind()).collect()
}

/// Every `RoundEnded` as `(round, reason)`, in order.
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

    // Two debaters and one closing call. The mechanical verdict costs nothing.
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
    // The synthesizer's product is the harness's own voice (spec §2), and the
    // question is the user's.
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
    // Only the round that ended the debate carries `RoundEnded`; the second
    // round's boundary is the synthesis round's `RoundStarted`.
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
    // N = 2 is not an implementation detail: the "N = 2 does not arbitrate"
    // decision is what makes the mechanical verdict enough, so a third debater
    // has to reopen that decision rather than slip in (spec §15, Out of Scope).
    let dir = tempfile::tempdir().unwrap();
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-discussion"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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
        Ok(_) => panic!("a one-debater roster must be refused"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Discussion(_)), "got {error:?}");
}

#[tokio::test]
async fn a_discussion_refuses_two_debaters_that_share_one_identity() {
    // Two debaters may be the same *model* — one subscription is not a reason to have
    // no discussion — but they may not be the same *participant*: every projection is
    // a function of `speaker_id`, so one name would hand each side the other's answer
    // as its own in the targeted round (spec §5).
    let dir = tempfile::tempdir().unwrap();
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-discussion"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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
        Ok(_) => panic!("two debaters with one identity must be refused"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, Error::Discussion(message) if message.contains("two identities")),
        "got {error:?}"
    );
}

#[tokio::test]
async fn a_discussion_refuses_a_roster_whose_token_budget_disagrees() {
    // The allowance is one value shared by every participant on the stream
    // (spec §17), so two different caps are a setup error rather than a race
    // about whose number the gate reads.
    let dir = tempfile::tempdir().unwrap();
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-discussion"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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
        Ok(_) => panic!("a roster that disagrees about the session budget must be refused"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, Error::Discussion(message) if message.contains("token budget")),
        "got {error:?}"
    );
}

// ---------------------------------------------------------------------------
// Independence, the reveal, and real concurrency.
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

    let asked = |request: &fs_agent::provider::ChatRequest, needle: &str| {
        request.messages.iter().any(|message| match message {
            fs_agent::provider::Message::User { content, .. }
            | fs_agent::provider::Message::System { content, .. } => content.contains(needle),
            fs_agent::provider::Message::Assistant { content, .. } => {
                content.as_deref().is_some_and(|text| text.contains(needle))
            }
            fs_agent::provider::Message::Tool { content, .. } => content.contains(needle),
        })
    };

    // Round one is independent by construction, not by luck: KIMI's first
    // request cannot see DEEPSEEK's answer even though DEEPSEEK's turn may have
    // finished first.
    assert!(asked(&kimi_rounds[0], "该不该复用事件流？"));
    assert!(!asked(&kimi_rounds[0], "DEEPSEEK-R1"));
    assert!(!asked(&deepseek_rounds[0], "KIMI-R1"));

    // The targeted round is the reveal: each side sees the other's first round.
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
    // The instruction reaches the model as the leading `system` message...
    match &kimi_requests[0].messages[0] {
        fs_agent::provider::Message::System { content, .. } => assert_eq!(content, &identity),
        other => panic!("expected a leading system message, got {other:?}"),
    }
    // ...and never enters the stream, or a later round's `messages` could not be
    // recomputed from the stream alone (spec §15).
    assert!(identity.contains("作答规则"));
    let raw = std::fs::read_to_string(&fixture.log_path).unwrap();
    assert!(!raw.contains("作答规则"));
}

#[tokio::test]
async fn the_two_debaters_are_in_flight_at_once() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    // Two parties: each provider's `send` waits for the other to arrive. A loop
    // that ran the turns one after another would block here and trip the
    // rendezvous timeout instead of producing a stream.
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
// Failure: one side absent is not consensus, and no failure is ever re-run.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_failed_side_is_absent_the_discussion_continues_and_the_absence_is_queryable() {
    let mut fixture = fixture(
        vec![answered("KIMI 正文", "复用事件流")],
        vec![Reply::Fail(fs_agent::provider::ProviderError::Transport {
            detail: "stream died".to_owned(),
        })],
        vec![Reply::text("共识：无（只有一方作答）")],
        Some(2),
    )
    .await;

    let outcome = fixture.harness.discuss("该不该复用事件流？").await.unwrap();
    fixture.harness.shutdown().await;

    // One attempt by the failed side, one answer, one closing call.
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 1);
    assert_eq!(outcome.reason, StopReason::NoDivergence);
    assert_eq!(outcome.rounds, 1);
    assert_eq!(outcome.absent, vec![deepseek()]);

    // The absence is a query over the stream, not a field: the failed side's own
    // `TurnEnded { Error }` plus the missing `MessageCompleted` is the whole
    // record (spec §15).
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

    // The dangerous misread: the synthesizer is told which side is missing rather
    // than being handed one answer to read as the consensus.
    let closing = &fixture.synthesizer.requests()[0].messages;
    let prompt = closing
        .iter()
        .find_map(|message| match message {
            fs_agent::provider::Message::User { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("the synthesizer is given a user message");
    assert!(prompt.contains("deepseek"));
    assert!(prompt.contains("缺席"));
}

#[tokio::test]
async fn both_sides_failing_ends_the_session_without_a_closing_call() {
    let mut fixture = fixture(
        vec![Reply::Fail(
            fs_agent::provider::ProviderError::QuotaExhausted {
                detail: "no quota".to_owned(),
            },
        )],
        vec![Reply::Fail(fs_agent::provider::ProviderError::Transport {
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
    // No re-run, and no closing call: the synthesis is the one call that cannot
    // be skipped, and it cannot be made when there is nothing to synthesize.
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 1);
    assert_eq!(fixture.synthesizer.requests().len(), 0);

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(round_endings(&events), vec![(1, StopReason::Error)]);
    assert!(kinds(&events).contains(&"SessionError"));
}

// ---------------------------------------------------------------------------
// Rendering (spec §19 / Testing Decisions' render category): the headless
// renderer writes the final product to stdout and everything else to stderr.
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

    // Every debater turn ends `Completed` too, so "final product" cannot mean
    // "the last completed turn" in a discussion: the closing call is the product.
    let stdout = fixture.stdout.text();
    assert!(stdout.contains("共识：复用事件流"));
    assert!(!stdout.contains("KIMI 正文不该出现在 stdout"));
    assert!(!stdout.contains("DEEPSEEK 正文也不该出现"));
    // The debaters' answers are still narrated, on the diagnostic sink.
    assert!(fixture.stderr.text().contains("KIMI 正文不该出现在 stdout"));
}

#[tokio::test]
async fn the_four_round_reasons_render_distinguishably() {
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let (render, receiver) = fs_agent::render::channel();
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
    // Each reason gets its own narration, and no two read the same: the phrase
    // comes from the one wording source, so this pins the wiring and the
    // distinctness together.
    let expected: Vec<String> = reasons
        .iter()
        .map(|reason| fs_agent::render::wording::round_ended(2, *reason))
        .collect();
    for line in &expected {
        assert!(
            rendered.contains(line.as_str()),
            "{line} missing from {rendered}"
        );
    }
    let distinct: std::collections::BTreeSet<&String> = expected.iter().collect();
    assert_eq!(
        distinct.len(),
        reasons.len(),
        "the four reasons must not collapse into one rendering: {rendered}"
    );
    // A round ending is narration, never a final product.
    assert_eq!(stdout.text(), "");
}

#[tokio::test]
async fn a_one_round_cap_never_opens_a_targeted_round() {
    // The cap is configuration, not a constant: with one round allowed, a
    // conflict ends the debate there instead of buying a second round.
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
    // "上限 2 轮（可配）": the cap has a default, and the seam can also name it.
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
    // The whole point of `task` (spec §16): a debater can have real work done,
    // and the other debater sees a summary — never the executor's process.
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
    // The dispatching debater gets the summary as its own `task` result.
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
        .expect("the task call's result");
    assert!(result.contains("EXECUTOR-ONLY: 12 modules"), "{result}");

    // The executor's process reaches neither debater's window; the other debater
    // sees at most the one-line tool summary of the `task` call itself.
    let kimi_round_two = &fixture.kimi.requests()[3];
    let summary = kimi_round_two
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Tool { content, .. } if content.contains("EXECUTOR-ONLY") => Some(content),
            _ => None,
        })
        .expect("the dispatcher's own tool result is in its window");
    assert!(summary.contains("files changed: none"), "{summary}");

    // The executor's process reaches the other debater's window in no form at
    // all — not as speech (an `executor:kimi-1` block) and not as a body. The
    // other debater learns of the work only through KIMI's own words.
    for request in fixture.deepseek.requests() {
        for message in &request.messages {
            let content = message_content(message);
            assert!(
                !content.contains("EXECUTOR-ONLY"),
                "an executor's process leaked into the other debater's window: {content}"
            );
            assert!(
                !content.contains("executor:kimi-1"),
                "an executor's events leaked into the other debater's window: {content}"
            );
        }
    }

    // The dispatcher's own window carries the summary exactly once, and as the
    // `task` call's tool result — never as an executor turn projected into it.
    let dispatcher = &fixture.kimi.requests()[3];
    assert!(dispatcher.messages.iter().any(
        |message| matches!(message, Message::Tool { content, .. } if content.contains("EXECUTOR-ONLY"))
    ));
    for message in &dispatcher.messages {
        if let Message::User { content, .. } = message {
            assert!(
                !content.contains("EXECUTOR-ONLY") && !content.contains("executor:kimi-1"),
                "an executor is not a speaker in a debater's window: {content}"
            );
        }
    }
    // And the first round's projection, taken before the executor existed, is
    // untouched by any of it.
    for message in &fixture.kimi.requests()[0].messages {
        assert!(!message_content(message).contains("EXECUTOR-ONLY"));
    }
    // The other debater's round two exists (the round was targeted), and knows
    // nothing of the executor beyond what KIMI said.
    assert_eq!(fixture.deepseek.requests().len(), 2);
}

/// The text of one wire message, whichever shape it has.
fn message_content(message: &Message) -> &str {
    match message {
        Message::System { content, .. }
        | Message::User { content, .. }
        | Message::Tool { content, .. } => content,
        Message::Assistant { content, .. } => content.as_deref().unwrap_or_default(),
    }
}

/// An answer whose usage alone blows a budget: 600 input tokens, no output.
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

/// The session allowance every participant of a test discussion is given.
fn budgeted(tokens: u64) -> SessionConfig {
    SessionConfig::new("fake-model").with_session_token_limit(tokens)
}

#[tokio::test]
async fn an_exhausted_session_opens_no_second_round_and_goes_straight_to_synthesis() {
    // The debaters' first answers are expensive and they diverge, so the
    // protocol's own plan would buy a targeted second round. The allowance is
    // already spent by then, so the hard stop takes that round away and the
    // discussion degrades into the one call it may not skip (spec §17).
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

    // One call each, and the closing one still happened.
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
        "the hard stop must not open a targeted round"
    );
    // The round that the budget closed says so, and the synthesis round still
    // ends `Completed`: the closing call is not a budget casualty.
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::BudgetExhausted), (2, StopReason::Completed)]
    );
    // The reason is distinguishable from the protocol's own three, on the wire
    // and in the narration a person reads.
    let rendered = fixture.stderr.text();
    assert!(
        rendered.contains(&fs_agent::render::wording::round_ended(
            1,
            StopReason::BudgetExhausted
        )),
        "{rendered}"
    );
    assert!(
        !rendered.contains(&fs_agent::render::wording::round_ended(
            1,
            StopReason::RoundsExhausted
        )),
        "{rendered}"
    );
}

#[tokio::test]
async fn the_synthesizer_is_the_one_call_the_budget_cannot_skip() {
    // An allowance of zero stops before the first provider call, so no debater
    // ever speaks. The closing call is still made: degrading *past* synthesis
    // would throw away the whole discussion (spec §17).
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
    // No debate round opened, so there is no round boundary to close: the only
    // `RoundEnded` belongs to the synthesis round.
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::Completed)],
        "a debate phase that ran no round closes no round"
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
    // The two landing points a cheaper model may be routed to are the
    // synthesizer and the executors (spec §17). Routing the first must not touch
    // the debaters: heterogeneity is the protocol's strongest lever.
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
// A discussion on a live session (`Harness::discuss`, the `/discuss` command)
// ---------------------------------------------------------------------------

/// One single-agent session on a temp log, with a scripted provider — the harness a
/// `/discuss` runs inside.
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
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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
        self.harness.as_mut().expect("harness already shut down")
    }

    /// Two debaters and a synthesizer, each with its own scripted provider.
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
    // The point of `/discuss`: the debaters are siblings of the session the user is in,
    // so their projection turns *its* turns into `user` messages — they argue about what
    // the session is about, not about a question in a vacuum (spec §5, §15).
    let mut fixture = session_fixture(vec![Reply::text("应该复用。"), Reply::text("继续。")]).await;
    fixture
        .harness()
        .run_turn("我们该不该复用事件流？")
        .await
        .unwrap();

    // Both sides conclude the same thing, so the discussion is one round: the round
    // cut, the stream, and the synthesizer's materials are what this test is about.
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
    // Swap in providers this test can read back.
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

    // What the first debater was actually sent: the session's own history, as `user`.
    let first_request = &first.requests()[0].messages;
    assert!(
        first_request.iter().any(|message| matches!(
            message,
            Message::User { content, .. } if content.contains("我们该不该复用事件流？")
        )),
        "the session's question reached the debater: {first_request:?}"
    );
    assert!(
        first_request.iter().any(|message| matches!(
            message,
            Message::User { content, .. } if content.contains("应该复用。")
        )),
        "and so did the answer this session already gave, as another speaker's words: \
         {first_request:?}"
    );

    // One stream: the session's own turn, then the discussion's rounds.
    let events = fixture.events();
    assert_eq!(
        round_starts_in(&events, RoundMode::Independent),
        vec![1],
        "the first discussion on this stream numbers from one"
    );
    assert_eq!(round_starts_in(&events, RoundMode::Synthesis), vec![2]);

    // And the session is still usable afterwards: the discussion appended to its
    // stream rather than taking it over.
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
    // Two discussions in one session is exactly what `/discuss` makes possible, so the
    // round numbers have to stay unique on the stream — and the second synthesizer must
    // not be handed the first discussion's answers to synthesize as well.
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

    // The second discussion runs with fresh providers, so its requests are readable on
    // their own.
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
        "the second discussion numbers after the first one's rounds"
    );
    assert_eq!(
        round_starts_in(&events, RoundMode::Synthesis),
        vec![2, 4],
        "and so does its synthesis"
    );

    // The second synthesizer's prompt holds the second discussion only.
    let prompt = second_synthesizer
        .requests()
        .last()
        .expect("the synthesizer was called")
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::User { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("a user message");
    assert!(prompt.contains("第二场的甲"), "{prompt}");
    assert!(prompt.contains("第二个讨论"), "{prompt}");
    assert!(
        !prompt.contains("第一场的甲") && !prompt.contains("第一个讨论"),
        "the first discussion is not material for the second synthesis: {prompt}"
    );
    // And the replayed prompt is the one that was sent: the invariant the whole
    // event-stream design rests on, now with two discussions on one stream.
    let replayed = fs_agent::agent::replay::replay(
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
    assert_eq!(replayed_prompt, prompt, "replay == what was sent");
}

#[tokio::test]
async fn a_persona_reaches_its_own_side_only_and_the_call_stays_recomputable() {
    // The soul is the one thing about a debater that is *not* derivable from its name,
    // so it travels on the stream: recorded as an injection attributed to that debater,
    // private to it, and therefore part of what `sessions replay` recomputes (spec §5,
    // §15).
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

    // Each side is told its own character and not the other's.
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

    // It is on the stream, attributed to the debater it describes.
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
    assert_eq!(personas.len(), 2, "one injection per debater: {personas:?}");
    assert!(personas[0].1.contains("法外狂徒"), "{personas:?}");

    // And because it is on the stream, the replay of that call is what was sent.
    let replayed = fs_agent::agent::replay::replay(
        &events,
        &SpeakerId::Debater("张三".into()),
        Some(1),
        &caps_for("deepseek-flash").unwrap(),
    )
    .unwrap();
    assert_eq!(
        replayed.len(),
        first.requests()[0].messages.len(),
        "same shape: {replayed:?}"
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
