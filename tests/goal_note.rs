//! 内建的 `goal_note(notes)` 工具（`.scratch/goal-loop/spec.md` §11）。
//!
//! 与 `todo` 完全同构：**args 即真相**，不为它新增任何事件。所以这里的断言分两头 —— 契约本身
//! 走派发接缝，而「真会话里参数留下的就是那些 note」走一个真会话。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::SessionConfig;
use heng::events::{read_events, Event, EventPayload, SessionId, SpeakerId, StopReason};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::goal_note::{read_notes, GOAL_NOTE_TOOL};
use heng::tools::{
    builtin, BashLimits, Effect, PathLocks, PendingCall, ReadSet, Registry, Sandbox, SessionPaths,
};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::json;
use support::{CaptureBuf, FakeProvider, Reply};

/// 派发接缝，按 `tests/tools_dispatch.rs` 搭它的方式搭。
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
            tool_name: GOAL_NOTE_TOOL.to_owned(),
            args,
            outputs_dir: self.outputs.clone(),
            paths: self.paths.clone(),
            locks: self.locks.clone(),
            skills: Arc::new(heng::context::skills::Skills::default()),
            repo_map: heng::context::repo_map::RepoMapInput::default(),
            bash: BashLimits::default(),
            sandbox: Sandbox::new(&heng::config::SandboxSettings::off()),
            executor: None,
            questions: None,
        }
    }

    async fn dispatch(&self, call: &PendingCall) -> heng::tools::DispatchOutcome {
        let mut read_set = ReadSet::default();
        let allowed = match self
            .registry
            .facts(&call.tool_name, &call.args, &self.paths, None)
            .expect("一个注册过的工具")
            .guardrails(&read_set)
        {
            heng::tools::GuardedCall::Run(allowed) => allowed,
            heng::tools::GuardedCall::Refused(error) => {
                return heng::tools::DispatchOutcome::failure(error, false)
            }
        };
        read_set.record_all(allowed.read_paths.iter().cloned());
        self.registry.dispatch(call, &allowed).await
    }

    async fn text(&self, args: serde_json::Value) -> Result<String, String> {
        let call = self.call("call-1", args);
        self.dispatch(&call)
            .await
            .result
            .map(|out| out.text)
            .map_err(|error| error.to_string())
    }
}

// --- 契约 ------------------------------------------------------------------

#[tokio::test]
async fn a_valid_call_answers_with_a_count_and_says_nothing_else() {
    let fixture = Fixture::new();
    let receipt = fixture
        .text(json!({ "notes": ["补一条迁移脚本", "把 README 的用法改掉"] }))
        .await
        .unwrap();
    assert_eq!(receipt, "goal_note：记下 2 条");

    let receipt = fixture
        .text(json!({ "notes": ["只有一条"] }))
        .await
        .unwrap();
    assert_eq!(receipt, "goal_note：记下 1 条");
}

#[tokio::test]
async fn a_call_the_schema_cannot_read_is_refused_with_a_model_readable_reason() {
    // 每一条都是模型自己能修的错：说清是哪里不对，而且整次调用被拒 —— 绝不悄悄丢一条。
    let fixture = Fixture::new();
    let cases: Vec<(serde_json::Value, &str)> = vec![
        (json!([]), "必须是一个带"),
        (json!({ "notes": "一句话" }), "字符串数组"),
        (json!({ "notes": [""] }), "空的"),
        (json!({ "notes": ["  "] }), "空的"),
        (json!({ "notes": [7] }), "不是字符串"),
        (json!({ "notes": [] }), "至少"),
        (json!({}), "notes"),
        (json!({ "notes": ["x"], "extra": 1 }), "extra"),
    ];
    for (args, expected) in cases {
        let error = fixture
            .text(args.clone())
            .await
            .expect_err(&format!("{args} 被拒了"));
        assert!(
            error.contains(expected),
            "{args}：这个理由点出了 `{expected}`：{error}"
        );
        assert!(error.starts_with(GOAL_NOTE_TOOL), "{error}");
    }
}

