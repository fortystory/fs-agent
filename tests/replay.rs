//! `sessions replay`: the recomputability acceptance (spec §18, Testing Decisions 3).
//!
//! The contract under test is the strongest one the architecture makes: the
//! `messages` recomputed from a finished stream **equal** the `messages` that were
//! actually handed to the provider. The fake provider records every request, so
//! the two can be compared directly — no second implementation, no approximation.
//!
//! Two shapes are covered: a single-agent turn (the whole-stream scope) and a
//! discussion (the structural round window, both the independent and the targeted
//! round, plus the synthesizer's single shot).

mod support;

use fs_agent::agent::replay::{replay, ReplayError};
use fs_agent::config::SessionConfig;
use fs_agent::events::{Event, ParticipantId, SessionId, SpeakerId};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::capability::caps_for;
use fs_agent::provider::{FinishReason, Message, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{
    assemble, assemble_discussion, AssemblyParts, DebaterParts, DiscussionHarness, DiscussionParts,
    Harness, SessionScaffold, SynthesizerParts,
};
use support::{CaptureBuf, FakeProvider, Reply};

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn deepseek() -> SpeakerId {
    SpeakerId::Debater("deepseek".into())
}

fn caps() -> fs_agent::provider::capability::ModelCaps {
    caps_for("deepseek-flash").expect("built-in model")
}

/// An answer as a debater writes it: prose, then the marker line.
fn answered(body: &str, conclusion: &str) -> Reply {
    Reply::text(&format!("{body}\nCONCLUSION: {conclusion}"))
}

struct Fixture {
    harness: DiscussionHarness,
    kimi: FakeProvider,
    deepseek: FakeProvider,
    synthesizer: FakeProvider,
    _dir: tempfile::TempDir,
}

async fn discussion(
    kimi_replies: Vec<Reply>,
    deepseek_replies: Vec<Reply>,
    synthesizer_replies: Vec<Reply>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();

    let kimi_provider = FakeProvider::new(kimi_replies);
    let deepseek_provider = FakeProvider::new(deepseek_replies);
    let synthesizer_provider = FakeProvider::new(synthesizer_replies);
    let config = SessionConfig::new("fake-model");

    let harness = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: session.join("log.jsonl"),
            session_id: SessionId::new("s-replay"),
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
                config: config.clone(),
                provider: Box::new(kimi_provider.clone()),
            },
            DebaterParts {
                speaker: deepseek(),
                config: config.clone(),
                provider: Box::new(deepseek_provider.clone()),
            },
        ],
        synthesizer: SynthesizerParts {
            config,
            provider: Box::new(synthesizer_provider.clone()),
        },
        max_rounds: None,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout),
            stderr_diagnostic: Box::new(stderr),
        }),
    })
    .await
    .unwrap();

    Fixture {
        harness,
        kimi: kimi_provider,
        deepseek: deepseek_provider,
        synthesizer: synthesizer_provider,
        _dir: dir,
    }
}

#[tokio::test]
async fn an_independent_round_replays_to_what_the_debaters_were_sent() {
    let mut fixture = discussion(
        vec![answered("复用共享事件流。", "复用共享事件流")],
        vec![answered("复用共享事件流即可。", "复用共享事件流")],
        vec![Reply::text("共识：复用共享事件流。")],
    )
    .await;

    fixture.harness.discuss("怎么共享状态？").await.unwrap();
    let events = fixture.harness.events();
    fixture.harness.shutdown().await;

    let kimi_sent = fixture.kimi.requests();
    let deepseek_sent = fixture.deepseek.requests();
    assert_eq!(kimi_sent.len(), 1);
    assert_eq!(deepseek_sent.len(), 1);

    assert_eq!(
        replay(&events, &kimi(), Some(1), &caps()).unwrap(),
        kimi_sent[0].messages,
        "round one for kimi recomputes byte for byte"
    );
    assert_eq!(
        replay(&events, &deepseek(), Some(1), &caps()).unwrap(),
        deepseek_sent[0].messages,
        "round one for deepseek recomputes byte for byte"
    );
}

#[tokio::test]
async fn a_targeted_round_replays_to_what_was_sent_and_still_reveals_the_first_round() {
    let mut fixture = discussion(
        vec![
            answered("用共享事件流。", "共享事件流"),
            answered("我还是坚持共享事件流。", "共享事件流"),
        ],
        vec![
            answered("用每个 agent 各自的状态。", "各自的状态"),
            answered("定向第二轮之后我仍然坚持。", "各自的状态"),
        ],
        vec![Reply::text("分歧：共享事件流 vs 各自状态。")],
    )
    .await;

    fixture.harness.discuss("怎么共享状态？").await.unwrap();
    let events = fixture.harness.events();
    fixture.harness.shutdown().await;

    let kimi_sent = fixture.kimi.requests();
    assert_eq!(
        kimi_sent.len(),
        2,
        "a divergent discussion opens a second round"
    );
    assert_eq!(
        replay(&events, &kimi(), Some(1), &caps()).unwrap(),
        kimi_sent[0].messages
    );
    assert_eq!(
        replay(&events, &kimi(), Some(2), &caps()).unwrap(),
        kimi_sent[1].messages,
        "the recomputed targeted round is the one that was sent"
    );
    // The targeted round is only meaningful if the first round is in it.
    let second = &kimi_sent[1].messages;
    assert!(
        second.iter().any(|message| matches!(
            message,
            Message::User { content, .. } if content.contains("各自的状态")
        )),
        "the second round reveals the other side's first-round answer: {second:?}"
    );
}

