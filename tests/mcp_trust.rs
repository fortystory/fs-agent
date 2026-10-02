//! 信任三个位：`trust_effects` / `trust_results` / `sandbox`（`.scratch/mcp-support/spec.md` §6；
//! 票 14）。
//!
//! 三个位**各自默认关、互不牵连**：这台是我们自己写的，不蕴含它的结果可以当指令读，也不蕴含
//! 它可以写任意路径。这一份测试逐个位单独打开，断言另外两处的行为一个都没动。
//!
//! `sandbox` 那一位落在进程层，用真 server 验（`tests/mcp_process.rs` 的
//! `sandbox_false_takes_the_server_out_of_bwrap`）；这里验的是效果与结果那两位。

mod support;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::config::{McpServerConfig, McpSettings, SessionConfig};
use fs_agent::events::{read_events, Decision, Event, EventPayload, SessionId, SpeakerId};
use fs_agent::mcp::{McpConnection, McpError, McpService, ServerManifest};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{builtin, with_mcp, MCP_CALL_TOOL};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::json;
use support::{CaptureBuf, FakeProvider, Reply};

// --- 假的 MCP 连接 --------------------------------------------------------

#[derive(Clone)]
struct FakeMcp {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Result<String, McpError>>>,
    calls: Mutex<Vec<String>>,
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

    fn calls(&self) -> Vec<String> {
        self.inner.calls.lock().expect("假连接已中毒").clone()
    }
}

#[async_trait]
impl McpConnection for FakeMcp {
    async fn list_tools(&self) -> Result<ServerManifest, McpError> {
        Err(McpError::unsupported("列工具"))
    }

    async fn call_tool(
        &self,
        tool: &str,
        _arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        self.inner
            .calls
            .lock()
            .expect("假连接已中毒")
            .push(tool.to_owned());
        self.inner
            .replies
            .lock()
            .expect("假连接已中毒")
            .pop_front()
            .expect("假连接：脚本里的响应已经用完了")
    }
}

/// 一台按 `tune` 调过的 server，挂上假连接。
fn service(
    tune: impl FnOnce(&mut McpServerConfig),
    connection: Arc<dyn McpConnection>,
) -> McpService {
    let mut server = McpServerConfig::stdio("github", vec!["true".to_owned()]);
    tune(&mut server);
    let settings = McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([(server.name.clone(), server)]),
    };
    McpService::new(settings).with_connection("github", connection)
}

// --- 会话 fixture ---------------------------------------------------------

fn call_reply(id: &str, tool: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: MCP_CALL_TOOL.to_owned(),
            arguments: json!({ "server": "github", "tool": tool, "arguments": {} }).to_string(),
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

struct Fixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    /// 在整个测试期间保持活着。
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, mcp: McpService) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mcp-trust"),
            tools: with_mcp(builtin(false), mcp),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(mode),
            asker: None,
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

// --- trust_effects --------------------------------------------------------

#[tokio::test]
async fn readonly_refuses_by_default_and_trust_effects_opens_exactly_the_named_tool() {
    // 缺省：最严 —— `readonly` 档直接拒，连接根本没被碰。
    let strict = FakeMcp::new(vec![Ok("不该发生".to_owned())]);
    let mut refused = fixture(
        vec![call_reply("call-1", "get_issue"), Reply::text("done")],
        Mode::Readonly,
        service(|_| {}, Arc::new(strict.clone())),
    )
    .await;
    refused.run_turn("read it").await;
    assert_eq!(refused.decisions(), vec![Decision::Deny]);
    assert!(strict.calls().is_empty(), "被拒的调用到不了 server");
    refused.shutdown().await;

    // 开了 `trust_effects` 且点名这条工具之后：`readonly` 档放行。
    let trusted = FakeMcp::new(vec![Ok("issue 42".to_owned())]);
    let mut allowed = fixture(
        vec![call_reply("call-1", "get_issue"), Reply::text("done")],
        Mode::Readonly,
        service(
            |server| {
                server.trust_effects = true;
                server.read_only_tools.push("get_issue".to_owned());
            },
            Arc::new(trusted.clone()),
        ),
    )
    .await;
    allowed.run_turn("read it").await;
    assert_eq!(allowed.decisions(), vec![Decision::Allow]);
    assert_eq!(trusted.calls(), vec!["get_issue".to_owned()]);
    allowed.shutdown().await;

    // 名单之外的工具在同一个 server 上照旧最严：放宽是**逐条**的。
    let strict_again = FakeMcp::new(vec![Ok("不该发生".to_owned())]);
    let mut other = fixture(
        vec![call_reply("call-1", "create_issue"), Reply::text("done")],
        Mode::Readonly,
        service(
            |server| {
                server.trust_effects = true;
                server.read_only_tools.push("get_issue".to_owned());
            },
            Arc::new(strict_again.clone()),
        ),
    )
    .await;
    other.run_turn("write it").await;
    assert_eq!(other.decisions(), vec![Decision::Deny]);
    assert!(strict_again.calls().is_empty());
    other.shutdown().await;
}

