//! 执行者（`task`，spec §16）。
//!
//! 接缝是组装入口：一个用脚本化假 provider 组装起来的
//! 会话，经公开的库 API 驱动，断言 JSONL 事件流、工作区
//! 与两个渲染 sink。执行者跑在派出它的那个会话的
//! provider 上（spec §16：模型是继承来的），所以一个 `FakeProvider`
//! 按调用顺序脚本化了整场嵌套对话。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::SessionConfig;
use heng::events::{
    read_events, Decision, Event, EventPayload, ParticipantId, Role, SessionId, SpeakerId,
    StopReason, Usage,
};
use heng::permissions::{Asker, Mode, Policy, Rule, Scope, Subject};
use heng::provider::{FinishReason, Message, ProviderError, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn executor(id: &str) -> SpeakerId {
    SpeakerId::Executor(ParticipantId::new(id))
}

/// 一条脚本化的助手消息，它要一次工具调用。
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
    /// 会话目录：日志与 `outputs/` 产物。
    session_dir: PathBuf,
    /// 会话工作区，工具在这里读和写。
    cwd: PathBuf,
    _dir: tempfile::TempDir,
}

/// 组装一个工作区里已经放着 `files` 的会话，于是 `AGENTS.md`
/// 与执行者的世界在组装读取它们之前就已就位。
async fn fixture(
    files: &[(&str, &str)],
    replies: Vec<Reply>,
    config: SessionConfig,
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
) -> Fixture {
    fixture_with(files, replies, config, policy, asker, None).await
}

/// 同上，外加挂上一个 hook。
async fn fixture_with(
    files: &[(&str, &str)],
    replies: Vec<Reply>,
    config: SessionConfig,
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn heng::hooks::Hook>>,
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
            tools: heng::tools::builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy,
            asker,
            questions: None,
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

/// 唯一那条匹配 `predicate` 的事件，否则 panic 并指出缺了什么。
fn only<'a>(
    events: &'a [Event],
    predicate: impl Fn(&EventPayload) -> bool,
    want: &str,
) -> &'a Event {
    let found: Vec<&Event> = events
        .iter()
        .filter(|event| predicate(&event.payload))
        .collect();
    assert_eq!(found.len(), 1, "{want} 恰好要有一条，得到 {found:?}");
    found[0]
}

/// `speaker` 发出的、匹配 `predicate` 的那唯一一条事件，
/// 否则 panic 并指出缺了什么。
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
        "要有 {speaker} 的一条 {want}，恰好一条，得到 {found:?}"
    );
    found[0]
}

/// 某个发言者说过的每一条 `MessageCompleted` 文本，按顺序。
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

fn tool_names(request: &heng::provider::ChatRequest) -> Vec<String> {
    request.tools.iter().map(|tool| tool.name.clone()).collect()
}

