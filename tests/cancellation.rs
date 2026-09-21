//! Cancellation propagation (spec §6, ticket 13).
//!
//! One gesture — the front end's Esc — stops the turn that is in flight and
//! travels **down** to the executors that turn dispatched. The gesture itself is
//! not an event; what the assertable contract sees is only the shape a stop
//! leaves behind: `TurnEnded { Aborted }`, one synthesized result for every
//! `tool_call` that had already started, and no dangling call for a later
//! `--continue` to have to guess at.
//!
//! Everything runs through the one assembly seam with a scripted fake provider,
//! like every other end-to-end test here.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    pending_tool_calls, read_events, Event, EventPayload, SessionId, SpeakerId, StopReason, Usage,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent, ToolSpec};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{Effect, Registry, Tool, ToolContext, ToolError, ToolOutput};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{CaptureBuf, FakeProvider, Reply};
use tokio::sync::Notify;

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    cwd: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, config: SessionConfig) -> Fixture {
    fixture_with_tools(replies, config, fs_agent::tools::builtin()).await
}

async fn fixture_with_tools(
    replies: Vec<Reply>,
    config: SessionConfig,
    tools: Registry,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let log_path = session.join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: kimi(),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-cancel"),
            tools,
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        provider,
        stdout,
        stderr,
        log_path,
        cwd,
        _dir: dir,
    }
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

/// A model turn that asks for one tool call and then stops asking.
fn calls(id: &str, name: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: args.to_string(),
        },
        StreamEvent::Usage(Usage::default()),
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// A tool that never returns on its own.
///
/// The instrument for "cancel a tool that is in flight": the registry is a value
/// injected at the one assembly seam, so a test tool is mounted the same way a
/// built-in is, and the tool announces its entry so the test presses at a defined
/// moment instead of sleeping.
struct StallingTool {
    started: Arc<Notify>,
}

