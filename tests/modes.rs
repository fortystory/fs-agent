//! The permission modes end to end: which one a session starts in, what the
//! `Shift+Tab` gesture does to it, and what it refuses
//! (`.scratch/todo-and-modes/spec.md` §1).
//!
//! The gesture is a library call here because the key that presses it belongs to a
//! renderer; everything else is the same contract the other end-to-end tests use:
//! a scripted provider, a scripted answerer, and assertions on the JSONL stream
//! and the workspace.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, Decision, DecisionSource, Event, EventPayload, ParticipantId, SessionId,
    SpeakerId, StopReason,
};
use fs_agent::permissions::{Asker, Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    /// Kept alive for the duration of the test; `None` when the fixture
    /// continues a session whose directory an earlier fixture still owns.
    _dir: Option<tempfile::TempDir>,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, asker: Option<Arc<dyn Asker>>) -> Fixture {
    fixture_at(replies, mode, asker, None).await
}

/// Build a session, optionally continuing an existing log so a test can assert
/// what a `--continue` starts from.
async fn fixture_at(
    replies: Vec<Reply>,
    mode: Mode,
    asker: Option<Arc<dyn Asker>>,
    existing_log: Option<&std::path::Path>,
) -> Fixture {
    let dir = match existing_log {
        Some(_) => None,
        None => Some(tempfile::tempdir().unwrap()),
    };
    let (session, workspace) = match existing_log {
        Some(path) => (
            path.parent().unwrap().to_path_buf(),
            path.parent().unwrap().join("workspace"),
        ),
        None => {
            let root = dir.as_ref().unwrap().path();
            (root.join("session"), root.join("workspace"))
        }
    };
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);
    // The built-in table, `bash` included: an `Exclusive` call is what `readonly`
    // must refuse, and the real tool is the honest way to pin that.
    let tools = fs_agent::tools::builtin(false);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mode"),
            tools,
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(mode),
            asker,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        log_path,
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<(Decision, DecisionSource, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided {
                    decision,
                    source,
                    reason,
                    ..
                } => Some((*decision, *source, reason.clone())),
                _ => None,
            })
            .collect()
    }
}

fn tool_reply(id: &str, name: &str, arguments: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn write_reply(id: &str, file: &str) -> Reply {
    tool_reply(
        id,
        "write_file",
        &serde_json::json!({ "file_path": file, "content": "written\n" }).to_string(),
    )
}

// --- which mode a session runs under --------------------------------------

#[tokio::test]
async fn a_session_starts_in_the_mode_it_was_configured_with() {
    for mode in [Mode::Readonly, Mode::Ask, Mode::Auto] {
        let fixture = fixture(vec![], mode, Some(Arc::new(AlwaysAllow))).await;
        assert_eq!(fixture.harness.mode(), mode);
        fixture.harness.shutdown().await;
    }
}

#[tokio::test]
async fn cycling_moves_the_policy_and_writes_nothing_to_the_stream() {
    // The choice this pins: a mode is a session value, so the gesture appends no
    // event and injects no instruction — a line in the head of `messages` would
    // throw the prefix cache away on every press (ADR 0003). The audit reads
    // `PermissionDecided.reason` instead.
    let fixture = fixture(vec![], Mode::Readonly, Some(Arc::new(AlwaysAllow))).await;
    let before = fixture.events().len();

    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Ask);
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Auto);
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Readonly);
    assert_eq!(
        fixture.harness.mode(),
        Mode::Readonly,
        "three presses return the session to where it started"
    );
    assert_eq!(
        fixture.events().len(),
        before,
        "history is untouched by the gesture"
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_continue_returns_to_the_configured_mode() {
    // The mode does not survive a resume, because it is not in the stream: the
    // configured value is what a reopened session runs under (spec §12).
    let fixture = fixture(vec![], Mode::Readonly, Some(Arc::new(AlwaysAllow))).await;
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Ask);
    let log_path = fixture.log_path.clone();
    fixture.harness.shutdown().await;

    let resumed = fixture_at(
        vec![write_reply("call-notes", "notes.txt"), Reply::text("ok")],
        Mode::Auto,
        Some(Arc::new(AlwaysAllow)),
        Some(&log_path),
    )
    .await;
    assert_eq!(resumed.harness.mode(), Mode::Auto);

    // And the mode is not a field anywhere on the stream, either.
    for event in resumed.events() {
        let value = serde_json::to_value(&event).unwrap();
        assert!(
            !has_key_named_mode(&value),
            "no event carries the mode: {value}"
        );
    }
    resumed.harness.shutdown().await;
}

