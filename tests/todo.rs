//! The built-in `todo(items)` tool (`.scratch/todo-and-modes/spec.md` §2, §3).
//!
//! The tool's contract is small and mostly about one decision: the list **is** the
//! call's arguments. These tests drive it both ways — through the dispatch seam
//! for the contract itself, and through a real session for the two things only a
//! session can show (one result per call, and the args as the truth a sidebar
//! recomputes from).

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{read_events, Event, EventPayload, SessionId, SpeakerId, StopReason};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::todo::{read_items, Status, TODO_TOOL};
use fs_agent::tools::{
    builtin, BashLimits, Effect, PathLocks, PendingCall, ReadSet, Registry, SessionPaths,
};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::json;
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// The dispatch seam, the way `tests/tools_dispatch.rs` builds it: the tool's own
/// contract does not depend on who called it.
struct Fixture {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    outputs: PathBuf,
    paths: SessionPaths,
    locks: PathLocks,
    registry: Registry,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let outputs = dir.path().join("outputs");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = std::fs::canonicalize(&workspace).unwrap();
        Self {
            paths: SessionPaths::new(&workspace),
            locks: PathLocks::new(),
            registry: builtin(false),
            outputs,
            dir,
        }
    }

    fn call(&self, id: &str, args: serde_json::Value) -> PendingCall {
        PendingCall {
            tool_call_id: id.to_owned(),
            tool_name: TODO_TOOL.to_owned(),
            args,
            outputs_dir: self.outputs.clone(),
            paths: self.paths.clone(),
            locks: self.locks.clone(),
            skills: Arc::new(fs_agent::context::skills::Skills::default()),
            repo_map: fs_agent::context::repo_map::RepoMapInput::default(),
            bash: BashLimits::default(),
            executor: None,
            questions: None,
        }
    }

    /// Run one call through the guardrails, exactly as the loop does.
    async fn dispatch(&self, call: &PendingCall) -> fs_agent::tools::DispatchOutcome {
        let mut read_set = ReadSet::default();
        let allowed = match self
            .registry
            .facts(&call.tool_name, &call.args, &self.paths)
            .expect("a registered tool")
            .guardrails(&read_set)
        {
            fs_agent::tools::GuardedCall::Run(allowed) => allowed,
            fs_agent::tools::GuardedCall::Refused(error) => {
                return fs_agent::tools::DispatchOutcome::failure(error, false)
            }
        };
        read_set.record_all(allowed.read_paths.iter().cloned());
        self.registry.dispatch(call, &allowed).await
    }

    /// The result text of one call, or the error text it was refused with.
    async fn text(&self, args: serde_json::Value) -> Result<String, String> {
        let call = self.call("call-1", args);
        self.dispatch(&call)
            .await
            .result
            .map(|out| out.text)
            .map_err(|error| error.to_string())
    }
}

fn items_of(list: &[(&str, &str)]) -> serde_json::Value {
    json!({
        "items": list
            .iter()
            .map(|(content, status)| json!({ "content": content, "status": status }))
            .collect::<Vec<_>>()
    })
}

// --- the contract ---------------------------------------------------------

#[tokio::test]
async fn a_valid_call_answers_with_a_count_of_the_whole_list() {
    let fixture = Fixture::new();

    let receipt = fixture
        .text(items_of(&[
            ("read the spec", "completed"),
            ("write the tool", "in_progress"),
            ("run the suite", "pending"),
        ]))
        .await
        .unwrap();
    assert_eq!(receipt, "todo: 3 items (1 completed)");

    // The list is replace-all, so the receipt counts what was just submitted and
    // nothing that came before.
    let receipt = fixture
        .text(items_of(&[("only one left", "pending")]))
        .await
        .unwrap();
    assert_eq!(receipt, "todo: 1 item (0 completed)");
}

#[tokio::test]
async fn a_missing_or_empty_list_clears_it() {
    // The two spellings the spec fixes: an empty array, and the field left out.
    let fixture = Fixture::new();
    for args in [json!({ "items": [] }), json!({})] {
        let receipt = fixture.text(args.clone()).await.unwrap();
        assert_eq!(receipt, "todo: cleared", "{args}");
    }
}