#[async_trait::async_trait]
impl Tool for StallingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "stall".to_owned(),
            description: "Never returns.".to_owned(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    fn effect(&self, _args: &serde_json::Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(
        &self,
        _ctx: &ToolContext<'_>,
        _args: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        self.started.notify_one();
        futures::future::pending::<()>().await;
        unreachable!("the stalling tool never returns on its own")
    }
}

fn kinds(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(|event| event.payload.kind()).collect()
}

#[tokio::test]
async fn a_cancel_stops_an_in_flight_provider_stream_without_entering_the_log() {
    let opened = Arc::new(Notify::new());
    let mut fixture = fixture(
        vec![Reply::Stall(
            opened.clone(),
            vec![StreamEvent::TextDelta("half a thought".into())],
        )],
        SessionConfig::new("fake-model"),
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    // The turn's future borrows the harness, so the whole gesture lives in a
    // block: it must be dropped before the harness is shut down.
    let outcome = {
        let turn = fixture.harness.run_turn("think out loud");
        tokio::pin!(turn);
        // Cancel while the stream is really in flight, not before it opened and
        // not after it ended.
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut turn => panic!("the turn ended before the cancel: {outcome:?}"),
        }
        cancel.cancel();
        turn.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(fixture.provider.requests().len(), 1);

    // The gesture is not in the stream: what a cancel leaves is exactly the
    // skeleton, the user's question, the turn it stopped, and its abort.
    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        kinds(&events),
        vec![
            "SessionStarted",
            "MessageCompleted",
            "TurnStarted",
            "TurnEnded",
        ],
        "{events:#?}"
    );
    let EventPayload::TurnEnded { reason } = events.last().unwrap().payload else {
        unreachable!("the last event is the turn end");
    };
    assert_eq!(reason, StopReason::Aborted);

    // A stream that never reached `[DONE]` produced no completed unit, so no
    // partial answer lands in the log.
    assert!(
        !events.iter().any(
            |event| matches!(event.payload, EventPayload::MessageCompleted { .. })
                && event.speaker_id == kimi()
                && matches!(
                    event.payload,
                    EventPayload::MessageCompleted {
                        role: fs_agent::events::Role::Assistant,
                        ..
                    }
                )
        ),
        "the aborted stream's partial text must not be recorded"
    );
    assert!(pending_tool_calls(&events).is_empty());
    // Nothing was completed, so nothing reaches the final-product sink; the
    // narration of the stop goes to the diagnostic sink instead.
    assert_eq!(fixture.stdout.text(), "");
    assert!(
        fixture.stderr.text().contains("cancelled"),
        "{}",
        fixture.stderr.text()
    );
}

#[tokio::test]
async fn a_cancel_during_a_tool_call_gives_that_call_its_one_result() {
    let started = Arc::new(Notify::new());
    let mut tools = Registry::new();
    tools.register(Box::new(StallingTool {
        started: started.clone(),
    }));
    let mut fixture = fixture_with_tools(
        vec![calls("call-1", "stall", serde_json::json!({}))],
        SessionConfig::new("fake-model"),
        tools,
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    let outcome = {
        let turn = fixture.harness.run_turn("stall for a while");
        tokio::pin!(turn);
        tokio::select! {
            _ = started.notified() => {}
            outcome = &mut turn => panic!("the turn ended before the cancel: {outcome:?}"),
        }
        cancel.cancel();
        turn.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    let events = read_events(&fixture.log_path).unwrap();

    // The call had already started, so it is owed exactly one result — the
    // fifth exception path of spec §3, and the reason the invariant still holds
    // when the gesture lands mid-tool.
    let completed: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(completed.len(), 1, "{events:#?}");
    let EventPayload::ToolCallCompleted {
        tool_call_id,
        ok,
        output,
        error,
        ..
    } = &completed[0].payload
    else {
        unreachable!("filtered for ToolCallCompleted")
    };
    assert_eq!(tool_call_id.as_str(), "call-1");
    assert!(!ok);
    assert!(output.is_none());
    let error = error.as_deref().unwrap_or_default();
    assert!(error.contains("cancelled"), "{error:?}");
    assert!(pending_tool_calls(&events).is_empty());

    let endings: Vec<StopReason> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::TurnEnded { reason } => Some(*reason),
            _ => None,
        })
        .collect();
    assert_eq!(endings, vec![StopReason::Aborted]);
}

#[tokio::test]
async fn a_cancel_closes_a_deferred_task_call_with_the_result_it_owes() {
    let started = Arc::new(Notify::new());
    let mut tools = fs_agent::tools::builtin();
    tools.register(Box::new(StallingTool {
        started: started.clone(),
    }));
    let mut fixture = fixture_with_tools(
        vec![Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call-task".to_owned(),
                name: "task".to_owned(),
                arguments: serde_json::json!({"brief": "do the thing"}).to_string(),
            },
            StreamEvent::ToolCallCompleted {
                index: 1,
                id: "call-stall".to_owned(),
                name: "stall".to_owned(),
                arguments: "{}".to_owned(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ])],
        SessionConfig::new("fake-model"),
        tools,
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    let outcome = {
        let turn = fixture.harness.run_turn("delegate, then stall");
        tokio::pin!(turn);
        tokio::select! {
            _ = started.notified() => {}
            outcome = &mut turn => panic!("the turn ended before the cancel: {outcome:?}"),
        }
        cancel.cancel();
        turn.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    let events = read_events(&fixture.log_path).unwrap();

    /// The one result a call got, as `(ok, text)`.
    fn result_of(events: &[Event], want: &str) -> (bool, String) {
        let found: Vec<&Event> = events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventPayload::ToolCallCompleted { tool_call_id, .. }
                        if tool_call_id.as_str() == want
                )
            })
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one result for {want}");
        let EventPayload::ToolCallCompleted {
            ok, output, error, ..
        } = &found[0].payload
        else {
            unreachable!("filtered for ToolCallCompleted")
        };
        (
            *ok,
            output.clone().or_else(|| error.clone()).unwrap_or_default(),
        )
    }

    // The `task` call was recorded as started and then deferred, so it is owed
    // exactly one result even though its executor never ran at all.
    let (task_ok, task_text) = result_of(&events, "call-task");
    assert!(!task_ok);
    assert!(task_text.contains("did not run"), "{task_text:?}");

    // The call that was really in flight says the other thing: the tool future
    // was dropped, so the workspace may or may not have changed.
    let (stall_ok, stall_text) = result_of(&events, "call-stall");
    assert!(!stall_ok);
    assert!(stall_text.contains("in flight"), "{stall_text:?}");

    // The deferred executor was never dispatched...
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. })),
        "{events:#?}"
    );
    assert!(pending_tool_calls(&events).is_empty());
    // ...and the stop happened before another model call.
    assert_eq!(fixture.provider.requests().len(), 1);
}