// --- what a mode refuses --------------------------------------------------

#[tokio::test]
async fn readonly_refuses_a_write_and_cycling_to_ask_lets_the_same_call_through() {
    let mut fixture = fixture(
        vec![
            write_reply("call-notes", "notes.txt"),
            Reply::text("blocked"),
            write_reply("call-again", "notes.txt"),
            Reply::text("wrote it"),
        ],
        Mode::Readonly,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();
    assert!(!fixture.exists("notes.txt"), "readonly denies the write");
    let refused = &fixture.decisions()[0];
    assert_eq!(refused.0, Decision::Deny);
    assert_eq!(refused.1, DecisionSource::Policy);
    assert!(
        refused.2.as_deref().unwrap().contains("readonly"),
        "the audit says which mode refused it: {:?}",
        refused.2
    );

    // One press moves `readonly` to `ask`, and the answerer approves: the very same
    // call now goes through. Nothing was injected in between — the gate read the new
    // stance because the mode is a value it reads per call.
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Ask);
    fixture.harness.run_turn("write it again").await.unwrap();
    assert_eq!(fixture.read("notes.txt"), "written\n");
    assert_eq!(
        fixture.decisions()[1].0,
        Decision::Allow,
        "the user's approval settles it in ask mode"
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_readonly_session_denies_a_shell_call_too() {
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-bash",
                "bash",
                &serde_json::json!({ "command": "echo hi > notes.txt" }).to_string(),
            ),
            Reply::text("I could not run it"),
        ],
        Mode::Readonly,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture.harness.run_turn("run it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert!(
        !fixture.exists("notes.txt"),
        "the denied shell never ran, so it wrote nothing"
    );
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_inherits_the_session_mode() {
    // The dispatcher is in `readonly` and everything is approved, so nothing but the
    // inherited mode can refuse the executor's write: dispatching is `ReadOnly`, and
    // the refusal has to come from the child's own policy.
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-task",
                "task",
                &serde_json::json!({"brief": "write notes.txt"}).to_string(),
            ),
            write_reply("exec-1", "notes.txt"),
            Reply::text("I could not write it"),
            Reply::text("the executor reported back"),
        ],
        Mode::Readonly,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert!(
        !fixture.exists("notes.txt"),
        "the executor inherits the mode"
    );

    let decisions = fixture.decisions();
    assert_eq!(
        decisions.len(),
        2,
        "the dispatch, then the executor's write"
    );
    assert_eq!(decisions[0].0, Decision::Allow, "dispatching is a read");
    assert_eq!(decisions[1].0, Decision::Deny);
    assert!(
        decisions[1].2.as_deref().unwrap().contains("readonly"),
        "the child's refusal is the session mode's: {:?}",
        decisions[1].2
    );

    // The refusal is attributed to the executor, not to the debater that dispatched
    // it.
    let speakers: Vec<SpeakerId> = fixture
        .events()
        .into_iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionDecided { .. }))
        .map(|event| event.speaker_id)
        .collect();
    assert_eq!(
        speakers,
        vec![
            SpeakerId::Debater("kimi".into()),
            SpeakerId::Executor(ParticipantId::new("kimi-1")),
        ]
    );
    fixture.harness.shutdown().await;
}

/// Whether any object anywhere in this JSON has a key named `mode`.
fn has_key_named_mode(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.contains_key("mode") || map.values().any(has_key_named_mode)
        }
        serde_json::Value::Array(items) => items.iter().any(has_key_named_mode),
        _ => false,
    }
}
