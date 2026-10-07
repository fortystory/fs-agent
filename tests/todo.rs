//! 内置的 `todo(items)` 工具（`.scratch/todo-and-modes/spec.md` §2、§3）。
//!
//! 这个工具的契约很小，而且几乎只围绕一个决定：那份列表**就是**
//! 这次调用的参数。这些测试从两头驱动它 —— 契约本身走派发接缝，
//! 另外两件只有真会话才展得出来的事走一个真会话
//! （每次调用一条结果，以及参数作为左栏
//! 用来重算的真相）。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::SessionConfig;
use heng::events::{Event, EventPayload, SessionId, SpeakerId, StopReason, read_events};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::todo::{Status, TODO_TOOL, read_items};
use heng::tools::{
    BashLimits, Effect, PathLocks, PendingCall, ReadSet, Registry, Sandbox, SessionPaths, builtin,
};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use serde_json::json;
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// 派发接缝，按 `tests/tools_dispatch.rs` 搭它的方式搭：工具自己的
/// 契约不取决于是谁调的它。
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
            skills: Arc::new(heng::context::skills::Skills::default()),
            repo_map: heng::context::repo_map::RepoMapInput::default(),
            bash: BashLimits::default(),
            // 沙箱在这个文件里是关的：这里测的是 `todo` 工具，不跑任何进程。
            sandbox: Sandbox::new(&heng::config::SandboxSettings::off()),
            executor: None,
            questions: None,
        }
    }

    /// 让一次调用过一遍护栏，与循环的做法一模一样。
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
                return heng::tools::DispatchOutcome::failure(error, false);
            }
        };
        read_set.record_all(allowed.read_paths.iter().cloned());
        self.registry.dispatch(call, &allowed).await
    }

    /// 一次调用的结果文本，或者它被拒时的错误文本。
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

// --- 契约 ------------------------------------------------------------------

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
    assert_eq!(receipt, "todo：3 项（1 项已完成）");

    // 这份列表是全量替换，所以回执数的是刚提交的那批，
    // 不含之前任何东西。
    let receipt = fixture
        .text(items_of(&[("only one left", "pending")]))
        .await
        .unwrap();
    assert_eq!(receipt, "todo：1 项（0 项已完成）");
}

#[tokio::test]
async fn a_missing_or_empty_list_clears_it() {
    // spec 定下来的两种写法：一个空数组，以及把这个字段留空。
    let fixture = Fixture::new();
    for args in [json!({ "items": [] }), json!({})] {
        let receipt = fixture.text(args.clone()).await.unwrap();
        assert_eq!(receipt, "todo：已清空", "{args}");
    }
}

#[tokio::test]
async fn a_call_the_schema_cannot_read_is_refused_with_a_model_readable_reason() {
    // 每一条都是模型自己能修的错，所以消息说的是哪里不对，
    // 而不是一句「参数无效」。这里没有一处 panic，
    // 也没有任何东西被悄悄丢掉：列表里有一项坏，整份就被拒。
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
        // 模型最容易弄错的两种形状，各自点名它错在哪儿：
        // 数组本身，然后是那个不是对象的项。
        (
            json!({ "items": { "content": "x" } }),
            "`items` 必须是一个由",
        ),
        (json!({ "items": ["x"] }), "第 0 项不是一个对象"),
        (
            json!({ "items": [{ "content": "x", "status": "pending" }], "extra": 1 }),
            "extra",
        ),
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
        assert!(
            error.starts_with(TODO_TOOL),
            "{args}：这个理由点出了工具名：{error}"
        );
    }
}

#[tokio::test]
async fn the_tool_touches_no_workspace_path() {
    // `effect` 是**工作区**副作用的词汇（spec §7），而一份住在
    // 调用自己参数里的列表什么都不写。这也正是
    // 两个这种调用可以并发、以及权限门从不为它
    // 发问的原因。
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
        "而它也什么都不读"
    );
    assert!(
        fixture
            .registry
            .get(TODO_TOOL)
            .unwrap()
            .command(&call.args)
            .is_none(),
        "没有 argv，所以没有 `CommandPrefix` 范围，也没有断路器"
    );
}