fn contents(request: &heng::provider::ChatRequest, want: &dyn Fn(&Message) -> bool) -> Vec<String> {
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
    // 生命周期事件是执行者自己的，于是简报能到达它的
    // 投影；`parent` 才是给派发者命名的那个字段。
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
        other => panic!("要的是 ExecutorSpawned，得到 {other:?}"),
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
        other => panic!("要的是 ExecutorFinished，得到 {other:?}"),
    }

    // 派发、执行者自己的回合与收尾都落在同一条
    // 流上，顺序就是这个顺序。
    assert!(spawned.seq < finished.seq);
    assert_eq!(
        said(&events, &executor("kimi-1")),
        vec!["EXECUTOR REPORT: 12 files under src".to_owned()]
    );

    // `task` 那条调用自己恰好得到一条结果，而这条结果就是
    // 派发它的那个发言者报给讨论的总结。
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
        other => panic!("要的是 ToolCallCompleted，得到 {other:?}"),
    }

    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 3, "父会话、执行者、父会话");

    // 派发者自己的表里挂着 `task`；执行者的没有，因为
    // 「递归深度为一」是靠工具表强制的，不是靠规则。
    assert!(tool_names(&requests[0]).contains(&"task".to_owned()));
    assert!(
        !tool_names(&requests[1]).contains(&"task".to_owned()),
        "执行者的工具表里不许有 `task`：{:?}",
        tool_names(&requests[1])
    );

    // 执行者的窗口：它的私有身份、钉住的那些注入，以及它
    // 自己的事件。派发会话的发言不在里面。
    match &requests[1].messages[0] {
        Message::System { content, .. } => {
            assert!(content.contains("执行者"), "{content}");
            assert!(!content.contains("CONCLUSION:"), "{content}");
            // 那一句思考语言的条款四段身份共用（`tests/thinking_language.rs`
            // 断言公开的那三段）；执行者的身份不在公开 API 上，只能从这里读。
            assert!(
                content.contains(heng::agent::THINKING_IN_CHINESE),
                "{content}"
            );
            // 联网那段指引同理（`.scratch/web-search-tool/spec.md` §8）：执行者去干活时
            // 自己就能查，不必让讨论者把结果转述过去。
            assert!(content.contains(heng::agent::WEB_GUIDANCE), "{content}");
            // 而时间那句**只**拼在本程序的身份上（`.scratch/time-mcp/spec.md` §5）：执行者的
            // 工具表里没有 MCP 那一套，指它反而是指一条不存在的路。
            assert!(!content.contains(heng::agent::TIME_GUIDANCE), "{content}");
        }
        other => panic!("要的是执行者自己的系统身份，得到 {other:?}"),
    }
    let head = contents(&requests[1], &is_user).join("\n");
    assert!(
        head.contains("PROJECT RULES: always run cargo fmt"),
        "与别的会话一样，执行者也会被注入 AGENTS.md：{head}"
    );
    assert!(head.contains("count the files under src"), "{head}");
    assert!(
        !head.contains("count them for me"),
        "执行者不重放派发会话的发言：{head}"
    );

    // 而执行者的过程不进派发者的窗口：总结只以工具结果的
    // 形式到达一次，从不作为发言出现。
    let dispatcher = &requests[2];
    let speech = contents(dispatcher, &is_user).join("\n");
    assert!(
        !speech.contains("EXECUTOR REPORT"),
        "执行者的消息不许投影给讨论者：{speech}"
    );
    let results = contents(dispatcher, &|message| {
        matches!(message, Message::Tool { .. })
    })
    .join("\n");
    assert!(results.contains("EXECUTOR REPORT"), "{results}");

    fixture.harness.shutdown().await;
    assert_eq!(fixture.stdout.text(), "the executor counted 12 files\n");
    // 执行者自己的干活过程会被叙述给读终端的人看，
    // 归在它自己的发言者标签下。
    let label = heng::render::wording::speaker_label(&SpeakerId::Executor("kimi-1".into()));
    assert!(
        fixture.stderr.text().contains(&label),
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

    // 执行者自己的回合跑了两轮迭代：它的结果正文是整段
    // 重放的，不像别的发言者 —— 后者的工具调用只以一行留存。
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 4, "父会话、执行者、执行者、父会话");
    let executor_second = &requests[2];
    let assistant = executor_second
        .messages
        .iter()
        .find_map(|message| match message {
            Message::Assistant { tool_calls, .. } if !tool_calls.is_empty() => Some(tool_calls),
            _ => None,
        })
        .expect("执行者自己那次工具调用被重放");
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
        .expect("执行者自己那条工具结果被重放");
    assert_eq!(result.0, "exec-1");
    assert!(result.1.contains("the answer is 42"), "{}", result.1);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executors_read_set_starts_empty_and_the_dispatchers_does_not_travel() {
    let mut fixture = fixture(
        &[("plan.txt", "keep me\n")],
        vec![
            // 派发者自己读了那个文件，这许可了它自己的那次编辑，
            // 别的什么都不许可。
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
            // 执行者没读就编辑：它的已读集一开始是空的。
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
        // 一个每次都批准写的 user，于是权限门放执行者的这次编辑
        // 过去，拒它的是「写前先读」那条护栏。
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
    assert!(
        failures[0].contains(heng::tools::READ_BEFORE_WRITE_PREFIX),
        "{}",
        failures[0]
    );
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executors_edit_is_undoable_like_any_other() {
    // （票 12。）`/undo` 走的是整个会话的流，而不是某个 agent 的那
    // 一片，所以执行者做过的一次编辑以同样的方式回滚（spec §11、§16）。
    let mut fixture = fixture(
        &[("plan.txt", "keep me\n")],
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "edit plan.txt"}),
            ),
            // 执行者先读：它的已读集一开始是空的。
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

    // 手势是用户的；流记下它引起的退役。
    let events = fixture.events();
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventPayload::HistorySuperseded {
            reason: heng::events::HistoryReason::Undo,
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
    // 派发者自己的回合不受失败执行者的影响（spec §16）。
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
        other => panic!("要的是 ExecutorFinished，得到 {other:?}"),
    }
    // 那四个失败值到达时是一条错误内容的工具结果，这也正是
    // 「每条 tool_call 恰好一条结果」对 `task` 同样成立的原因。
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
            assert!(error.contains("执行者 kimi-1 结束：Error"), "{error}");
        }
        other => panic!("要的是 ToolCallCompleted，得到 {other:?}"),
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
            // 执行者拿到两个回合，然后撞上自己的上限；
            // 派发者的上限没被动到。
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
        other => panic!("要的是 ExecutorFinished，得到 {other:?}"),
    }
    let executor_turns = events
        .iter()
        .filter(|event| {
            event.speaker_id == executor("kimi-1")
                && matches!(event.payload, EventPayload::TurnStarted { .. })
        })
        .count();
    assert_eq!(executor_turns, 2, "约束它的是执行者自己的上限");
    // 派发者的原因是它自己的：执行者上的 `MaxIterations`
    // 不会向上漏成这个会话的停止原因。
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
        // 执行者自己那一档模式对写要问；这个用户一律批准。
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

    // `/undo` 对执行者的编辑也管用，因为产物落在同一个会话
    // 目录下、用同一套命名约定（spec §11、§16）。
    let snapshot = fixture.session_dir.join("outputs").join("exec-edit.before");
    assert!(snapshot.exists(), "预期 {} 存在", snapshot.display());
    // 快照是被替换的那一段本身，不是整个文件：`/undo`
    // 写回去的就是它（spec §8、§11）。
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "before");

    // 而这次改动以元数据的形式报回来，元数据从流上派生。
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
        .expect("task 调用的结果");
    // 工具报的是它解析出来的路径，所以元数据点名的就是
    // 这次编辑落上去的那个文件。
    assert!(reported.contains("notes.txt"), "{reported}");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn two_task_calls_in_one_batch_run_at_once() {
    // 两次执行者调用必须都在进行中的时候碰头：屏障只有在第二个
    // 到达时才放行，而串行的派发器会先撞上自己的
    // 超时。
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
        "两个执行者在任何一个收尾之前都被派出去了：spawned {spawned:?}，\
         finished {finished:?}"
    );

    // 每个执行者得到自己的 id，每条 `task` 调用得到自己的结果。
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
    // 两个执行者自己的回合都不是这个会话的产物。
    assert_eq!(fixture.stdout.text(), "both executors reported\n");
}

