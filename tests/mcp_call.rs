//! `mcp_call` 元工具：参数原样透传、默认最严的副作用、结果带不可信标记、上限走既有流水线
//! （`.scratch/mcp-support/spec.md` §2、§6、§7；票 11）。

mod support;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use heng::config::{McpServerConfig, McpSettings, SessionConfig};
use heng::context::TRUNCATED_MARKER;
use heng::events::{read_events, Decision, Event, EventPayload, SessionId, SpeakerId};
use heng::mcp::{McpConnection, McpError, McpService};
use heng::permissions::{Answer, Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{builtin, with_mcp, MCP_CALL_TOOL};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::json;
use support::{CaptureBuf, FakeProvider, Reply, ScriptedAsker};

// --- 假的 MCP 连接 --------------------------------------------------------

/// 记下每一次调用（工具名 + 实参），并按脚本回一条结果。
#[derive(Clone)]
struct FakeMcp {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Result<String, McpError>>>,
    calls: Mutex<Vec<(String, serde_json::Value)>>,
}

impl FakeMcp {
    fn new(replies: Vec<Result<String, McpError>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                replies: Mutex::new(replies.into()),
                calls: Mutex::new(Vec::new()),
            }),
        }
    }

    fn calls(&self) -> Vec<(String, serde_json::Value)> {
        self.inner.calls.lock().expect("假连接已中毒").clone()
    }
}

#[async_trait]
impl McpConnection for FakeMcp {
    async fn list_tools(&self) -> Result<heng::mcp::ServerManifest, McpError> {
        Err(McpError::unsupported("列工具"))
    }

    async fn call_tool(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        self.inner
            .calls
            .lock()
            .expect("假连接已中毒")
            .push((tool.to_owned(), arguments));
        self.inner
            .replies
            .lock()
            .expect("假连接已中毒")
            .pop_front()
            .expect("假连接：脚本里的响应已经用完了")
    }
}

fn settings() -> McpSettings {
    McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([(
            "github".to_owned(),
            McpServerConfig::stdio("github", vec!["true".to_owned()]),
        )]),
    }
}

fn service(connection: Option<Arc<dyn McpConnection>>) -> McpService {
    match connection {
        Some(connection) => McpService::new(settings()).with_connection("github", connection),
        None => McpService::new(settings()),
    }
}

// --- 会话 fixture ---------------------------------------------------------

fn call_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: MCP_CALL_TOOL.to_owned(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn completed_output(events: &[Event], tool_call_id: &str) -> Result<String, String> {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok,
                output,
                error,
                ..
            } if id.as_str() == tool_call_id => Some(if *ok {
                Ok(output.clone().unwrap_or_default())
            } else {
                Err(error.clone().unwrap_or_default())
            }),
            _ => None,
        })
        .expect("这次调用正好有一条结果")
}

fn completed_count(events: &[Event], tool_call_id: &str) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(&event.payload, EventPayload::ToolCallCompleted { tool_call_id: id, .. }
                if id.as_str() == tool_call_id)
        })
        .count()
}

struct Fixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    outputs: PathBuf,
    /// 在整个测试期间保持活着。
    _dir: tempfile::TempDir,
}

async fn fixture(
    replies: Vec<Reply>,
    mode: Mode,
    asker: Option<Arc<dyn Asker>>,
    mcp: McpService,
    session: SessionConfig,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    let outputs = session_dir.join("outputs");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&outputs).unwrap();
    let log_path = session_dir.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: session,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mcp-call"),
            tools: with_mcp(builtin(false), mcp),
            locks: heng::tools::PathLocks::new(),
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
        harness: Some(harness),
        log_path,
        outputs,
        _dir: dir,
    }
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<Decision> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided { decision, .. } => Some(*decision),
                _ => None,
            })
            .collect()
    }

    async fn run_turn(&mut self, input: &str) {
        self.harness
            .as_mut()
            .expect("harness 已经关掉了")
            .run_turn(input)
            .await
            .unwrap();
    }

    async fn shutdown(&mut self) {
        if let Some(harness) = self.harness.take() {
            harness.shutdown().await;
        }
    }
}

fn session() -> SessionConfig {
    SessionConfig::new("fake-model")
}

// --- 透传 -----------------------------------------------------------------