#[test]
fn the_list_a_reader_sees_is_the_arguments_of_the_call() {
    // 真相就是那些参数，而这就是左栏与后来任何读者
    // 用的那种读法。没有任何东西去解析回执文本。
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

    // 一次工具本来会拒掉的调用，贡献的是没有列表，而不是半份
    // 列表：写的那一侧会校验，所以读的这一侧绝不能 panic。
    assert!(read_items(&json!({ "items": "nope" })).is_empty());
    assert!(read_items(&json!({})).is_empty());
}

// --- 目标清单的条目 id（`.scratch/goal-loop/spec.md` §3） --------------------

/// 一项在照做的计划：带 id 的引用清单条目，不带 id 的照旧。
fn items_with_ids(list: &[(&str, &str, Option<&str>)]) -> serde_json::Value {
    json!({
        "items": list
            .iter()
            .map(|(content, status, id)| match id {
                Some(id) => json!({ "id": id, "content": content, "status": status }),
                None => json!({ "content": content, "status": status }),
            })
            .collect::<Vec<_>>()
    })
}

#[test]
fn an_item_may_carry_the_id_of_the_manifest_entry_it_is_working_on() {
    let args = items_with_ids(&[
        ("补测试", "completed", Some("03")),
        ("老形状的一项", "pending", None),
    ]);
    let items = read_items(&args);

    assert_eq!(items[0].id.as_deref(), Some("03"));
    assert_eq!(items[0].content, "补测试");
    assert_eq!(items[0].status, Status::Completed);
    assert_eq!(items[1].id, None, "缺 `id` 照旧：老调用能原样解析");
    assert_eq!(items[1].content, "老形状的一项");
}

#[tokio::test]
async fn an_id_that_is_not_two_decimal_digits_is_a_model_readable_error() {
    // 格式在这一票校验；**存在性不在** —— 工具不认识清单，越界的 id 由循环在派生进度时
    // 忽略（但不静默）。
    let fixture = Fixture::new();
    for bad in ["3", "003", "ab", "", "01a", " 03"] {
        let args = items_with_ids(&[("x", "pending", Some(bad))]);
        let error = fixture
            .text(args.clone())
            .await
            .expect_err(&format!("`{bad}` 会被拒"));
        assert!(
            error.contains("id") && error.contains("两位"),
            "`{bad}`：这个理由说清是哪个字段、期望什么：{error}"
        );
        assert!(error.starts_with(TODO_TOOL), "{error}");
    }
    // 合法的那几个照常通过，包括边界上的 `01` 与 `99`。
    for good in ["01", "99"] {
        let args = items_with_ids(&[("x", "pending", Some(good))]);
        assert_eq!(read_items(&args)[0].id.as_deref(), Some(good));
    }
}

#[test]
fn the_sidebar_shows_the_id_before_the_content() {
    use heng::events::ToolCallId;
    use heng::render::todo::TodoPanel;
    use heng::render::{Block, ToolBlock};
    use ratatui::layout::Rect;

    let mut panel = TodoPanel::default();
    panel.observe(&Block::Tool(Box::new(ToolBlock {
        speaker: SpeakerId::Debater("kimi".into()),
        tool_call_id: ToolCallId::new("call-1"),
        tool: TODO_TOOL.to_owned(),
        args: items_with_ids(&[
            ("补测试", "completed", Some("03")),
            ("没有 id 的一项", "pending", None),
        ]),
        outcome: None,
    })));

    let rows: Vec<String> = panel
        .lines(Rect::new(0, 0, 28, 4))
        .iter()
        .map(|line| line.to_string())
        .collect();
    assert_eq!(
        rows,
        vec![
            "✓ 03 补测试".to_owned(),
            "☐ 没有 id 的一项".to_owned(),
            "已完成 1/2".to_owned(),
        ],
        "有 id 的项在状态字形之后带上 id，没 id 的照旧；计数行不动"
    );
}