#[tokio::test]
async fn the_batch_cap_bounds_how_many_executors_work_at_once() {
    // 两个执行者在屏障处碰头；第三个必须等一个空位，
    // 所以它的派发记在那一对里某个收尾之后。
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
    assert!(spawned[1] < finished[0], "前两个是一起跑的");
    assert!(
        finished[0] < spawned[2],
        "第三个等了一个空位：spawned {spawned:?}，finished {finished:?}"
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
    // 结果按批次的顺序记下，不论它们以什么顺序收尾。
    assert!(results[0].contains("first report"), "{}", results[0]);
    assert!(results[1].contains("second report"), "{}", results[1]);
    assert!(results[2].contains("third report"), "{}", results[2]);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_hook_that_stops_the_turn_still_gives_a_deferred_task_its_one_result() {
    let hook = support::ScriptedHook::new(
        vec![
            Ok(heng::hooks::Constraint::Continue),
            Ok(heng::hooks::Constraint::Stop),
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
    // 被延后的 `task` 在 hook 停住回合之前已经在流上开始了，
    // 所以它仍然欠着恰好一条结果 —— 而它从没跑过。
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
    assert_eq!(results.len(), 2, "每条已开始的调用都保有自己的那条结果");
    assert!(results.iter().all(|(ok, _)| !ok));
    assert!(
        results
            .iter()
            .all(|(_, error)| error.contains("钩子停掉了这个回合")),
        "{results:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. })),
        "执行者从没被派出"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_denial_travels_down_to_the_executor() {
    let mut policy = Policy::for_mode(Mode::Auto);
    policy.push(Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        // `Deny` 默认向下传播（spec §12）：它是一条约束，
        // 而约束只能往下走。
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
            // 执行者先读，所以挡在它和这次写之间的只有
            // 那条继承来的拒绝。
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
        "执行者的那次拒绝",
    );
    match &decided.payload {
        EventPayload::PermissionDecided {
            decision, reason, ..
        } => {
            assert_eq!(*decision, Decision::Deny);
            assert!(reason.as_deref().unwrap().contains("规则"), "{reason:?}");
        }
        other => panic!("要的是 PermissionDecided，得到 {other:?}"),
    }
    // `edit_file` 这次要问写，所以执行者根本没走到工具。
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_allowance_does_not_travel_down_to_the_executor() {
    let mut policy = Policy::for_mode(Mode::Ask);
    // 派发者自己的会话级许可：`Allow` 不传播，
    // 而 headless 会话没有人可问，所以执行者的写被
    // 降级成拒绝，而不是挥手放过。
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
        denied[0].contains("没有可交互的作答者"),
        "执行者的模式要问，而没有人能回答：{}",
        denied[0]
    );
    assert_eq!(fixture.read("plan.txt"), "keep me\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_writes_under_the_dispatchers_auto_stance() {
    // `auto` 的意思是「写是放行的」，而执行者干的是同一个会话里的
    // 活：一个只能读的执行者，会让 `task` 恰恰在最该无人值守运行
    // 的地方变得没用。这里没有应答者，所以继承一档 `ask`
    // 本来会直接拒掉这次写。
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
    // 委派从来不是绕过一档硬立场的路。这个应答者什么都批准，
    // 所以仅仅「要问」的一档模式本来也会放它过去。
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
        "改派了执行者而没有挪动派发者"
    );
    assert_eq!(
        requests[2].model, "dispatcher-model",
        "而派发者自己下一次调用没被动到"
    );
}

#[tokio::test]
async fn the_report_names_the_file_a_rewriting_hook_actually_wrote() {
    // 一次 `hook.pre` 可能在 `ToolCallStarted` 记下模型要的东西
    // 之后改写这次调用。真正被写的文件只有结果里那一份记录，
    // 所以报告该点名的就是它。
    let hook = support::ScriptedHook::new(
        vec![
            Ok(heng::hooks::Constraint::Continue),
            Ok(heng::hooks::Constraint::Rewrite(serde_json::json!({
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
        .expect("task 调用的结果");
    assert!(reported.contains("created.txt"), "{reported}");
    assert!(!reported.contains("requested.txt"), "{reported}");
}

#[tokio::test]
async fn an_exhausted_session_dispatches_no_new_executor() {
    // 要执行者的那条回复本身就把整份额度用满了，所以
    // 这次派发被拒。因为那条调用已经开始了，它仍然恰好得到
    // 一条结果（不变量 1），而那条结果说的是执行者从没跑过 ——
    // 一个已经在跑的执行者本会被放着跑完（spec §17）。
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
        "额度用完的会话不派发新执行者"
    );
    let results: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(results.len(), 1, "task 调用保有自己的那条结果");
    match &results[0].payload {
        EventPayload::ToolCallCompleted { ok, error, .. } => {
            assert!(!ok);
            let error = error.as_deref().unwrap();
            assert!(error.contains("会话 token 额度已用尽"), "{error}");
            assert!(error.contains("不再派发新的执行者"), "{error}");
        }
        other => panic!("要的是 ToolCallCompleted，得到 {other:?}"),
    }
    assert_eq!(
        fixture.provider.requests().len(),
        1,
        "回合停下了，而不是再调一次 provider"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_already_running_finishes_even_when_the_allowance_is_gone() {
    // 「不派新的执行者，已经在跑的跑完」（spec §17）。执行者的
    // 第一次调用把会话额度打爆了、又要一个工具，所以带闸门的
    // 循环会在下一轮迭代把它丢掉；它必须反过来跑到
    // 自己的回合上限，而硬停结束的是派发者
    // 自己的那个回合。
    let limit = 500;
    let mut fixture = fixture(
        &[("notes.txt", "the notes\n")],
        vec![
            // 在额度之内，所以这次派发放行。
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
            // 执行者自己的那次调用花得远超上限、又要一个
            // 工具，所以它的循环本会迭代第二次。
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
                "已经在跑的执行者会被放着跑完"
            );
            assert_eq!(summary, "read the notes");
        }
        other => panic!("要的是 ExecutorFinished，得到 {other:?}"),
    }
    // 它的花费仍然算数：派发者自己下一轮迭代被拒。
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
                } if output.starts_with("执行者 kimi-1 结束：")
            )
        },
        "task 的结果",
    );
    match &reported.payload {
        EventPayload::ToolCallCompleted { output, .. } => {
            let output = output.as_deref().unwrap();
            assert!(
                output.contains("结束：Completed"),
                "task 报的是执行者自己的收尾：{output}"
            );
        }
        other => panic!("要的是 ToolCallCompleted，得到 {other:?}"),
    }
    assert_eq!(
        fixture.provider.requests().len(),
        3,
        "执行者那两次调用发生了，派发者第二次没有"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_inherits_the_outside_read_knob() {
    // `outside_read` 是一条**策略级**的旋钮、与档位正交（`.scratch/workspace-mode` 的
    // spec §2）：执行者沿用派发者的立场，所以父会话把区外读放开之后，`task` 派出的执行者
    // 也读得到 —— 否则同一场会话里讨论者读得到、执行者读不到。
    let policy = Policy::for_mode(Mode::Ask).with_outside_read(Decision::Allow);
    let mut fixture = fixture(
        &[("../outside/secret.txt", "peek")],
        vec![
            calls(
                "call-task",
                "task",
                serde_json::json!({"brief": "读一下 ../outside/secret.txt 再报告"}),
            ),
            calls(
                "call-read",
                "read_file",
                serde_json::json!({"file_path": "../outside/secret.txt"}),
            ),
            Reply::text("EXECUTOR REPORT: 文件里写的是 peek"),
            Reply::text("执行者读到了"),
        ],
        SessionConfig::new("fake-model"),
        policy,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    fixture.harness.run_turn("去读它").await.unwrap();

    let events = fixture.events();
    let read_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                ..
            } if tool_name == "read_file" => Some(tool_call_id.as_str().to_owned()),
            _ => None,
        })
        .expect("执行者调了 read_file");
    let (ok, message) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                ..
            } if tool_call_id.as_str() == read_id => Some((
                *ok,
                output.clone().or_else(|| error.clone()).unwrap_or_default(),
            )),
            _ => None,
        })
        .expect("read_file 拿到了结果");

    assert!(ok, "执行者继承了派发者的区外读裁决：{message}");
    assert!(message.contains("peek"), "{message}");
}