#[tokio::test]
async fn a_call_the_schema_cannot_read_is_refused_with_a_model_readable_reason() {
    // Every one of these is a mistake the model can fix, so the message says what
    // was wrong rather than "invalid arguments". Nothing here is a panic and
    // nothing is silently dropped: a list with one bad item is refused whole.
    let fixture = Fixture::new();
    let cases: Vec<(serde_json::Value, &str)> = vec![
        (
            json!({ "items": [{ "content": "", "status": "pending" }] }),
            "content",
        ),
        (
            json!({ "items": [{ "content": "  ", "status": "pending" }] }),
            "content",
        ),
        (
            json!({ "items": [{ "content": "x", "status": "done" }] }),
            "status",
        ),
        (json!({ "items": [{ "content": "x" }] }), "status"),
        // The two shapes a model is most likely to get wrong, each named where it
        // went wrong: the array itself, then the item that is not an object.
        (
            json!({ "items": { "content": "x" } }),
            "`items` must be an array",
        ),
        (json!({ "items": ["x"] }), "item 0 is not an object"),
        (
            json!({ "items": [{ "content": "x", "status": "pending" }], "extra": 1 }),
            "extra",
        ),
    ];
    for (args, expected) in cases {
        let error = fixture
            .text(args.clone())
            .await
            .expect_err(&format!("{args} is refused"));
        assert!(
            error.contains(expected),
            "{args}: the reason names `{expected}`: {error}"
        );
        assert!(
            error.starts_with(TODO_TOOL),
            "{args}: the reason names the tool: {error}"
        );
    }
}

#[tokio::test]
async fn the_tool_touches_no_workspace_path() {
    // `effect` is the **workspace** side-effect vocabulary (spec §7), and a list
    // that lives in the call's own arguments writes nothing. That is also what
    // lets two of these run concurrently and what keeps the gate from ever asking
    // about one.
    let fixture = Fixture::new();
    let call = fixture.call("call-1", items_of(&[("x", "pending")]));
    assert_eq!(
        fixture.registry.get(TODO_TOOL).unwrap().effect(&call.args),
        Effect::ReadOnly
    );
    assert!(
        fixture
            .registry
            .get(TODO_TOOL)
            .unwrap()
            .read_paths(&call.args)
            .is_empty(),
        "and it reads nothing either"
    );
    assert!(
        fixture
            .registry
            .get(TODO_TOOL)
            .unwrap()
            .command(&call.args)
            .is_none(),
        "no argv, so no `CommandPrefix` scope and no breaker"
    );
}

#[test]
fn the_list_a_reader_sees_is_the_arguments_of_the_call() {
    // The truth is the args, and this is the reading of them the sidebar and any
    // later reader uses. Nothing parses the receipt text.
    let args = items_of(&[
        ("first", "completed"),
        ("second", "in_progress"),
        ("third", "pending"),
    ]);
    let items = read_items(&args);
    assert_eq!(
        items
            .iter()
            .map(|item| (item.content.as_str(), item.status))
            .collect::<Vec<_>>(),
        vec![
            ("first", Status::Completed),
            ("second", Status::InProgress),
            ("third", Status::Pending),
        ]
    );

    // A call the tool would have refused contributes no list rather than half a
    // one: the writer validates, so this is a reader that must not panic.
    assert!(read_items(&json!({ "items": "nope" })).is_empty());
    assert!(read_items(&json!({})).is_empty());
}

#[test]
fn a_later_call_in_the_same_message_is_the_one_in_force() {
    // Two `todo` calls in one assistant message are two calls of a `ReadOnly`
    // tool, so they may run concurrently — and the second one's list is the one
    // left standing, because replace-all is the contract. Reading by `seq` is how
    // every reader of the stream settles it, and this pins that "last one wins" is
    // a property of the data rather than a race.
    let first = items_of(&[("old", "pending")]);
    let second = items_of(&[("new", "completed")]);
    let stream = [(1u64, first), (2, second)];
    let latest = read_items(&stream[stream.len() - 1].1);
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].content, "new");
    assert_eq!(latest[0].status, Status::Completed);
}