#[test]
fn the_tool_touches_no_workspace_path_so_the_gate_never_asks() {
    // `effect` 是**工作区**副作用的词汇，而一份住在调用自己参数里的记录什么都不写。这也正是
    // 两个这样的调用可以并发、无人值守时不会停下来等人的原因。
    let fixture = Fixture::new();
    let call = fixture.call("call-1", json!({ "notes": ["x"] }));
    let tool = fixture.registry.get(GOAL_NOTE_TOOL).unwrap();
    assert_eq!(tool.effect(&call.args), Effect::ReadOnly);
    assert!(tool.read_paths(&call.args).is_empty(), "它也什么都不读");
    assert!(tool.command(&call.args).is_none(), "没有 argv");
}

#[test]
fn the_notes_a_reader_sees_are_the_arguments_of_the_call() {
    let args = json!({ "notes": ["第一件", "第二件"] });
    assert_eq!(read_notes(&args), ["第一件", "第二件"]);

    // 一次工具本来会拒掉的调用，贡献的是没有 note，而不是半份。
    assert!(read_notes(&json!({ "notes": "nope" })).is_empty());
    assert!(read_notes(&json!({})).is_empty());
}

#[test]
fn every_table_that_can_plan_has_the_tool() {
    // 主会话、讨论者（它们就是主会话）与执行者都要能记新工作；`delegable` 保持默认的 `true`，
    // 正是为了让执行者也拿到它 —— 与 `task` 被拿掉恰好相反。headless 是第三种前端，而这个
    // 工具不需要人。
    for can_ask in [false, true] {
        let table = builtin(can_ask);
        assert!(
            table.get(GOAL_NOTE_TOOL).is_some(),
            "内置表也把它摆进去（can_ask = {can_ask}）"
        );
        assert!(
            table.for_executor().get(GOAL_NOTE_TOOL).is_some(),
            "而执行者照样留着它（can_ask = {can_ask}）"
        );
    }
}

// --- 走一个真会话 ----------------------------------------------------------

struct Session {
    harness: Harness,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

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
            session_id: SessionId::new("s-notes"),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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

fn note_reply(id: &str, args: &serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: GOAL_NOTE_TOOL.into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

#[tokio::test]
async fn one_call_gets_exactly_one_result_and_the_arguments_are_the_truth() {
    let args = json!({ "notes": ["还要加一条迁移脚本", "顺手把 README 改了"] });
    let mut session = session(vec![note_reply("call-1", &args), Reply::text("记下了")]).await;

    let outcome = session.harness.run_turn("开工").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let events = session.events();
    let started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } if tool_name == GOAL_NOTE_TOOL => Some(args.clone()),
            _ => None,
        })
        .expect("流上扛着这次调用");
    assert_eq!(started, args, "参数按写下的样子存着：note 就是这次调用");

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
    assert_eq!(results.len(), 1, "正好一条结果，不多也不少");
    match results[0] {
        EventPayload::ToolCallCompleted { ok, output, .. } => {
            assert!(*ok);
            assert_eq!(
                output.as_deref(),
                Some("goal_note：记下 2 条"),
                "回执是一个确认，不是那份名单"
            );
        }
        other => panic!("期望一条完成，实际得到 {other:?}"),
    }

    // 票 05 的汇总读的就是这里：从流上的 args 重算，而不是去解析回执文本。
    assert_eq!(
        read_notes(&started),
        ["还要加一条迁移脚本", "顺手把 README 改了"]
    );

    // 不为它新增任何事件：这条流上多出来的只有那两条 `tool_call`。
    assert!(events
        .iter()
        .all(|event| !matches!(event.payload, EventPayload::ContextInjected { .. })));

    session.harness.shutdown().await;
}
