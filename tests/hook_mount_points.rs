//! Hook mount points at the assembly seam.
//!
//! Every test here drives the one assembly seam with a scripted provider, a
//! scripted hook and (where a question can happen) a scripted answerer, then
//! asserts the JSONL stream, the workspace and the two sinks. The seven scripted
//! scenarios are the prototype's seven, turned into tests one for one
//! (`prototype/05-hook-permission-ordering.html`); the rest cover the exception
//! paths the ticket names but the prototype did not script.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use fs_agent::config::SessionConfig;
use fs_agent::events::{
    pending_tool_calls, read_events, Decision, DecisionSource, Event, EventPayload, SessionId,
    SpeakerId, StopReason,
};
use fs_agent::hooks::{Constraint, Hook, HookError, Tightening};
use fs_agent::permissions::{Answer, Asker, Mode, Policy, Rule};
use fs_agent::provider::{ChatRequest, FinishReason, Message, StreamEvent, ToolSpec};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{Effect, Tool, ToolContext, ToolError, ToolOutput};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::Value;
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply, ScriptedAsker, ScriptedHook};

struct Fixture {
    /// `Option` so a test can shut the renderer down (flushing the sinks) and
    /// still keep using the fixture for assertions.
    harness: Option<Harness>,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    workspace: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(
    replies: Vec<Reply>,
    mode: Mode,
    rules: Vec<Rule>,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn Hook>>,
) -> Fixture {
    fixture_with_tools(replies, mode, rules, asker, hook, Vec::new()).await
}

async fn fixture_with_tools(
    replies: Vec<Reply>,
    mode: Mode,
    rules: Vec<Rule>,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn Hook>>,
    extra_tools: Vec<Box<dyn Tool>>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();

    let mut session_policy = Policy::for_mode(mode);
    for rule in rules {
        session_policy.push(rule);
    }

    let mut tools = fs_agent::tools::builtin(false);
    for tool in extra_tools {
        tools.register(tool);
    }

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-hooks"),
            tools,
            locks: fs_agent::tools::PathLocks::new(),
            policy: session_policy,
            asker,
            questions: None,
            hook,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness: Some(harness),
        provider,
        stdout,
        stderr,
        log_path,
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    /// Record a user message and run one turn to completion.
    async fn run_turn(&mut self, input: &str) -> fs_agent::agent::TurnOutcome {
        self.harness
            .as_mut()
            .expect("harness already shut down")
            .run_turn(input)
            .await
            .unwrap()
    }

    /// Drop the render channel and wait for the sinks to drain, so stderr can be
    /// asserted on.
    async fn shutdown(&mut self) {
        if let Some(harness) = self.harness.take() {
            harness.shutdown().await;
        }
    }

    fn write(&self, name: &str, content: &str) {
        let path = self.workspace.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
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

    fn kinds(&self) -> Vec<&'static str> {
        self.events()
            .iter()
            .map(|event| event.payload.kind())
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

    fn asked_count(&self) -> usize {
        self.events()
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::PermissionAsked { .. }))
            .count()
    }