// ---------------------------------------------------------------------------
// A discussion: the gesture travels down to the executors and never up.
// ---------------------------------------------------------------------------

struct DiscussionFixture {
    harness: fs_agent::DiscussionHarness,
    kimi: FakeProvider,
    deepseek: FakeProvider,
    synthesizer: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn discussion_fixture(
    kimi_replies: Vec<Reply>,
    deepseek_replies: Vec<Reply>,
    synthesizer_replies: Vec<Reply>,
) -> DiscussionFixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let log_path = session.join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let kimi_provider = FakeProvider::new(kimi_replies);
    let deepseek_provider = FakeProvider::new(deepseek_replies);
    let synthesizer_provider = FakeProvider::new(synthesizer_replies);

    let harness = fs_agent::assemble_discussion(fs_agent::DiscussionParts {
        scaffold: SessionScaffold {
            cwd,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-cancel-discussion"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            hook: None,
            home: None,
        },
        debaters: vec![
            fs_agent::DebaterParts {
                speaker: kimi(),
                config: SessionConfig::new("fake-model"),
                provider: Box::new(kimi_provider.clone()),
            },
            fs_agent::DebaterParts {
                speaker: SpeakerId::Debater("deepseek".into()),
                config: SessionConfig::new("fake-model"),
                provider: Box::new(deepseek_provider.clone()),
            },
        ],
        synthesizer: fs_agent::SynthesizerParts {
            config: SessionConfig::new("fake-model"),
            provider: Box::new(synthesizer_provider.clone()),
        },
        max_rounds: Some(2),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
    })
    .await
    .unwrap();