#[test]
fn every_table_that_can_plan_has_the_tool() {
    // Main sessions, debaters (they are main sessions) and executors all plan;
    // `delegable` stays at its default `true` precisely so an executor gets it.
    // Headless is the third renderer, and this tool needs no person — unlike
    // `ask_user_question`, which is the one tool `can_ask` gates (spec §7, §19).
    for can_ask in [false, true] {
        let table = builtin(can_ask);
        assert!(
            table.get(TODO_TOOL).is_some(),
            "the built-in table offers it (can_ask = {can_ask})"
        );
        assert!(
            table.for_executor().get(TODO_TOOL).is_some(),
            "and an executor keeps it (can_ask = {can_ask})"
        );
    }
}

#[test]
fn the_identity_tells_the_model_to_keep_a_list() {
    // The rules section (spec §3): guidance, not enforcement. The instruction is
    // in the model-visible prefix of every request, so it *is* the cached prefix —
    // adding to it is allowed, changing it is not (ADR 0001, ADR 0003).
    let identity = fs_agent::agent::agent_identity();
    assert!(identity.contains("todo"), "{identity}");
    assert!(identity.contains("pending"), "{identity}");
    assert!(identity.contains("in_progress"), "{identity}");
    assert!(identity.contains("completed"), "{identity}");
    assert!(identity.contains("Before you start"), "{identity}");
}

// --- through a real session ------------------------------------------------

struct Session {
    harness: Harness,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

/// A session whose model does what the script says, mounted with the real table.
async fn session(replies: Vec<Reply>) -> Session {
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session_dir.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-todo"),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: Some(Arc::new(AlwaysAllow)),
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Session {
        harness,
        log_path,
        _dir: dir,
    }
}

impl Session {
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }
}

fn todo_reply(id: &str, args: &serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: TODO_TOOL.into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

#[tokio::test]
async fn one_todo_call_gets_exactly_one_result_and_the_arguments_stand_verbatim() {
    let args = items_of(&[("写测试", "in_progress"), ("跑全量", "pending")]);
    let mut session = session(vec![todo_reply("call-1", &args), Reply::text("记下了")]).await;

    let outcome = session.harness.run_turn("开工").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = session.events();
    let started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } if tool_name == TODO_TOOL => Some(args.clone()),
            _ => None,
        })
        .expect("the stream carries the call");
    assert_eq!(
        started, args,
        "the arguments are stored as written: the list is the call"
    );

    let results: Vec<&EventPayload> = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventPayload::ToolCallCompleted { tool_call_id, .. }
                    if tool_call_id.as_str() == "call-1"
            )
        })
        .map(|event| &event.payload)
        .collect();
    assert_eq!(results.len(), 1, "exactly one result, no more and no fewer");
    match results[0] {
        EventPayload::ToolCallCompleted { ok, output, .. } => {
            assert!(*ok);
            assert_eq!(
                output.as_deref(),
                Some("todo: 2 items (0 completed)"),
                "the receipt is the acknowledgement, not the list"
            );
        }
        other => panic!("expected a completion, got {other:?}"),
    }

    session.harness.shutdown().await;
}

#[tokio::test]
async fn two_calls_in_one_message_each_get_a_result_and_the_last_list_wins() {
    // Concurrent by `effect()`, ordered by `seq`: two lists land, and the one a
    // reader takes — the later — is the one in force.
    let first = items_of(&[("旧的", "pending")]);
    let second = items_of(&[("新的", "completed")]);
    let both = Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: "call-1".into(),
            name: TODO_TOOL.into(),
            arguments: first.to_string(),
        },
        StreamEvent::ToolCallCompleted {
            index: 1,
            id: "call-2".into(),
            name: TODO_TOOL.into(),
            arguments: second.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ]);
    let mut session = session(vec![both, Reply::text("好")]).await;

    session.harness.run_turn("两次").await.unwrap();

    let lists: Vec<serde_json::Value> = session
        .events()
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } if tool_name == TODO_TOOL => Some(args.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(lists.len(), 2, "both calls are on the stream");

    let results = session
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .count();
    assert_eq!(results, 2, "and each has its one result");

    let in_force = read_items(lists.last().unwrap());
    assert_eq!(in_force[0].content, "新的");
    assert_eq!(in_force[0].status, Status::Completed);

    session.harness.shutdown().await;
}