    fn results(&self) -> Vec<(String, bool, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    ok,
                    error,
                    ..
                } => Some((tool_call_id.as_str().to_owned(), *ok, error.clone())),
                _ => None,
            })
            .collect()
    }

    /// Every hook execution as `(point, outcome)`.
    fn hook_events(&self) -> Vec<(String, String)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::HookExecuted { point, outcome, .. } => {
                    Some((point.clone(), outcome.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn pending(&self) -> Vec<String> {
        pending_tool_calls(&self.events())
            .into_iter()
            .map(|id| id.as_str().to_owned())
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

fn read_reply(id: &str, file: &str) -> Reply {
    tool_reply(
        id,
        "read_file",
        &serde_json::json!({ "file_path": file }).to_string(),
    )
}

fn write_reply(id: &str, file: &str) -> Reply {
    tool_reply(
        id,
        "write_file",
        &serde_json::json!({ "file_path": file, "content": "written\n" }).to_string(),
    )
}

fn tool_message_contents(request: &ChatRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// A test-only command tool whose argv hits the `rm` circuit breaker.
struct RmRoot;

#[async_trait]
impl Tool for RmRoot {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run".to_owned(),
            description: "test-only command tool".to_owned(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::Exclusive
    }

    fn command(&self, _args: &Value) -> Option<Vec<String>> {
        Some(vec!["rm".to_owned(), "-rf".to_owned(), "/".to_owned()])
    }

    async fn call(&self, _ctx: &ToolContext<'_>, _args: Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::new(
            "ran: the breaker should have prevented this",
        ))
    }
}

// --- the prototype's seven scenarios --------------------------------------

#[tokio::test]
async fn scenario_1_a_clean_call_in_auto_records_the_full_order() {
    let hook = ScriptedHook::continuing();
    let mut fixture = fixture(
        vec![read_reply("call-read", "notes.txt"), Reply::text("done")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;
    fixture.write("notes.txt", "hello\n");

    fixture.run_turn("read it").await;
    fixture.shutdown().await;

    assert_eq!(
        fixture.kinds(),
        vec![
            "SessionStarted",
            "MessageCompleted",
            "TurnStarted",
            "MessageCompleted",
            "ToolCallStarted",
            "HookExecuted",
            "PermissionDecided",
            "ToolCallCompleted",
            "HookExecuted",
            "TurnStarted",
            "UsageRecorded",
            "MessageCompleted",
            "TurnEnded",
        ],
        "the pre-hook runs before the gate and the post-hook after the result"
    );
    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Allow);
    assert_eq!(
        decisions[0].1,
        DecisionSource::Policy,
        "no hook and no rule: the policy is the source"
    );
    assert_eq!(
        fixture.hook_events(),
        vec![
            ("pre_tool_use".to_owned(), "continue".to_owned()),
            ("post_tool_use".to_owned(), "continue".to_owned()),
        ]
    );
    assert!(fixture.pending().is_empty());
}

#[tokio::test]
async fn scenario_2_a_hook_can_tighten_to_ask_and_the_question_happens() {
    let hook = ScriptedHook::new(
        vec![Ok(Constraint::Tighten(Tightening::Ask))],
        vec![Ok(None)],
    );
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "hello.txt"), Reply::text("ok")],
        Mode::Auto,
        vec![],
        Some(Arc::new(asker.clone())),
        Some(Arc::new(hook.clone())),
    )
    .await;

    fixture.run_turn("write it").await;
    fixture.shutdown().await;

    // `auto` would have allowed this; the hook alone caused the ask.
    assert_eq!(fixture.read("hello.txt"), "written\n");
    assert_eq!(fixture.asked_count(), 1, "the tightening to ask was real");
    assert_eq!(asker.requests().len(), 1);
    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Allow);
    assert_eq!(decisions[0].1, DecisionSource::User);
    assert!(fixture
        .hook_events()
        .contains(&("pre_tool_use".to_owned(), "tighten:ask".to_owned())));
}

#[tokio::test]
async fn scenario_3_a_gate_deny_yields_one_error_result_and_the_tool_never_runs() {
    let hook = ScriptedHook::new(vec![Ok(Constraint::Continue)], vec![]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "notes.txt"), Reply::text("ok")],
        Mode::Readonly,
        vec![],
        // An answerer proves this is a deny, not an ask.
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;

    fixture.run_turn("write it").await;
    fixture.shutdown().await;

    assert!(!fixture.exists("notes.txt"), "the tool never ran");
    let results = fixture.results();
    assert_eq!(results.len(), 1, "exactly one synthesized result");
    assert!(!results[0].1);
    assert!(results[0].2.clone().unwrap().contains("permission denied"));
    assert_eq!(fixture.asked_count(), 0);
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    assert_eq!(fixture.decisions()[0].1, DecisionSource::Policy);
    assert!(fixture.pending().is_empty());
}

#[tokio::test]
async fn scenario_4_a_hook_can_stop_an_ask_from_happening() {
    let hook = ScriptedHook::new(vec![Ok(Constraint::Tighten(Tightening::Deny))], vec![]);
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "notes.txt"), Reply::text("ok")],
        Mode::Ask,
        vec![],
        Some(Arc::new(asker.clone())),
        Some(Arc::new(hook.clone())),
    )
    .await;

    fixture.run_turn("write it").await;
    fixture.shutdown().await;

    assert!(!fixture.exists("notes.txt"));
    assert_eq!(
        fixture.asked_count(),
        0,
        "the tighten to deny happened before the ask, so the question never existed"
    );
    assert_eq!(asker.requests().len(), 0);
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    assert_eq!(
        fixture.decisions()[0].1,
        DecisionSource::Hook,
        "the hook raised the gate's ask to a deny"
    );
    assert!(fixture.pending().is_empty());
}

