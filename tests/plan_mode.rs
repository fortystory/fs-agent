//! Plan mode end to end (ticket 15): the gesture that enters it, the one write
//! it allows, and what leaving it restores.
//!
//! The gestures are library calls here because the key that presses them belongs
//! to a renderer (ticket 18). Everything else is the same contract the other
//! end-to-end tests use: a scripted provider, a scripted answerer, and
//! assertions on the JSONL stream and the workspace.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, ContextSource, Decision, DecisionSource, Event, EventPayload, ParticipantId,
    SessionId, SpeakerId, StopReason,
};
use fs_agent::permissions::{Asker, Mode, PlanConflict, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::RenderSinks;
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply, ScriptedAsker};

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
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
    // The built-in table, `bash` included: an `Exclusive` call is what plan mode
    // must refuse, and the real tool is the honest way to pin that.
    let tools = fs_agent::tools::builtin();

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        sinks: RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        },
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-plan"),
            tools,
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(mode),
            asker,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        provider,
        log_path,
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.workspace.join(name), content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    /// Every pinned injection the stream carries, in order.
    fn injections(&self) -> Vec<(ContextSource, String)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ContextInjected { source, content } => {
                    Some((*source, content.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn plan_instruction(&self) -> String {
        self.injections()
            .into_iter()
            .find(|(source, _)| *source == ContextSource::PlanMode)
            .expect("the session carries a plan-mode instruction")
            .1
    }

    /// Every `seq` a `HistorySuperseded` retired, in event order.
    fn retired(&self) -> Vec<u64> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::HistorySuperseded { targets, .. } => Some(targets.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// `(tool_call_id, ok)` for every completed call, in stream order.
    fn results(&self) -> Vec<(String, bool)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    tool_call_id, ok, ..
                } => Some((tool_call_id.as_str().to_owned(), *ok)),
                _ => None,
            })
            .collect()
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

// A test-only stand-in for the shape plan mode must refuse is no longer needed:
// ticket 20 landed the real `bash` tool, whose `effect()` is `Exclusive`. The
// e2e below drives it, so the rule is pinned by the tool the session ships.

// --- the gesture (spec §13) -----------------------------------------------

#[tokio::test]
async fn entering_plan_mode_switches_the_policy_and_injects_one_instruction() {
    let mut fixture = fixture(vec![], Mode::Ask, Some(Arc::new(AlwaysAllow))).await;

    let conflict = fixture.harness.enter_plan_mode().await.unwrap();

    assert_eq!(
        conflict, None,
        "there is no PLAN.md yet, so there is nothing to ask about"
    );
    assert_eq!(fixture.harness.mode(), Mode::Plan);
    let injections = fixture.injections();
    assert_eq!(injections.len(), 1, "one instruction, recorded once");
    assert_eq!(injections[0].0, ContextSource::PlanMode);
    assert!(
        injections[0].1.contains("PLAN.md"),
        "the instruction points at the file: {}",
        injections[0].1
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn entering_plan_mode_twice_switches_once_and_injects_once() {
    let mut fixture = fixture(vec![], Mode::Ask, Some(Arc::new(AlwaysAllow))).await;

    fixture.harness.enter_plan_mode().await.unwrap();
    let second = fixture.harness.enter_plan_mode().await.unwrap();

    assert_eq!(second, None);
    assert_eq!(fixture.harness.mode(), Mode::Plan);
    assert_eq!(
        fixture.injections().len(),
        1,
        "re-entering does not re-inject: the instruction is already pinned"
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn leaving_plan_mode_restores_the_mode_and_retires_the_instruction() {
    let mut fixture = fixture(
        vec![
            write_reply("call-notes", "notes.txt"),
            Reply::text("wrote it"),
        ],
        Mode::Auto,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    assert!(
        !fixture.harness.exit_plan_mode().await.unwrap(),
        "leaving a mode the session is not in changes nothing"
    );

    fixture.harness.enter_plan_mode().await.unwrap();
    assert_eq!(fixture.harness.mode(), Mode::Plan);

    assert!(
        fixture.harness.exit_plan_mode().await.unwrap(),
        "the gesture left plan mode"
    );
    assert_eq!(
        fixture.harness.mode(),
        Mode::Auto,
        "leaving restores the mode the session was configured with, not a default"
    );

    // The instruction is retired, not contradicted: history keeps the record,
    // and the `HistorySuperseded` takes it out of effect (spec §2, §13).
    assert_eq!(fixture.injections().len(), 1, "history is append-only");
    let entry = fixture.plan_instruction();
    assert!(
        !entry.is_empty(),
        "the entry instruction was recorded before it was retired"
    );
    assert_eq!(
        fixture.retired().len(),
        1,
        "exactly one injection was retired"
    );

    // So the next request no longer tells the model it may not write — while the
    // gate lets the write through, which is the whole point of leaving.
    fixture.harness.run_turn("write it").await.unwrap();
    assert_eq!(fixture.read("notes.txt"), "written\n");
    assert!(
        !carries_plan_instruction(&fixture.provider.requests()[0]),
        "the model is not told it is still in plan mode: {:?}",
        fixture.provider.requests()[0].messages
    );
    fixture.harness.shutdown().await;
}

// --- an existing plan file (spec §13) -------------------------------------

#[tokio::test]
async fn an_existing_plan_file_is_put_to_the_user_before_the_mode_changes() {
    let asker = Arc::new(ScriptedAsker::with_conflicts(
        vec![],
        vec![PlanConflict::Append],
    ));
    let mut fixture = fixture(vec![], Mode::Ask, Some(asker.clone())).await;
    fixture.write("PLAN.md", "# old plan\n");

    let conflict = fixture.harness.enter_plan_mode().await.unwrap();

    assert_eq!(conflict, Some(PlanConflict::Append));
    assert_eq!(fixture.harness.mode(), Mode::Plan);
    assert_eq!(
        fixture.read("PLAN.md"),
        "# old plan\n",
        "append keeps the file"
    );
    assert_eq!(
        asker.conflict_paths(),
        vec![fixture.workspace.join("PLAN.md")],
        "the question names the file it is about"
    );
    assert!(
        fixture.plan_instruction().contains("追加"),
        "the instruction carries the answer: {}",
        fixture.plan_instruction()
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn overwrite_clears_the_plan_file_the_model_could_not_clobber_itself() {
    let asker = ScriptedAsker::with_conflicts(vec![], vec![PlanConflict::Overwrite]);
    let mut fixture = fixture(vec![], Mode::Ask, Some(Arc::new(asker))).await;
    fixture.write("PLAN.md", "# old plan\n");

    let conflict = fixture.harness.enter_plan_mode().await.unwrap();

    assert_eq!(conflict, Some(PlanConflict::Overwrite));
    assert_eq!(
        fixture.read("PLAN.md"),
        "",
        "the CLI clears it: read-before-write would refuse to clobber it"
    );
    assert!(
        !fixture.plan_instruction().contains("已经存在"),
        "the model is not told about a file that is no longer there"
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn keep_leaves_the_file_and_treats_it_as_the_plan_already_in_force() {
    let asker = ScriptedAsker::with_conflicts(vec![], vec![PlanConflict::Keep]);
    let mut fixture = fixture(vec![], Mode::Ask, Some(Arc::new(asker))).await;
    fixture.write("PLAN.md", "# old plan\n");

    let conflict = fixture.harness.enter_plan_mode().await.unwrap();

    assert_eq!(conflict, Some(PlanConflict::Keep));
    assert_eq!(fixture.read("PLAN.md"), "# old plan\n");
    assert!(
        fixture.plan_instruction().contains("现行计划"),
        "keep and append are the same file on disk, so the instruction is what tells them apart: {}",
        fixture.plan_instruction()
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_session_with_no_answerer_never_clears_the_users_file() {
    // A gesture only happens interactively, but a script can call one; with
    // nobody to ask, the session picks the answer that destroys nothing.
    let mut fixture = fixture(vec![], Mode::Ask, None).await;
    fixture.write("PLAN.md", "# old plan\n");

    let conflict = fixture.harness.enter_plan_mode().await.unwrap();

    assert_eq!(conflict, Some(PlanConflict::Keep));
    assert_eq!(fixture.harness.mode(), Mode::Plan);
    assert_eq!(fixture.read("PLAN.md"), "# old plan\n");
    fixture.harness.shutdown().await;
}

// --- the mode as the loop sees it -----------------------------------------

#[tokio::test]
async fn a_turn_in_plan_mode_writes_the_plan_file_and_nothing_else() {
    let mut fixture = fixture(
        vec![
            write_reply("call-notes", "notes.txt"),
            write_reply("call-plan", "PLAN.md"),
            tool_reply(
                "call-bash",
                "bash",
                &serde_json::json!({ "command": "echo hi > notes.txt" }).to_string(),
            ),
            Reply::text("planned"),
        ],
        Mode::Ask,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;
    fixture.harness.enter_plan_mode().await.unwrap();

    let outcome = fixture.harness.run_turn("plan it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    // The instruction reaches the model as its own pinned message: it was
    // injected after history existed, so it stays separate rather than merging
    // into the leading block (spec §13).
    let request = &fixture.provider.requests()[0];
    assert!(
        matches!(
            &request.messages[0],
            fs_agent::provider::Message::User { injected: true, .. }
        ),
        "the instruction leads this request: {:?}",
        request.messages
    );
    assert!(matches!(
        &request.messages[1],
        fs_agent::provider::Message::User {
            injected: false,
            ..
        }
    ));

    assert!(!fixture.exists("notes.txt"), "an ordinary write is denied");
    assert_eq!(fixture.read("PLAN.md"), "written\n", "the plan file is not");
    assert_eq!(
        fixture.results(),
        vec![
            ("call-notes".to_owned(), false),
            ("call-plan".to_owned(), true),
            ("call-bash".to_owned(), false),
        ],
        "every call gets exactly one result, and only the plan write ran"
    );

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 3);
    assert_eq!(decisions[0].0, Decision::Deny);
    assert_eq!(decisions[0].1, DecisionSource::Policy);
    assert!(
        decisions[0].2.as_deref().unwrap().contains("plan"),
        "the audit says which mode refused it: {:?}",
        decisions[0].2
    );
    assert_eq!(decisions[1].0, Decision::Allow);
    assert_eq!(
        decisions[2].0,
        Decision::Deny,
        "the real `bash` is Exclusive, so it cannot borrow the write exemption"
    );
    assert!(!fixture.exists("notes.txt"), "the denied shell never ran");
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn leaving_plan_mode_lets_the_same_call_through_again() {
    let mut fixture = fixture(
        vec![
            write_reply("call-notes", "notes.txt"),
            Reply::text("wrote it"),
        ],
        Mode::Ask,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;
    fixture.harness.enter_plan_mode().await.unwrap();
    assert!(fixture.harness.exit_plan_mode().await.unwrap());

    fixture.harness.run_turn("write it").await.unwrap();

    assert_eq!(
        fixture.read("notes.txt"),
        "written\n",
        "leaving restores the mode the session was configured with"
    );
    assert_eq!(fixture.decisions()[0].0, Decision::Allow);
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_dispatched_in_plan_mode_cannot_write_either() {
    // The dispatcher starts in `auto` and approves everything, so nothing but
    // the inherited mode can refuse the executor's write: dispatching is
    // `ReadOnly`, and the refusal has to come from the child's own policy.
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
        Mode::Auto,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;
    fixture.harness.enter_plan_mode().await.unwrap();

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
        decisions[1].2.as_deref().unwrap().contains("plan"),
        "the child's refusal is the plan mode's: {:?}",
        decisions[1].2
    );

    // The refusal is attributed to the executor, not to the debater that
    // dispatched it.
    let executors: Vec<SpeakerId> = fixture
        .events()
        .into_iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionDecided { .. }))
        .map(|event| event.speaker_id)
        .collect();
    assert_eq!(
        executors,
        vec![
            SpeakerId::Debater("kimi".into()),
            SpeakerId::Executor(ParticipantId::new("kimi-1")),
        ]
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn the_mode_is_a_session_value_and_a_continue_repairs_a_stale_instruction() {
    let mut fixture = fixture(vec![], Mode::Ask, Some(Arc::new(AlwaysAllow))).await;
    fixture.harness.enter_plan_mode().await.unwrap();
    let log_path = fixture.log_path.clone();
    fixture.harness.shutdown().await;

    // A resumed session is assembled with the configured mode; nothing in the
    // stream can bring plan mode back (spec §12).
    let mut resumed = fixture_at(
        vec![write_reply("call-notes", "notes.txt"), Reply::text("ok")],
        Mode::Auto,
        Some(Arc::new(AlwaysAllow)),
        Some(&log_path),
    )
    .await;
    assert_eq!(resumed.harness.mode(), Mode::Auto);
    assert_eq!(
        resumed.injections().len(),
        1,
        "resuming re-records no session skeleton, and records no new instruction"
    );

    // The killed process's instruction was still live, so the resume retires it:
    // otherwise the model would be told it may not write while the gate allows
    // it (spec §13).
    assert_eq!(
        resumed.retired().len(),
        1,
        "the stale instruction was retired"
    );
    resumed.harness.run_turn("write it").await.unwrap();
    assert_eq!(resumed.read("notes.txt"), "written\n");
    assert!(
        !carries_plan_instruction(&resumed.provider.requests()[0]),
        "the resumed session does not replay a plan instruction: {:?}",
        resumed.provider.requests()[0].messages
    );

    // The mode is not a field anywhere on the stream: it is a session value, and
    // `PermissionDecided` is what an audit reads instead.
    for event in resumed.events() {
        let value = serde_json::to_value(&event).unwrap();
        assert!(
            !has_key_named_mode(&value),
            "no event carries the mode: {value}"
        );
    }
    resumed.harness.shutdown().await;
}

/// Whether the request the model received still carries a live plan-mode
/// instruction, read off the projected messages rather than the raw log: a
/// retired injection stays in the log and leaves the request (spec §2, §13).
fn carries_plan_instruction(request: &fs_agent::provider::ChatRequest) -> bool {
    request.messages.iter().any(|message| {
        matches!(
            message,
            fs_agent::provider::Message::User { content, injected: true, .. }
                if content.contains("PLAN.md")
        )
    })
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