// --- trust_results --------------------------------------------------------

#[tokio::test]
async fn trust_results_is_the_only_thing_that_drops_the_marker() {
    // 缺省：带标记。
    let plain = FakeMcp::new(vec![Ok("issue 42".to_owned())]);
    let mut marked = fixture(
        vec![call_reply("call-1", "get_issue"), Reply::text("done")],
        Mode::Auto,
        service(|_| {}, Arc::new(plain)),
    )
    .await;
    marked.run_turn("read it").await;
    let output = completed_output(&marked.events(), "call-1").unwrap();
    assert!(output.starts_with("[外部内容："), "{output}");
    marked.shutdown().await;

    // 开了 `trust_results`：原样过去。
    let trusted = FakeMcp::new(vec![Ok("issue 42".to_owned())]);
    let mut bare = fixture(
        vec![call_reply("call-1", "get_issue"), Reply::text("done")],
        Mode::Auto,
        service(|server| server.trust_results = true, Arc::new(trusted)),
    )
    .await;
    bare.run_turn("read it").await;
    let output = completed_output(&bare.events(), "call-1").unwrap();
    assert_eq!(output, "issue 42");
    bare.shutdown().await;
}

// --- 互不牵连 -------------------------------------------------------------

#[tokio::test]
async fn turning_on_results_does_not_widen_the_effect() {
    // 只开 `trust_results`：结果不带标记，但副作用仍是最严 —— `readonly` 档照样拒。
    let connection = FakeMcp::new(vec![Ok("不该发生".to_owned())]);
    let mut fixture = fixture(
        vec![call_reply("call-1", "get_issue"), Reply::text("done")],
        Mode::Readonly,
        service(
            |server| server.trust_results = true,
            Arc::new(connection.clone()),
        ),
    )
    .await;
    fixture.run_turn("read it").await;

    assert_eq!(fixture.decisions(), vec![Decision::Deny]);
    assert!(connection.calls().is_empty());
    fixture.shutdown().await;
}

// --- 配置校验 -------------------------------------------------------------

#[test]
fn a_read_only_list_without_trust_effects_is_a_startup_error() {
    // 「配了等于没配」当场纠正：名单一个字都不会生效的组合不该静默通过。
    let refused = fs_agent::config::resolve(
        Some(
            "[mcp.servers.github]\ncommand = [\"x\"]\n\
             read_only_tools = [\"get_issue\"]\n",
        ),
        &BTreeMap::new(),
    );
    assert!(
        refused.is_err(),
        "没开 `trust_effects` 的名单要被拒：{refused:?}"
    );

    let accepted = fs_agent::config::resolve(
        Some(
            "[mcp.servers.github]\ncommand = [\"x\"]\n\
             trust_effects = true\nread_only_tools = [\"get_issue\"]\n",
        ),
        &BTreeMap::new(),
    )
    .expect("两个一起写就是合法的");
    let server = &accepted.mcp.servers["github"];
    assert!(server.trust_effects);
    assert_eq!(server.read_only_tools, vec!["get_issue".to_owned()]);
}