#[tokio::test]
async fn scenario_5_a_pre_hook_failure_is_fail_closed_and_the_turn_continues() {
    let hook = ScriptedHook::new(vec![Err(HookError::Failed("exit 2".to_owned()))], vec![]);
    let mut fixture = fixture(
        vec![
            write_reply("call-write", "notes.txt"),
            Reply::text("recovered"),
        ],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;

    let outcome = fixture.run_turn("write it").await;
    fixture.shutdown().await;

    assert!(
        !fixture.exists("notes.txt"),
        "fail-closed: the tool never ran"
    );
    let results = fixture.results();
    assert_eq!(results.len(), 1, "the invariant survives the failure");
    assert!(!results[0].1);
    assert!(results[0].2.clone().unwrap().contains("hook failed"));
    assert_eq!(
        fixture.hook_events(),
        vec![("pre_tool_use".to_owned(), "failed: exit 2".to_owned())]
    );
    assert!(
        fixture.stderr.text().contains("hook.pre failed"),
        "the failure is diagnosed: {}",
        fixture.stderr.text()
    );
    assert_eq!(
        fixture.provider.requests().len(),
        2,
        "a broken hook does not kill the turn; the model sees the error result"
    );
    assert_eq!(outcome.reason, StopReason::Completed);
    assert!(fixture.pending().is_empty());
}

#[tokio::test]
async fn scenario_6_post_hook_feedback_is_merged_into_the_tool_message() {
    let hook = ScriptedHook::new(
        vec![Ok(Constraint::Continue)],
        vec![Ok(Some("cargo test failed: 3 tests".to_owned()))],
    );
    let mut fixture = fixture(
        vec![read_reply("call-read", "notes.txt"), Reply::text("done")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;
    fixture.write("notes.txt", "hello\n");

    fixture.run_turn("read it").await;
    fixture.shutdown().await;

    // The stream keeps two events: the result and its annotation.
    assert_eq!(
        fixture.hook_events(),
        vec![
            ("pre_tool_use".to_owned(), "continue".to_owned()),
            (
                "post_tool_use".to_owned(),
                "feedback: cargo test failed: 3 tests".to_owned()
            ),
        ]
    );
    assert_eq!(
        fixture
            .kinds()
            .iter()
            .filter(|kind| **kind == "ToolCallCompleted")
            .count(),
        1
    );

    // The projection merges them into the one `tool` message the provider allows
    // for that call, in the second request.
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 2);
    let tool_messages = tool_message_contents(&requests[1]);
    assert_eq!(tool_messages.len(), 1);
    let merged = &tool_messages[0];
    assert!(
        merged.contains("hello"),
        "the result is still there: {merged}"
    );
    assert!(
        merged.contains("[hook feedback] cargo test failed: 3 tests"),
        "the feedback was merged into the same message: {merged}"
    );
}

#[tokio::test]
async fn scenario_7_a_skipped_call_still_gets_exactly_one_result() {
    let hook = ScriptedHook::new(vec![Ok(Constraint::Skip)], vec![]);
    let mut fixture = fixture(
        vec![read_reply("call-read", "notes.txt"), Reply::text("done")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;
    fixture.write("notes.txt", "hello\n");

    fixture.run_turn("read it").await;
    fixture.shutdown().await;

    let results = fixture.results();
    assert_eq!(
        results.len(),
        1,
        "the tool did not run, but a result exists"
    );
    assert!(!results[0].1);
    assert!(results[0].2.clone().unwrap().contains("hook skipped"));
    assert_eq!(
        fixture.hook_events(),
        vec![("pre_tool_use".to_owned(), "skip".to_owned())],
        "a skip is not a dispatch, so no post-hook runs"
    );
    assert!(fixture.pending().is_empty());
}

// --- the exception paths and closure the ticket also names ----------------

#[tokio::test]
async fn a_pre_hook_timeout_is_fail_closed_too() {
    // The ticket groups "failure" and "timeout": the port has a `Timeout` case,
    // and the loop must treat it exactly like any other pre-hook failure.
    let hook = ScriptedHook::new(vec![Err(HookError::Timeout)], vec![]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "notes.txt"), Reply::text("ok")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;

    fixture.run_turn("write it").await;
    fixture.shutdown().await;

    assert!(
        !fixture.exists("notes.txt"),
        "fail-closed: the tool never ran"
    );
    assert_eq!(
        fixture.hook_events(),
        vec![(
            "pre_tool_use".to_owned(),
            "failed: hook timed out".to_owned()
        )]
    );
    let results = fixture.results();
    assert_eq!(results.len(), 1);
    assert!(!results[0].1);
    assert!(results[0].2.clone().unwrap().contains("hook timed out"));
    assert!(fixture.pending().is_empty());
}

#[tokio::test]
async fn a_pre_hook_rewrites_the_arguments_the_gate_and_the_tool_see() {
    let rewritten = serde_json::json!({ "file_path": ".env", "content": "leak\n" });
    let hook = ScriptedHook::new(vec![Ok(Constraint::Rewrite(rewritten.clone()))], vec![]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "notes.txt"), Reply::text("ok")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;

    fixture.run_turn("write it").await;
    fixture.shutdown().await;

    // The hook saw the call as the model made it...
    assert_eq!(
        hook.pre_calls()[0].args,
        serde_json::json!({ "file_path": "notes.txt", "content": "written\n" })
    );
    // ...but the gate judged the rewritten call, so the `.env` floor denied it.
    assert!(!fixture.exists("notes.txt"));
    assert!(!fixture.exists(".env"));
    let results = fixture.results();
    assert_eq!(results.len(), 1);
    assert!(results[0].2.clone().unwrap().contains("permission denied"));
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    assert_eq!(fixture.decisions()[0].1, DecisionSource::Policy);
    assert!(fixture
        .hook_events()
        .contains(&("pre_tool_use".to_owned(), "rewrite".to_owned())));
}

#[tokio::test]
async fn a_hook_cannot_relax_the_rm_circuit_breaker() {
    // The gate would allow in `auto`, and the hook only asks — but the breaker's
    // deny is the supremum, so it stands and no question is asked.
    let hook = ScriptedHook::new(vec![Ok(Constraint::Tighten(Tightening::Ask))], vec![]);
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture_with_tools(
        vec![tool_reply("call-rm", "run", "{}"), Reply::text("ok")],
        Mode::Auto,
        vec![],
        Some(Arc::new(asker.clone())),
        Some(Arc::new(hook.clone())),
        vec![Box::new(RmRoot)],
    )
    .await;

    fixture.run_turn("run it").await;
    fixture.shutdown().await;

    let results = fixture.results();
    assert_eq!(results.len(), 1);
    assert!(
        results[0].2.clone().unwrap().contains("circuit breaker"),
        "{:?}",
        results[0].2
    );
    assert_eq!(
        fixture.asked_count(),
        0,
        "a deny is never downgraded to an ask"
    );
    assert_eq!(asker.requests().len(), 0);
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    assert_eq!(fixture.decisions()[0].1, DecisionSource::Policy);
    assert!(fixture.pending().is_empty());
}

#[tokio::test]
async fn a_stopped_turn_still_leaves_exactly_one_result() {
    let hook = ScriptedHook::new(vec![Ok(Constraint::Stop)], vec![]);
    let mut fixture = fixture(
        vec![
            write_reply("call-write", "notes.txt"),
            Reply::text("never asked"),
        ],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;

    let outcome = fixture.run_turn("write it").await;
    fixture.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert!(!fixture.exists("notes.txt"));
    let kinds = fixture.kinds();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "ToolCallStarted")
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "ToolCallCompleted")
            .count(),
        1,
        "the started call is owed a result even when the hook stops the turn"
    );
    assert!(fixture.pending().is_empty());
    assert!(fixture.results()[0]
        .2
        .clone()
        .unwrap()
        .contains("hook stopped the turn"));
}

#[tokio::test]
async fn every_call_in_a_batch_gets_exactly_one_result_across_the_exception_paths() {
    let batch = Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: "call-denied".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({ "file_path": "a.txt", "content": "a\n" }).to_string(),
        },
        StreamEvent::ToolCallCompleted {
            index: 1,
            id: "call-skipped".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({ "file_path": "b.txt", "content": "b\n" }).to_string(),
        },
        StreamEvent::ToolCallCompleted {
            index: 2,
            id: "call-run".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({ "file_path": "c.txt", "content": "c\n" }).to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ]);
    let hook = ScriptedHook::new(
        vec![
            Ok(Constraint::Tighten(Tightening::Deny)),
            Ok(Constraint::Skip),
            Ok(Constraint::Continue),
        ],
        vec![Ok(None)],
    );
    let mut fixture = fixture(
        vec![batch, Reply::text("ok")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;

    fixture.run_turn("write three").await;
    fixture.shutdown().await;

    // Denied and skipped calls never ran; the continued one did.
    assert!(!fixture.exists("a.txt"));
    assert!(!fixture.exists("b.txt"));
    assert_eq!(fixture.read("c.txt"), "c\n");

    let kinds = fixture.kinds();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "ToolCallStarted")
            .count(),
        3
    );
    let results = fixture.results();
    assert_eq!(results.len(), 3, "one per started call, no more, no fewer");
    assert!(fixture.pending().is_empty());
    assert_eq!(fixture.hook_events().len(), 4, "three pre, one post");
}