    DiscussionFixture {
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
    Reply::text(&format!("{body}\nCONCLUSION: {conclusion}"))
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
async fn a_cancel_reaches_a_running_executor_and_the_discussion_is_not_an_error() {
    let opened = Arc::new(Notify::new());
    // The roster order matters here, and it is the order `join_all` polls in:
    // the first debater answers normally and its whole turn completes in the
    // first poll, so the other side's stalled executor is the only thing left in
    // flight when the gesture arrives.
    let mut fixture = discussion_fixture(
        vec![answered("KIMI 正文", "先做甲")],
        vec![
            // The dispatcher asks for an executor...
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "do the thing"}),
            ),
            // ...and the executor's own stream is what the gesture interrupts.
            Reply::Stall(
                opened.clone(),
                vec![StreamEvent::TextDelta("working on it".into())],
            ),
        ],
        // The synthesizer is never called.
        vec![],
    )
    .await;

    let signal = fixture.harness.cancel_signal();
    let outcome = {
        let discuss = fixture.harness.discuss("这件事该怎么做？");
        tokio::pin!(discuss);
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut discuss => panic!("the discussion ended before the cancel: {outcome:?}"),
        }
        signal.cancel();
        discuss.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(outcome.synthesis, "");
    // The gesture stops the round; it does not open the closing call.
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 2);
    assert_eq!(fixture.synthesizer.requests().len(), 0);

    let events = read_events(&fixture.log_path).unwrap();

    // The one side that got all the way through really did answer: the round is
    // stopped, not merely incomplete.
    assert!(
        events.iter().any(|event| {
            event.speaker_id == kimi()
                && matches!(
                    &event.payload,
                    EventPayload::MessageCompleted {
                        role: fs_agent::events::Role::Assistant,
                        ..
                    }
                )
        }),
        "{events:#?}"
    );

    // The executor winds down instead of being dropped: it gets a finish line,
    // and `Aborted` — not `Error` — is what it says (spec §6).
    let finished: Vec<(String, StopReason)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ExecutorFinished {
                executor_id,
                reason,
                ..
            } => Some((executor_id.to_string(), *reason)),
            _ => None,
        })
        .collect();
    assert_eq!(
        finished,
        vec![("deepseek-1".to_owned(), StopReason::Aborted)]
    );

    // The `task` call had started, so it keeps exactly one result, and the
    // dispatcher reads the cancellation as an ordinary failed tool result.
    assert!(pending_tool_calls(&events).is_empty());
    let results: Vec<&EventPayload> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .map(|event| &event.payload)
        .collect();
    assert_eq!(results.len(), 1, "{events:#?}");
    let EventPayload::ToolCallCompleted { ok, error, .. } = results[0] else {
        unreachable!("filtered for ToolCallCompleted")
    };
    assert!(!ok);
    let error = error.as_deref().unwrap_or_default();
    assert!(error.contains("Aborted"), "{error:?}");

    // A cancelled round is not a debate result, and the debater that was
    // stopped did not become an "absence read as consensus".
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::Aborted)],
        "{events:#?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionError { .. })),
        "a cancelled discussion is not a session failure"
    );
    assert!(
        !events.iter().any(
            |event| matches!(event.payload, EventPayload::RoundStarted { .. })
                && matches!(
                    event.payload,
                    EventPayload::RoundStarted {
                        mode: fs_agent::events::RoundMode::Synthesis,
                        ..
                    }
                )
        ),
        "the synthesizer never opens"
    );

    // Nothing completed, so the final-product sink stays empty; the narration
    // of the stop goes to the diagnostic sink instead.
    assert_eq!(fixture.stdout.text(), "");
    assert!(
        fixture.stderr.text().contains("cancelled"),
        "{}",
        fixture.stderr.text()
    );
}

#[tokio::test]
async fn a_cancel_that_stops_both_debaters_is_still_not_a_discussion_failure() {
    let kimi_opened = Arc::new(Notify::new());
    let deepseek_opened = Arc::new(Notify::new());
    let mut fixture = discussion_fixture(
        vec![Reply::Stall(
            kimi_opened.clone(),
            vec![StreamEvent::TextDelta("kimi half an answer".into())],
        )],
        vec![Reply::Stall(
            deepseek_opened.clone(),
            vec![StreamEvent::TextDelta("deepseek half an answer".into())],
        )],
        vec![],
    )
    .await;

    let signal = fixture.harness.cancel_signal();
    let outcome = {
        let discuss = fixture.harness.discuss("这件事该怎么做？");
        tokio::pin!(discuss);
        tokio::select! {
            _ = async {
                tokio::join!(kimi_opened.notified(), deepseek_opened.notified());
            } => {}
            outcome = &mut discuss => panic!("the discussion ended before the cancel: {outcome:?}"),
        }
        signal.cancel();
        discuss.await.unwrap()
    };
    fixture.harness.shutdown().await;

    // Nobody answered, and the round still is not the Error that "nobody
    // answered" would be if the round had simply failed: the gesture is what
    // stopped it (spec §6).
    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(outcome.rounds, 1);
    assert_eq!(outcome.synthesis, "");

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(round_endings(&events), vec![(1, StopReason::Aborted)]);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionError { .. })),
        "{events:#?}"
    );
    assert!(pending_tool_calls(&events).is_empty());
    assert_eq!(fixture.synthesizer.requests().len(), 0);
    assert_eq!(fixture.stdout.text(), "");
}

