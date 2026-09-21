//! The executor (`task`, spec §16).
//!
//! The seam is the assembly entry: a session assembled with a scripted fake
//! provider, driven through the public library API, asserting the JSONL event
//! stream, the workspace and the two render sinks. The executor runs on the
//! provider of the session that dispatched it (spec §16: the model is inherited),
//! so one `FakeProvider` scripts the whole nested conversation in call order.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, Decision, Event, EventPayload, ParticipantId, Role, SessionId, SpeakerId,
    StopReason, Usage,
};
use fs_agent::permissions::{Asker, Mode, Policy, Rule, Scope, Subject};
use fs_agent::provider::{FinishReason, Message, ProviderError, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn executor(id: &str) -> SpeakerId {
    SpeakerId::Executor(ParticipantId::new(id))
}

/// One scripted assistant message that asks for one tool call.
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

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    /// The session directory: the log and the `outputs/` artifacts.
    session_dir: PathBuf,
    /// The session workspace, where the tools read and write.
    cwd: PathBuf,
    _dir: tempfile::TempDir,
}

/// Assemble a session whose workspace already holds `files`, so `AGENTS.md` and
/// the executor's world are in place before assembly reads them.
async fn fixture(
    files: &[(&str, &str)],
    replies: Vec<Reply>,
    config: SessionConfig,
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
) -> Fixture {
    fixture_with(files, replies, config, policy, asker, None).await
}

/// The same, with a hook mounted.
async fn fixture_with(
    files: &[(&str, &str)],
    replies: Vec<Reply>,
    config: SessionConfig,
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn fs_agent::hooks::Hook>>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    for (name, content) in files {
        let path = cwd.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }
    let log_path = session_dir.join("log.jsonl");
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
            session_id: SessionId::new("s-executor"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy,
            asker,
            hook,
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
        session_dir,
        cwd,
        _dir: dir,
    }
}

impl Fixture {
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.cwd.join(name)).unwrap()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }
}

/// The one event matching `predicate`, or a panic naming what was missing.
fn only<'a>(
    events: &'a [Event],
    predicate: impl Fn(&EventPayload) -> bool,
    want: &str,
) -> &'a Event {
    let found: Vec<&Event> = events
        .iter()
        .filter(|event| predicate(&event.payload))
        .collect();
    assert_eq!(found.len(), 1, "expected exactly one {want}, got {found:?}");
    found[0]
}

/// The one event from `speaker` matching `predicate`, or a panic naming what was
/// missing.
fn only_speaker<'a>(
    events: &'a [Event],
    speaker: &SpeakerId,
    predicate: impl Fn(&EventPayload) -> bool,
    want: &str,
) -> &'a Event {
    let found: Vec<&Event> = events
        .iter()
        .filter(|event| &event.speaker_id == speaker && predicate(&event.payload))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one {want} from {speaker}, got {found:?}"
    );
    found[0]
}