#[test]
fn a_later_call_in_the_same_message_is_the_one_in_force() {
    // 一条 assistant 消息里两次 `todo` 调用，就是两次 `ReadOnly`
    // 工具的调用，所以它们可以并发 —— 而站着的是第二份列表，
    // 因为契约是全量替换。按 `seq` 读，是流上每一个
    // 读者的了断方式，而这一条钉住「后来的赢」是
    // 数据的性质，而不是一次竞态。
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
    // 主会话、讨论者（它们就是主会话）与执行者都要做计划；
    // `delegable` 保持默认的 `true`，正是为了让执行者也拿到它。
    // headless 是第三种前端，而这个工具不需要人 —— 不像
    // `ask_user_question`，那是 `can_ask` 唯一把关的工具（spec §7、§19）。
    for can_ask in [false, true] {
        let table = builtin(can_ask);
        assert!(
            table.get(TODO_TOOL).is_some(),
            "内置表也把它摆进去（can_ask = {can_ask}）"
        );
        assert!(
            table.for_executor().get(TODO_TOOL).is_some(),
            "而执行者照样留着它（can_ask = {can_ask}）"
        );
    }
}

#[test]
fn the_identity_tells_the_model_to_keep_a_list() {
    // 规则那一段（spec §3）：是引导，不是强制。这条指令在
    // 每个请求里模型看得见的前缀上，所以*它就是*那段被缓存的前缀 ——
    // 往里加是允许的，改它不行（ADR 0001、ADR 0003）。
    let identity = heng::agent::agent_identity();
    assert!(identity.contains("todo"), "{identity}");
    assert!(identity.contains("pending"), "{identity}");
    assert!(identity.contains("in_progress"), "{identity}");
    assert!(identity.contains("completed"), "{identity}");
    assert!(identity.contains("开工前先把计划写下来"), "{identity}");
}

// --- 走一个真会话 ----------------------------------------------------------

struct Session {
    harness: Harness,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

/// 一个模型照脚本行事的会话，挂的是真工具表。
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
        .expect("流上扛着这次调用");
    assert_eq!(started, args, "参数按写下的样子存着：列表就是这次调用");

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
                Some("todo：2 项（0 项已完成）"),
                "回执是一个确认，不是那份列表"
            );
        }
        other => panic!("期望一条完成，实际得到 {other:?}"),
    }

    session.harness.shutdown().await;
}

#[tokio::test]
async fn two_calls_in_one_message_each_get_a_result_and_the_last_list_wins() {
    // 由 `effect()` 判定并发、由 `seq` 定序：两份列表都落下来，
    // 而读者取的那一份 —— 更晚的那份 —— 才是生效的。
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
    assert_eq!(lists.len(), 2, "两次调用都在流上");

    let results = session
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .count();
    assert_eq!(results, 2, "而每一次都有它那一条结果");

    let in_force = read_items(lists.last().unwrap());
    assert_eq!(in_force[0].content, "新的");
    assert_eq!(in_force[0].status, Status::Completed);

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_real_session_keeps_the_id_in_the_arguments_and_the_sidebar_reads_it_back() {
    use heng::events::ToolCallId;
    use heng::render::todo::TodoPanel;
    use heng::render::{Block, ToolBlock};
    use ratatui::layout::Rect;

    let args = items_with_ids(&[
        ("补测试", "completed", Some("03")),
        ("写文档", "in_progress", Some("04")),
    ]);
    let mut session = session(vec![todo_reply("call-1", &args), Reply::text("好")]).await;
    session.harness.run_turn("开工").await.unwrap();

    let started = session
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } if tool_name == TODO_TOOL => Some(args.clone()),
            _ => None,
        })
        .expect("流上扛着这次调用");
    assert_eq!(
        started, args,
        "id 就在那次调用的参数里 —— 清单的进度正是从这里重算的"
    );

    // 侧栏读到的是同一份 args：它不认识清单，只是把 id 画出来。
    let mut panel = TodoPanel::default();
    panel.observe(&Block::Tool(Box::new(ToolBlock {
        speaker: SpeakerId::Debater("kimi".into()),
        tool_call_id: ToolCallId::new("call-1"),
        tool: TODO_TOOL.to_owned(),
        args: started,
        outcome: None,
    })));
    let rows: Vec<String> = panel
        .lines(Rect::new(0, 0, 28, 4))
        .iter()
        .map(|line| line.to_string())
        .collect();
    assert_eq!(
        rows,
        vec![
            "✓ 03 补测试".to_owned(),
            "▸ 04 写文档".to_owned(),
            "已完成 1/2".to_owned(),
        ]
    );

    session.harness.shutdown().await;
}