#[tokio::test]
async fn a_cancel_that_reaches_the_synthesizer_ends_the_discussion_without_a_product() {
    let opened = Arc::new(Notify::new());
    let mut fixture = discussion_fixture(
        vec![answered("KIMI 正文", "复用事件流")],
        vec![answered("DEEPSEEK 正文", "复用事件流")],
        // The closing call is the one that stalls: the debate itself finished.
        vec![Reply::Stall(
            opened.clone(),
            vec![StreamEvent::TextDelta("共识：复用".into())],
        )],
    )
    .await;

    let signal = fixture.harness.cancel_signal();
    let outcome = {
        let discuss = fixture.harness.discuss("要不要复用事件流？");
        tokio::pin!(discuss);
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut discuss => panic!("the discussion ended before the cancel: {outcome:?}"),
        }
        signal.cancel();
        discuss.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(outcome.synthesis, "");
    assert_eq!(fixture.synthesizer.requests().len(), 1);

    let events = read_events(&fixture.log_path).unwrap();
    // The debate phase really did end `NoDivergence`; it is the closing round
    // that the gesture stopped.
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::NoDivergence), (2, StopReason::Aborted)]
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionError { .. })),
        "a stopped synthesizer is not a `synthesis_failed`: {events:#?}"
    );
    // A partial product never lands, and nothing reaches the final-product sink.
    assert!(
        !events.iter().any(
            |event| matches!(event.payload, EventPayload::MessageCompleted { .. })
                && event.speaker_id == SpeakerId::System
        ),
        "{events:#?}"
    );
    assert_eq!(fixture.stdout.text(), "");
}

#[tokio::test]
async fn a_killed_cancelled_session_resumes_and_closes_the_call_it_left_open() {
    let opened = Arc::new(Notify::new());
    let fixture = fixture(
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "do the thing"}),
            ),
            Reply::Stall(
                opened.clone(),
                vec![StreamEvent::TextDelta("working".into())],
            ),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;
    let Fixture {
        mut harness,
        log_path,
        cwd,
        _dir,
        ..
    } = fixture;

    let signal = harness.cancel_signal();
    {
        let turn = harness.run_turn("delegate the thing");
        tokio::pin!(turn);
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut turn => panic!("the turn ended before the cancel: {outcome:?}"),
        }
        signal.cancel();
        // The second press during a cancellation forces the process down: the
        // turn is dropped mid-flight and nothing winds down (spec §6).
    }
    harness.shutdown().await;

    // The stream the killed process left behind has a `tool_call` with no
    // result. `--continue` opens the same log under the same id, and recovery
    // is what closes it.
    let resumed = assemble(AssemblyParts {
        provider: Box::new(FakeProvider::new(vec![Reply::text("resumed")])),
        speaker: kimi(),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-cancel"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            hook: None,
            home: None,
        },
    })
    .await
    .expect("a stream with an open call is resumable");
    assert_eq!(resumed.session_id().as_str(), "s-cancel");
    resumed.shutdown().await;

    let events = read_events(&log_path).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::SessionStarted { .. }))
            .count(),
        1,
        "a resume never records a second session head"
    );
    assert!(pending_tool_calls(&events).is_empty());
    let recovered = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok: false,
                error: Some(error),
                ..
            } if tool_call_id.as_str() == "call-1" => Some(error.clone()),
            _ => None,
        })
        .expect("recovery closed the call the process died on");
    assert!(recovered.contains("interrupted"), "{recovered}");
}

#[tokio::test]
async fn a_gesture_is_scoped_to_one_run_so_a_cancelled_session_stays_usable() {
    let mut fixture = fixture(
        vec![
            Reply::text("a fresh answer"),
            Reply::text("a second answer"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    assert!(!cancel.is_cancelled());
    // Raised while nothing is running: there is no work to stop...
    cancel.cancel();
    assert!(cancel.is_cancelled());

    // ...and the next turn does not inherit it. Without this, one Esc would
    // wedge the session for every later question in the same process.
    let outcome = fixture.harness.run_turn("do something").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.provider.requests().len(), 1);
    assert!(!cancel.is_cancelled(), "a run starts from a clean gesture");

    let outcome = fixture.harness.run_turn("and another thing").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.provider.requests().len(), 2);
    fixture.harness.shutdown().await;
}