/// Every `MessageCompleted` text by one speaker, in order.
fn said(events: &[Event], speaker: &SpeakerId) -> Vec<String> {
    events
        .iter()
        .filter(|event| &event.speaker_id == speaker)
        .filter_map(|event| match &event.payload {
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text,
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn tool_names(request: &fs_agent::provider::ChatRequest) -> Vec<String> {
    request.tools.iter().map(|tool| tool.name.clone()).collect()
}

fn contents(
    request: &fs_agent::provider::ChatRequest,
    want: &dyn Fn(&Message) -> bool,
) -> Vec<String> {
    request
        .messages
        .iter()
        .filter(|message| want(message))
        .map(|message| match message {
            Message::System { content, .. }
            | Message::User { content, .. }
            | Message::Tool { content, .. } => content.clone(),
            Message::Assistant { content, .. } => content.clone().unwrap_or_default(),
        })
        .collect()
}

fn is_user(message: &Message) -> bool {
    matches!(message, Message::User { .. })
}

#[tokio::test]
async fn a_task_call_runs_a_nested_executor_and_reports_the_summary_back() {
    let mut fixture = fixture(
        &[("AGENTS.md", "PROJECT RULES: always run cargo fmt")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "count the files under src"}),
            ),
            Reply::text("EXECUTOR REPORT: 12 files under src"),
            Reply::text("the executor counted 12 files"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("count them for me, please")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "the executor counted 12 files");

    let events = fixture.events();
    let spawned = only(
        &events,
        |payload| matches!(payload, EventPayload::ExecutorSpawned { .. }),
        "ExecutorSpawned",
    );
    // The lifecycle events are the executor's own, so the brief can reach its
    // projection; `parent` is what names the dispatcher.
    assert_eq!(spawned.speaker_id, executor("kimi-1"));
    match &spawned.payload {
        EventPayload::ExecutorSpawned {
            executor_id,
            parent,
            brief,
        } => {
            assert_eq!(executor_id.as_str(), "kimi-1");
            assert_eq!(parent.as_str(), "kimi");
            assert_eq!(brief, "count the files under src");
        }
        other => panic!("expected ExecutorSpawned, got {other:?}"),
    }

    let finished = only(
        &events,
        |payload| matches!(payload, EventPayload::ExecutorFinished { .. }),
        "ExecutorFinished",
    );
    match &finished.payload {
        EventPayload::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => {
            assert_eq!(executor_id.as_str(), "kimi-1");
            assert_eq!(*reason, StopReason::Completed);
            assert_eq!(summary, "EXECUTOR REPORT: 12 files under src");
        }
        other => panic!("expected ExecutorFinished, got {other:?}"),
    }

    // The spawn, the executor's own turn and the finish all land on the one
    // stream, in that order.
    assert!(spawned.seq < finished.seq);
    assert_eq!(
        said(&events, &executor("kimi-1")),
        vec!["EXECUTOR REPORT: 12 files under src".to_owned()]
    );

    // The `task` call itself gets exactly one result, and that result is the
    // summary the dispatching speaker reports to the discussion.
    let result = only(
        &events,
        |payload| matches!(payload, EventPayload::ToolCallCompleted { .. }),
        "ToolCallCompleted",
    );
    match &result.payload {
        EventPayload::ToolCallCompleted { ok, output, .. } => {
            assert!(ok);
            let output = output.as_deref().unwrap();
            assert!(
                output.contains("EXECUTOR REPORT: 12 files under src"),
                "{output}"
            );
            assert!(output.contains("Completed"), "{output}");
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }

    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 3, "parent, executor, parent");

    // The dispatcher's own table mounts `task`; the executor's does not, because
    // recursion depth one is enforced by the tool table, not by a rule.
    assert!(tool_names(&requests[0]).contains(&"task".to_owned()));
    assert!(
        !tool_names(&requests[1]).contains(&"task".to_owned()),
        "the executor's tool table must not contain `task`: {:?}",
        tool_names(&requests[1])
    );

    // The executor's window: its private identity, the pinned injections and its
    // own events. The dispatching session's speech is not in it.
    match &requests[1].messages[0] {
        Message::System { content, .. } => {
            assert!(content.contains("executor"), "{content}");
            assert!(!content.contains("CONCLUSION:"), "{content}");
        }
        other => panic!("expected the executor's own system identity, got {other:?}"),
    }
    let head = contents(&requests[1], &is_user).join("\n");
    assert!(
        head.contains("PROJECT RULES: always run cargo fmt"),
        "AGENTS.md is injected for an executor like any other session: {head}"
    );
    assert!(head.contains("count the files under src"), "{head}");
    assert!(
        !head.contains("count them for me"),
        "the executor does not replay the dispatching session's speech: {head}"
    );

    // And the executor's process stays out of the dispatcher's window: the
    // summary arrives once, as the tool result, and never as speech.
    let dispatcher = &requests[2];
    let speech = contents(dispatcher, &is_user).join("\n");
    assert!(
        !speech.contains("EXECUTOR REPORT"),
        "an executor's message must not be projected to a debater: {speech}"
    );
    let results = contents(dispatcher, &|message| {
        matches!(message, Message::Tool { .. })
    })
    .join("\n");
    assert!(results.contains("EXECUTOR REPORT"), "{results}");

    fixture.harness.shutdown().await;
    assert_eq!(fixture.stdout.text(), "the executor counted 12 files\n");
    // The executor's own working is narrated for a person reading the terminal,
    // attributed to the executor.
    assert!(
        fixture.stderr.text().contains("kimi-1"),
        "{}",
        fixture.stderr.text()
    );
}

#[tokio::test]
async fn an_executor_projects_its_own_tool_round_trip_in_full() {
    let mut fixture = fixture(
        &[("notes.txt", "the answer is 42\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "read notes.txt and report the answer"}),
            ),
            calls(
                "exec-1",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            Reply::text("the answer is 42"),
            Reply::text("it reported 42"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate the reading")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    // The executor's own turn ran two iterations: its result body is replayed in
    // full, unlike another speaker's, whose tool calls survive as one line.
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 4, "parent, executor, executor, parent");
    let executor_second = &requests[2];
    let assistant = executor_second
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant { tool_calls, .. } if !tool_calls.is_empty() => Some(tool_calls),
            _ => None,
        })
        .expect("the executor's own tool call is replayed");
    assert_eq!(assistant[0].id, "exec-1");
    let result = executor_second
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Tool {
                tool_call_id,
                content,
            } => Some((tool_call_id.clone(), content.clone())),
            _ => None,
        })
        .expect("the executor's own tool result is replayed");
    assert_eq!(result.0, "exec-1");
    assert!(result.1.contains("the answer is 42"), "{}", result.1);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executors_read_set_starts_empty_and_the_dispatchers_does_not_travel() {
    let mut fixture = fixture(
        &[("plan.txt", "keep me\n")],
        vec![
            // The dispatcher reads the file itself, which licenses its own edit
            // and nothing else.
            calls(
                "call-read",
                "read_file",
                serde_json::json!({"file_path": "plan.txt"}),
            ),
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "edit plan.txt"}),
            ),
            // The executor edits without reading: its read set started empty.
            calls(
                "exec-1",
                "edit_file",
                serde_json::json!({"file_path": "plan.txt", "old_string": "keep me", "new_string": "changed"}),
            ),
            Reply::text("I could not edit it"),
            Reply::text("the executor could not edit it"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        // A user who approves every write, so the gate lets the executor's edit
        // through and the read-before-edit guardrail is what refuses it.
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate the edit").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = fixture.events();
    let failures: Vec<String> = events
        .iter()
        .filter(|event| event.speaker_id == executor("kimi-1"))
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: false,
                error: Some(error),
                ..
            } => Some(error.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("read before write"), "{}", failures[0]);
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executors_edit_is_undoable_like_any_other() {
    // (Ticket 12.) `/undo` walks the session's stream, not one agent's slice, so
    // an edit an executor made rolls back the same way (spec §11, §16).
    let mut fixture = fixture(
        &[("plan.txt", "keep me\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "edit plan.txt"}),
            ),
            // The executor reads first: its read set starts empty.
            calls(
                "exec-read",
                "read_file",
                serde_json::json!({"file_path": "plan.txt"}),
            ),
            calls(
                "exec-edit",
                "edit_file",
                serde_json::json!({"file_path": "plan.txt", "old_string": "keep me", "new_string": "changed"}),
            ),
            Reply::text("edited plan.txt"),
            Reply::text("the executor edited it"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate the edit").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.read("plan.txt"), "changed\n");

    let undone = fixture.harness.undo_last_edit().await.unwrap().unwrap();
    assert_eq!(undone.tool_call_id.as_str(), "exec-edit");
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    // The gesture is the user's; the stream records the retirement it caused.
    let events = fixture.events();
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventPayload::HistorySuperseded {
            reason: fs_agent::events::HistoryReason::Undo,
            ..
        }
    )));

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_failed_executor_is_an_error_result_and_the_dispatcher_carries_on() {
    let mut fixture = fixture(
        &[],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "do something impossible"}),
            ),
            Reply::Fail(ProviderError::Transport {
                detail: "the wire went away".to_owned(),
            }),
            Reply::text("the executor failed, so I will do it myself"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate it").await.unwrap();
    // The dispatcher's own turn is untouched by a failed executor (spec §16).
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "the executor failed, so I will do it myself");

    let events = fixture.events();
    match &only(
        &events,
        |payload| matches!(payload, EventPayload::ExecutorFinished { .. }),
        "ExecutorFinished",
    )
    .payload
    {
        EventPayload::ExecutorFinished { reason, .. } => assert_eq!(*reason, StopReason::Error),
        other => panic!("expected ExecutorFinished, got {other:?}"),
    }
    // The four failure values arrive as one error-content tool result, which is
    // what keeps "every tool_call gets exactly one result" true for `task` too.
    match &only(
        &events,
        |payload| matches!(payload, EventPayload::ToolCallCompleted { .. }),
        "ToolCallCompleted",
    )
    .payload
    {
        EventPayload::ToolCallCompleted { ok, error, .. } => {
            assert!(!ok);
            let error = error.as_deref().unwrap();
            assert!(error.contains("executor kimi-1 finished: Error"), "{error}");
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_runs_under_its_own_turn_cap() {
    let mut fixture = fixture(
        &[],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "keep reading forever"}),
            ),
            // The executor gets two turns and then hits its own cap; the
            // dispatcher's cap is untouched.
            calls(
                "exec-1",
                "read_file",
                serde_json::json!({"file_path": "a.txt"}),
            ),
            calls(
                "exec-2",
                "read_file",
                serde_json::json!({"file_path": "a.txt"}),
            ),
            Reply::text("the executor ran out of turns"),
        ],
        SessionConfig::new("fake-model").with_executor_max_iterations(2),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = fixture.events();
    match &only(
        &events,
        |payload| matches!(payload, EventPayload::ExecutorFinished { .. }),
        "ExecutorFinished",
    )
    .payload
    {
        EventPayload::ExecutorFinished { reason, .. } => {
            assert_eq!(*reason, StopReason::MaxIterations)
        }
        other => panic!("expected ExecutorFinished, got {other:?}"),
    }
    let executor_turns = events
        .iter()
        .filter(|event| {
            event.speaker_id == executor("kimi-1")
                && matches!(event.payload, EventPayload::TurnStarted { .. })
        })
        .count();
    assert_eq!(executor_turns, 2, "the executor's own cap is what bound it");
    // The dispatcher's reason is its own: `MaxIterations` on the executor does
    // not leak upward as the session's stop reason.
    let dispatcher_turns = events
        .iter()
        .filter(|event| {
            event.speaker_id == kimi() && matches!(event.payload, EventPayload::TurnStarted { .. })
        })
        .count();
    assert_eq!(dispatcher_turns, 2);
}

#[tokio::test]
async fn an_executors_edit_leaves_an_undo_snapshot_in_the_one_session_directory() {
    let mut fixture = fixture(
        &[("notes.txt", "before\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "rewrite notes.txt"}),
            ),
            calls(
                "exec-read",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            calls(
                "exec-edit",
                "edit_file",
                serde_json::json!({"file_path": "notes.txt", "old_string": "before", "new_string": "after"}),
            ),
            Reply::text("rewrote it"),
            Reply::text("the executor rewrote notes.txt"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        // The executor's own mode asks about writes; this user approves.
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate the rewrite")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.read("notes.txt"), "after\n");

    // `/undo` works on an executor's edit because the artifacts land in the same
    // session directory under the same naming convention (spec §11, §16).
    let snapshot = fixture.session_dir.join("outputs").join("exec-edit.before");
    assert!(
        snapshot.exists(),
        "expected {} to exist",
        snapshot.display()
    );
    // The snapshot is the replaced span itself, not the whole file: that is what
    // `/undo` writes back (spec §8, §11).
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "before");

    // And the change is reported back as metadata, derived from the stream.
    let reported = fixture
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: true,
                output: Some(output),
                ..
            } if event.speaker_id == kimi() => Some(output.clone()),
            _ => None,
        })
        .expect("the task call's result");
    // The tool reports the path it resolved, so the metadata names the file the
    // edit landed on.
    assert!(reported.contains("notes.txt"), "{reported}");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn two_task_calls_in_one_batch_run_at_once() {
    // Both executor calls must be in flight together: the barrier releases only
    // when the second one arrives, and a serial dispatcher would trip its
    // timeout instead.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut fixture = fixture(
        &[],
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".to_owned(),
                    name: "task".to_owned(),
                    arguments: serde_json::json!({"brief": "count the tests"}).to_string(),
                },
                StreamEvent::ToolCallCompleted {
                    index: 1,
                    id: "call-2".to_owned(),
                    name: "task".to_owned(),
                    arguments: serde_json::json!({"brief": "count the modules"}).to_string(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::Meet(barrier.clone(), Box::new(Reply::text("first report"))),
            Reply::Meet(barrier.clone(), Box::new(Reply::text("second report"))),
            Reply::text("both executors reported"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate both counts")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = fixture.events();
    let spawned: Vec<u64> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. }))
        .map(|event| event.seq)
        .collect();
    let finished: Vec<u64> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ExecutorFinished { .. }))
        .map(|event| event.seq)
        .collect();
    assert_eq!(spawned.len(), 2);
    assert_eq!(finished.len(), 2);
    assert!(
        spawned[1] < finished[0],
        "both executors were dispatched before either finished: spawned {spawned:?}, \
         finished {finished:?}"
    );

    // Each executor got its own id, and each `task` call its own result.
    let ids: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ExecutorSpawned { executor_id, .. } => Some(executor_id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec!["kimi-1".to_owned(), "kimi-2".to_owned()]);

    let results: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: true,
                output: Some(output),
                ..
            } => Some(output.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].contains("first report"), "{}", results[0]);
    assert!(results[1].contains("second report"), "{}", results[1]);

    fixture.harness.shutdown().await;
    // Neither executor's own turn is the session's product.
    assert_eq!(fixture.stdout.text(), "both executors reported\n");
}