#[tokio::test]
async fn the_synthesizers_single_shot_replays_to_what_was_sent() {
    let mut fixture = discussion(
        vec![answered("复用共享事件流。", "复用共享事件流")],
        vec![answered("复用共享事件流即可。", "复用共享事件流")],
        vec![Reply::text("共识：复用共享事件流。")],
    )
    .await;

    fixture.harness.discuss("怎么共享状态？").await.unwrap();
    let events = fixture.harness.events();
    fixture.harness.shutdown().await;

    let sent = fixture.synthesizer.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        replay(&events, &SpeakerId::System, Some(2), &caps()).unwrap(),
        sent[0].messages,
        "the closing call is its identity plus a prompt recomputed from the stream"
    );
}

#[tokio::test]
async fn a_round_that_never_ran_is_a_refusal_not_an_empty_projection() {
    let events: Vec<Event> = Vec::new();
    assert_eq!(
        replay(&events, &kimi(), Some(1), &caps()).unwrap_err(),
        ReplayError::UnknownRound(1)
    );
}

struct SoloFixture {
    harness: Harness,
    provider: FakeProvider,
    _dir: tempfile::TempDir,
}

async fn solo(replies: Vec<Reply>) -> SoloFixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("solo".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout),
            stderr_diagnostic: Box::new(stderr),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: session.join("log.jsonl"),
            session_id: SessionId::new("s-solo"),
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

    SoloFixture {
        harness,
        provider,
        _dir: dir,
    }
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[tokio::test]
async fn a_single_agent_turn_replays_to_what_was_sent() {
    let mut fixture = solo(vec![Reply::text("你好。")]).await;
    fixture.harness.run_turn("打个招呼").await.unwrap();
    let events = fixture.harness.events();
    fixture.harness.shutdown().await;

    let sent = fixture.provider.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        replay(&events, &SpeakerId::Debater("solo".into()), None, &caps()).unwrap(),
        sent[0].messages
    );
}

#[tokio::test]
async fn the_second_call_of_a_tool_turn_replays_with_its_tool_round_trip() {
    // The first call carries the question; the second carries the question, the
    // assistant's tool call and its result. Replay reproduces the call, not the
    // finished state, which is why the last request is the reproducible one.
    let mut fixture = solo(vec![
        Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call-1".into(),
                name: "read_file".into(),
                arguments: "{\"file_path\":\"src/lib.rs\"}".into(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ]),
        Reply::text("读完了。"),
    ])
    .await;
    write(
        &fixture._dir.path().join("workspace/src/lib.rs"),
        "pub fn main() {}\n",
    );

    fixture.harness.run_turn("读 src/lib.rs").await.unwrap();
    let events = fixture.harness.events();
    fixture.harness.shutdown().await;

    let sent = fixture.provider.requests();
    assert_eq!(sent.len(), 2, "the tool call opened a second iteration");
    assert_eq!(
        replay(&events, &SpeakerId::Debater("solo".into()), None, &caps()).unwrap(),
        sent[1].messages,
        "the recomputed second call carries the tool call and its one result"
    );
    // The tool round trip really is in there: a projection that dropped it would
    // still differ from the wire, but this makes the failure legible.
    let has_tool = sent[1]
        .messages
        .iter()
        .any(|message| matches!(message, Message::Tool { .. }));
    assert!(has_tool, "{:?}", sent[1].messages);
}

#[tokio::test]
async fn an_executors_nested_window_replays_to_what_was_sent() {
    // The dispatcher, the executor and the dispatcher again all answer on one
    // client, so the middle request is the executor's. Its window is its own
    // events plus the pinned injections, and its identity is the executor's —
    // both derived from the stream by replay (spec §16).
    let mut fixture = solo(vec![
        Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call-1".into(),
                name: "task".into(),
                arguments: "{\"brief\":\"count the files under src\"}".into(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ]),
        Reply::text("EXECUTOR REPORT: 12 files under src"),
        Reply::text("the executor counted 12 files"),
    ])
    .await;

    fixture.harness.run_turn("count the files").await.unwrap();
    let events = fixture.harness.events();
    fixture.harness.shutdown().await;

    let sent = fixture.provider.requests();
    assert_eq!(sent.len(), 3, "dispatcher, executor, dispatcher");
    let executor = SpeakerId::Executor(ParticipantId::new("solo-1"));
    assert_eq!(
        replay(&events, &executor, None, &caps()).unwrap(),
        sent[1].messages,
        "the executor's own window recomputes byte for byte"
    );
    // It is genuinely the executor's window: exactly its identity and its brief,
    // with the dispatching session's question nowhere in it (spec §16).
    let users: Vec<&Message> = sent[1]
        .messages
        .iter()
        .filter(|message| matches!(message, Message::User { .. }))
        .collect();
    assert_eq!(users.len(), 1, "{:?}", sent[1].messages);
    assert!(
        matches!(users[0], Message::User { content, .. } if content == "count the files under src"),
        "{:?}",
        users[0]
    );
    assert!(
        matches!(&sent[1].messages[0], Message::System { content, .. }
            if content.starts_with("You are an executor")),
        "{:?}",
        sent[1].messages[0]
    );
}