#[tokio::test]
async fn arguments_are_passed_through_untouched() {
    let connection = FakeMcp::new(vec![
        Ok("issue 42 建好了".to_owned()),
        Ok("ok".to_owned()),
        Ok("ok".to_owned()),
    ]);
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({
                    "server": "github",
                    "tool": "create_issue",
                    "arguments": { "title": "标题", "labels": ["bug", "p1"], "nested": { "a": 1 } }
                }),
            ),
            // 不给 `arguments`：透传一个空对象，而不是报错。
            call_reply(
                "call-2",
                json!({ "server": "github", "tool": "list_labels" }),
            ),
            // 空对象是它自己的形状，原样过去。
            call_reply(
                "call-3",
                json!({ "server": "github", "tool": "list_labels", "arguments": {} }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection.clone()))),
        session(),
    )
    .await;
    fixture.run_turn("call it").await;

    let calls = connection.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].0, "create_issue");
    assert_eq!(
        calls[0].1,
        json!({ "title": "标题", "labels": ["bug", "p1"], "nested": { "a": 1 } }),
        "实参原样透传，含嵌套对象"
    );
    assert_eq!(calls[1].1, json!({}), "缺席的 `arguments` 是一个空对象");
    assert_eq!(calls[2].1, json!({}));
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_missing_server_or_tool_is_a_parameter_error() {
    let connection = FakeMcp::new(Vec::new());
    let mut fixture = fixture(
        vec![
            call_reply("call-1", json!({ "tool": "create_issue" })),
            call_reply("call-2", json!({ "server": "github" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection.clone()))),
        session(),
    )
    .await;
    fixture.run_turn("bad calls").await;

    let events = fixture.events();
    for (id, needle) in [("call-1", "server"), ("call-2", "tool")] {
        let error = completed_output(&events, id).unwrap_err();
        assert!(error.contains(needle), "{id}：{error}");
    }
    assert!(
        connection.calls().is_empty(),
        "两次都在到 server 之前被拒：{:?}",
        connection.calls()
    );
    fixture.shutdown().await;
}

// --- 副作用默认最严 -------------------------------------------------------

#[tokio::test]
async fn readonly_refuses_an_external_call() {
    let connection = FakeMcp::new(vec![Ok("不该发生".to_owned())]);
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({ "server": "github", "tool": "create_issue", "arguments": {} }),
            ),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
        service(Some(Arc::new(connection.clone()))),
        session(),
    )
    .await;
    fixture.run_turn("write something").await;

    assert_eq!(fixture.decisions(), vec![Decision::Deny]);
    assert!(
        connection.calls().is_empty(),
        "被拒的调用到不了 server：{:?}",
        connection.calls()
    );
    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(!error.is_empty(), "拒绝要有一条可读结果");
    fixture.shutdown().await;
}

#[tokio::test]
async fn ask_mode_asks_once_and_then_really_calls() {
    let connection = FakeMcp::new(vec![Ok("issue 42 建好了".to_owned())]);
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({ "server": "github", "tool": "create_issue", "arguments": { "title": "x" } }),
            ),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
        service(Some(Arc::new(connection.clone()))),
        session(),
    )
    .await;
    fixture.run_turn("write something").await;

    assert_eq!(asker.requests().len(), 1, "外来工具默认每次都要过门");
    assert_eq!(fixture.decisions(), vec![Decision::Allow]);
    assert_eq!(connection.calls().len(), 1, "批准之后真的调了");
    fixture.shutdown().await;
}

// --- 结果 -----------------------------------------------------------------

#[tokio::test]
async fn the_result_carries_the_untrusted_marker() {
    let connection = FakeMcp::new(vec![Ok("issue 42：标题是 x".to_owned())]);
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({ "server": "github", "tool": "get_issue", "arguments": { "number": 42 } }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection))),
        session(),
    )
    .await;
    fixture.run_turn("read it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.starts_with("[外部内容："), "{output}");
    assert!(output.contains("issue 42：标题是 x"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_server_side_failure_is_a_readable_result_with_exactly_one_event() {
    let connection = FakeMcp::new(vec![Err(McpError::tool_error(
        "github",
        "create_issue",
        "422：title 是空的",
    ))]);
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({ "server": "github", "tool": "create_issue", "arguments": {} }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection))),
        session(),
    )
    .await;
    fixture.run_turn("call it").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(output.contains("MCP_TOOL_ERROR"), "{output}");
    assert!(output.contains("422：title 是空的"), "{output}");
    assert_eq!(completed_count(&events, "call-1"), 1);
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_unknown_server_is_a_readable_result() {
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({ "server": "nope", "tool": "create_issue", "arguments": {} }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(None),
        session(),
    )
    .await;
    fixture.run_turn("call it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("MCP_UNKNOWN_SERVER"), "{output}");
    assert!(output.contains("nope"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_oversized_result_spills_before_it_reaches_the_stream() {
    let connection = FakeMcp::new(vec![Ok("x".repeat(4000))]);
    let mut fixture = fixture(
        vec![
            call_reply(
                "call-1",
                json!({ "server": "github", "tool": "dump", "arguments": {} }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection))),
        SessionConfig::new("fake-model").with_max_tool_result_tokens(50),
    )
    .await;
    fixture.run_turn("dump it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains(TRUNCATED_MARKER), "{output}");
    let pointer = fixture.outputs.join("call-1.txt");
    assert!(
        output.contains(&pointer.display().to_string()),
        "流上扛的是那个指针：{output}"
    );
    assert!(
        std::fs::read_to_string(&pointer)
            .unwrap()
            .contains(&"x".repeat(100)),
        "整个正文都在磁盘上"
    );
    fixture.shutdown().await;
}

// --- 谁能用 ---------------------------------------------------------------

#[test]
fn the_executor_table_has_mcp_call() {
    let table = with_mcp(builtin(false), service(None));
    assert!(
        table.for_executor().get(MCP_CALL_TOOL).is_some(),
        "执行者也拿得到它（spec §9）"
    );
}