#[tokio::test]
async fn the_batch_cap_bounds_how_many_executors_work_at_once() {
    // Two executors meet at the barrier; the third must wait for a free slot, so
    // its spawn is recorded after one of the pair has finished.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let task = |id: &str, brief: &str| StreamEvent::ToolCallCompleted {
        index: 0,
        id: id.to_owned(),
        name: "task".to_owned(),
        arguments: serde_json::json!({ "brief": brief }).to_string(),
    };
    let mut fixture = fixture(
        &[],
        vec![
            Reply::Stream(vec![
                task("call-1", "first"),
                task("call-2", "second"),
                task("call-3", "third"),
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::Meet(barrier.clone(), Box::new(Reply::text("first report"))),
            Reply::Meet(barrier.clone(), Box::new(Reply::text("second report"))),
            Reply::text("third report"),
            Reply::text("all three reported"),
        ],
        SessionConfig::new("fake-model").with_max_parallel_executors(2),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate three counts")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = fixture.events();
    let spawned: Vec<u64> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. }))
        .map(|event| event.seq)
        .collect();
    let finished: Vec<u64> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ExecutorFinished { .. }))
        .map(|event| event.seq)
        .collect();
    assert_eq!(spawned.len(), 3);
    assert_eq!(finished.len(), 3);
    assert!(spawned[1] < finished[0], "the first two ran together");
    assert!(
        finished[0] < spawned[2],
        "the third waited for a free slot: spawned {spawned:?}, finished {finished:?}"
    );

    let results: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: true,
                output: Some(output),
                ..
            } => Some(output.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 3, "{results:?}");
    // Results are recorded in the batch's order, whatever order they finished in.
    assert!(results[0].contains("first report"), "{}", results[0]);
    assert!(results[1].contains("second report"), "{}", results[1]);
    assert!(results[2].contains("third report"), "{}", results[2]);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_hook_that_stops_the_turn_still_gives_a_deferred_task_its_one_result() {
    let hook = support::ScriptedHook::new(
        vec![
            Ok(fs_agent::hooks::Constraint::Continue),
            Ok(fs_agent::hooks::Constraint::Stop),
        ],
        Vec::new(),
    );
    let mut fixture = fixture_with(
        &[],
        vec![Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call-1".to_owned(),
                name: "task".to_owned(),
                arguments: serde_json::json!({"brief": "count the tests"}).to_string(),
            },
            StreamEvent::ToolCallCompleted {
                index: 1,
                id: "call-2".to_owned(),
                name: "read_file".to_owned(),
                arguments: serde_json::json!({"file_path": "notes.txt"}).to_string(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ])],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
        Some(Arc::new(hook)),
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate it, then read")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Aborted);

    let events = fixture.events();
    // The deferred `task` was started on the stream before the hook stopped the
    // turn, so it is still owed exactly one result — and it never ran.
    let starts = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallStarted { .. }))
        .count();
    let results: Vec<(bool, String)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok, output, error, ..
            } => Some((
                *ok,
                output.clone().or_else(|| error.clone()).unwrap_or_default(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(starts, 2);
    assert_eq!(results.len(), 2, "every started call keeps its one result");
    assert!(results.iter().all(|(ok, _)| !ok));
    assert!(
        results
            .iter()
            .all(|(_, error)| error.contains("hook stopped the turn")),
        "{results:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. })),
        "the executor was never dispatched"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_denial_travels_down_to_the_executor() {
    let mut policy = Policy::for_mode(Mode::Auto);
    policy.push(Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        // `Deny` propagates by default (spec §12): it is a constraint, and a
        // constraint can only travel.
        Decision::Deny,
    ));
    let mut fixture = fixture(
        &[("plan.txt", "keep me\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "edit plan.txt"}),
            ),
            // The executor reads first, so the only thing standing between it and
            // the write is the inherited denial.
            calls(
                "exec-read",
                "read_file",
                serde_json::json!({"file_path": "plan.txt"}),
            ),
            calls(
                "exec-edit",
                "edit_file",
                serde_json::json!({"file_path": "plan.txt", "old_string": "keep me", "new_string": "changed"}),
            ),
            Reply::text("the write was refused"),
            Reply::text("the executor was refused"),
        ],
        SessionConfig::new("fake-model"),
        policy,
        None,
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate the edit").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = fixture.events();
    let decided = only_speaker(
        &events,
        &executor("kimi-1"),
        |payload| {
            matches!(
                payload,
                EventPayload::PermissionDecided {
                    decision: Decision::Deny,
                    ..
                }
            )
        },
        "the executor's refusal",
    );
    match &decided.payload {
        EventPayload::PermissionDecided {
            decision, reason, ..
        } => {
            assert_eq!(*decision, Decision::Deny);
            assert!(reason.as_deref().unwrap().contains("rule"), "{reason:?}");
        }
        other => panic!("expected PermissionDecided, got {other:?}"),
    }
    // `edit_file` asks about the write, so the executor never reached the tool.
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_allowance_does_not_travel_down_to_the_executor() {
    let mut policy = Policy::for_mode(Mode::Ask);
    // The dispatcher's own session-scoped allowance: `Allow` does not propagate,
    // and a headless session has nobody to ask, so the executor's write is
    // downgraded to a refusal rather than waved through.
    policy.push(Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        Decision::Allow,
    ));
    let mut fixture = fixture(
        &[("plan.txt", "keep me\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "edit plan.txt"}),
            ),
            calls(
                "exec-read",
                "read_file",
                serde_json::json!({"file_path": "plan.txt"}),
            ),
            calls(
                "exec-edit",
                "edit_file",
                serde_json::json!({"file_path": "plan.txt", "old_string": "keep me", "new_string": "changed"}),
            ),
            Reply::text("the write asked a question nobody answered"),
            Reply::text("the executor was refused"),
        ],
        SessionConfig::new("fake-model"),
        policy,
        None,
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate the edit").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = fixture.events();
    let denied: Vec<String> = events
        .iter()
        .filter(|event| event.speaker_id == executor("kimi-1"))
        .filter_map(|event| match &event.payload {
            EventPayload::PermissionDecided {
                decision: Decision::Deny,
                reason: Some(reason),
                ..
            } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(denied.len(), 1, "{denied:?}");
    assert!(
        denied[0].contains("no interactive answerer"),
        "the executor's mode asks, and nobody can answer: {}",
        denied[0]
    );
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_writes_under_the_dispatchers_auto_stance() {
    // `auto` means "writes are allowed", and the executor is doing the work in
    // the same session: an executor that could only read would make `task`
    // useless exactly where it is meant to run unattended. There is no asker
    // here, so an inherited `ask` would have refused the write outright.
    let mut fixture = fixture(
        &[("notes.txt", "before\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "rewrite notes.txt"}),
            ),
            calls(
                "exec-read",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            calls(
                "exec-edit",
                "edit_file",
                serde_json::json!({"file_path": "notes.txt", "old_string": "before", "new_string": "after"}),
            ),
            Reply::text("rewrote it"),
            Reply::text("done"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate the rewrite")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.read("notes.txt"), "after\n");
}

#[tokio::test]
async fn a_readonly_dispatcher_keeps_its_executor_read_only() {
    // A delegation is never the way around a hard stance. The asker approves
    // everything, so a mode that merely *asked* would still have let this through.
    let mut fixture = fixture(
        &[("notes.txt", "before\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "rewrite notes.txt"}),
            ),
            calls(
                "exec-read",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            calls(
                "exec-edit",
                "edit_file",
                serde_json::json!({"file_path": "notes.txt", "old_string": "before", "new_string": "after"}),
            ),
            Reply::text("I could not write"),
            Reply::text("the executor was refused"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Readonly),
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture
        .harness
        .run_turn("delegate the rewrite")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.read("notes.txt"), "before\n");
    let refused = fixture
        .events()
        .iter()
        .filter(|event| event.speaker_id == executor("kimi-1"))
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: false,
                error: Some(error),
                ..
            } => Some(error.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains("readonly"), "{}", refused[0]);
}

#[tokio::test]
async fn an_executor_model_override_routes_only_the_executor() {
    let mut fixture = fixture(
        &[],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "count the modules"}),
            ),
            Reply::text("12 modules"),
            Reply::text("done"),
        ],
        SessionConfig::new("dispatcher-model").with_executor_model("executor-model"),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    fixture.harness.run_turn("delegate it").await.unwrap();

    let requests = fixture.provider.requests();
    assert_eq!(requests[0].model, "dispatcher-model");
    assert_eq!(
        requests[1].model, "executor-model",
        "the executor is routed without moving the dispatcher"
    );
    assert_eq!(
        requests[2].model, "dispatcher-model",
        "and the dispatcher's own next call is untouched"
    );
}

#[tokio::test]
async fn the_report_names_the_file_a_rewriting_hook_actually_wrote() {
    // A `hook.pre` may rewrite the call after `ToolCallStarted` recorded what the
    // model asked for. The result is the only record of the file that was really
    // written, so that is what the report must name.
    let hook = support::ScriptedHook::new(
        vec![
            Ok(fs_agent::hooks::Constraint::Continue),
            Ok(fs_agent::hooks::Constraint::Rewrite(serde_json::json!({
                "file_path": "created.txt",
                "content": "from the hook\n"
            }))),
        ],
        vec![Ok(None), Ok(None)],
    );
    let mut fixture = fixture_with(
        &[],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "create a file"}),
            ),
            calls(
                "exec-write",
                "write_file",
                serde_json::json!({"file_path": "requested.txt", "content": "from the model\n"}),
            ),
            Reply::text("created it"),
            Reply::text("done"),
        ],
        SessionConfig::new("fake-model"),
        Policy::for_mode(Mode::Auto),
        None,
        Some(Arc::new(hook)),
    )
    .await;

    fixture.harness.run_turn("delegate it").await.unwrap();

    assert_eq!(fixture.read("created.txt"), "from the hook\n");
    assert!(!fixture.cwd.join("requested.txt").exists());
    let reported = fixture
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                ok: true,
                output: Some(output),
                ..
            } if event.speaker_id == kimi() => Some(output.clone()),
            _ => None,
        })
        .expect("the task call's result");
    assert!(reported.contains("created.txt"), "{reported}");
    assert!(!reported.contains("requested.txt"), "{reported}");
}

#[tokio::test]
async fn an_exhausted_session_dispatches_no_new_executor() {
    // The reply that asks for the executor already lands the whole allowance, so
    // the dispatch is refused. Because the call was started it still gets exactly
    // one result (invariant 1), and that result says the executor never ran — an
    // executor that was already running would have been left to finish (spec §17).
    let limit = 1_000;
    let mut fixture = fixture(
        &[],
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".into(),
                    name: "task".into(),
                    arguments: serde_json::json!({"brief": "count the modules"}).to_string(),
                },
                StreamEvent::Usage(Usage {
                    input_tokens: limit,
                    output_tokens: 0,
                    cached_tokens: 0,
                    miss_tokens: limit,
                    reasoning_tokens: None,
                }),
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("a reply the budget refuses to ask for"),
        ],
        SessionConfig::new("fake-model").with_session_token_limit(limit),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::BudgetExhausted);

    let events = fixture.events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. })),
        "an exhausted session dispatches no new executor"
    );
    let results: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(results.len(), 1, "the task call keeps its one result");
    match &results[0].payload {
        EventPayload::ToolCallCompleted { ok, error, .. } => {
            assert!(!ok);
            let error = error.as_deref().unwrap();
            assert!(error.contains("budget exhausted"), "{error}");
            assert!(error.contains("no new executor"), "{error}");
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
    assert_eq!(
        fixture.provider.requests().len(),
        1,
        "the turn stops rather than calling the provider again"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_already_running_finishes_even_when_the_allowance_is_gone() {
    // "No new executors, and the ones already running finish" (spec §17). The
    // executor's first call blows the session allowance and asks for a tool, so a
    // gated loop would abandon it on the next iteration; it must instead run to
    // its own turn cap, and the dispatcher's own turn is the one the hard stop
    // ends.
    let limit = 500;
    let mut fixture = fixture(
        &[("notes.txt", "the notes\n")],
        vec![
            // Inside the allowance, so the dispatch is allowed.
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".into(),
                    name: "task".into(),
                    arguments: serde_json::json!({"brief": "read the notes"}).to_string(),
                },
                StreamEvent::Usage(Usage {
                    input_tokens: 100,
                    output_tokens: 0,
                    cached_tokens: 0,
                    miss_tokens: 100,
                    reasoning_tokens: None,
                }),
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            // The executor's own call spends far past the cap and asks for a
            // tool, so its loop would iterate a second time.
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "exec-read".into(),
                    name: "read_file".into(),
                    arguments: "{\"file_path\":\"notes.txt\"}".into(),
                },
                StreamEvent::Usage(Usage {
                    input_tokens: 1_000,
                    output_tokens: 0,
                    cached_tokens: 0,
                    miss_tokens: 1_000,
                    reasoning_tokens: None,
                }),
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("read the notes"),
            Reply::text("a reply the dispatcher's budget refuses to ask for"),
        ],
        SessionConfig::new("fake-model").with_session_token_limit(limit),
        Policy::for_mode(Mode::Auto),
        None,
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate it").await.unwrap();

    let events = fixture.events();
    let finished = only(
        &events,
        |payload| matches!(payload, EventPayload::ExecutorFinished { .. }),
        "ExecutorFinished",
    );
    match &finished.payload {
        EventPayload::ExecutorFinished {
            reason, summary, ..
        } => {
            assert_eq!(
                *reason,
                StopReason::Completed,
                "an executor that was already running is left to finish"
            );
            assert_eq!(summary, "read the notes");
        }
        other => panic!("expected ExecutorFinished, got {other:?}"),
    }
    // Its spend still counts: the dispatcher's own next iteration is refused.
    assert_eq!(outcome.reason, StopReason::BudgetExhausted);
    let reported = only(
        &events,
        |payload| {
            matches!(
                payload,
                EventPayload::ToolCallCompleted {
                    ok: true,
                    output: Some(output),
                    ..
                } if output.starts_with("executor kimi-1 finished:")
            )
        },
        "task result",
    );
    match &reported.payload {
        EventPayload::ToolCallCompleted { output, .. } => {
            let output = output.as_deref().unwrap();
            assert!(
                output.contains("finished: Completed"),
                "the task reports the executor's own finish: {output}"
            );
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
    assert_eq!(
        fixture.provider.requests().len(),
        3,
        "the executor's two calls happened, the dispatcher's second did not"
    );

    fixture.harness.shutdown().await;
}