#[tokio::test]
async fn a_post_hook_failure_only_drops_feedback() {
    let hook = ScriptedHook::new(
        vec![Ok(Constraint::Continue)],
        vec![Err(HookError::Failed("post exploded".to_owned()))],
    );
    let mut fixture = fixture(
        vec![read_reply("call-read", "notes.txt"), Reply::text("done")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;
    fixture.write("notes.txt", "hello\n");

    fixture.run_turn("read it").await;
    fixture.shutdown().await;

    // The world already changed: the successful result stands.
    let results = fixture.results();
    assert_eq!(results.len(), 1);
    assert!(results[0].1, "{:?}", results[0].2);
    assert_eq!(
        fixture.hook_events(),
        vec![
            ("pre_tool_use".to_owned(), "continue".to_owned()),
            (
                "post_tool_use".to_owned(),
                "failed: post exploded".to_owned()
            ),
        ]
    );
    assert!(fixture.stderr.text().contains("hook.post failed"));

    // The failure is not merged into the model's tool message: only feedback is.
    let requests = fixture.provider.requests();
    let merged = tool_message_contents(&requests[1]).join("\n");
    assert!(!merged.contains("post exploded"), "{merged}");
    assert!(!merged.contains("[hook feedback]"), "{merged}");
}

#[tokio::test]
async fn the_hook_sees_only_the_closed_public_subset() {
    let hook = ScriptedHook::continuing();
    let mut fixture = fixture(
        vec![read_reply("call-read", "notes.txt"), Reply::text("done")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        Some(Arc::new(hook.clone())),
    )
    .await;
    fixture.write("notes.txt", "hello\n");

    fixture.run_turn("read it").await;
    fixture.shutdown().await;

    // Before the gate: the session frame and the call. Never the user message,
    // the turn frame or an assistant message.
    assert_eq!(
        hook.pre_calls()[0].history,
        vec!["SessionStarted", "ToolCallStarted"]
    );
    // After the result: the permission verdict and the result, still no message.
    assert_eq!(
        hook.post_calls()[0].history,
        vec![
            "SessionStarted",
            "ToolCallStarted",
            "PermissionDecided",
            "ToolCallCompleted"
        ]
    );
}

#[tokio::test]
async fn no_hook_means_no_hook_events() {
    let mut fixture = fixture(
        vec![read_reply("call-read", "notes.txt"), Reply::text("done")],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
        None,
    )
    .await;
    fixture.write("notes.txt", "hello\n");

    fixture.run_turn("read it").await;
    fixture.shutdown().await;

    assert!(
        fixture.hook_events().is_empty(),
        "an unmounted hook point appends nothing"
    );
    assert_eq!(fixture.results().len(), 1);
    assert!(fixture.stdout.text().contains("done"));
}
