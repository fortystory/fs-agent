//! `mcp_list` 元工具与它下面那一层服务：组装期开关、清单的形状、每次现问、结构化错误，
//! 以及它在权限门下的待遇（`.scratch/mcp-support/spec.md` §1–§2、§5、§7；票 10）。
//!
//! 没有真连接：MCP 连接层是组装期注入的一个进程内假连接，与 `src/web/` 那条「后端全部是
//! 假的」规矩同一档。真 stdio 的那条路走 `tests/mcp_stdio.rs`。

mod support;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use heng::config::{McpServerConfig, McpSettings};
use heng::events::{Decision, Event, EventPayload, SessionId, SpeakerId, read_events};
use heng::mcp::{McpConnection, McpError, McpService, ServerManifest, ToolSummary};
use heng::permissions::{Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{MCP_LIST_TOOL, builtin, with_mcp};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use serde_json::json;
use support::{CaptureBuf, FakeProvider, Reply, ScriptedAsker};

// --- 假的 MCP 连接 --------------------------------------------------------

/// 按脚本作答、并记下每一次清单询问的连接。
#[derive(Clone)]
struct FakeMcp {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Result<ServerManifest, McpError>>>,
    lists: Mutex<Vec<()>>,
}

impl FakeMcp {
    fn new(replies: Vec<Result<ServerManifest, McpError>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                replies: Mutex::new(replies.into()),
                lists: Mutex::new(Vec::new()),
            }),
        }
    }

    /// 一台永远只报同一份清单的 server。
    fn fixed(manifest: ServerManifest) -> Self {
        Self::new(vec![Ok(manifest)])
    }

    fn list_calls(&self) -> usize {
        self.inner.lists.lock().expect("假连接已中毒").len()
    }
}

#[async_trait]
impl McpConnection for FakeMcp {
    async fn list_tools(&self) -> Result<ServerManifest, McpError> {
        self.inner.lists.lock().expect("假连接已中毒").push(());
        self.inner
            .replies
            .lock()
            .expect("假连接已中毒")
            .pop_front()
            .expect("假连接：脚本里的响应已经用完了")
    }
}

fn tool(name: &str, description: &str) -> ToolSummary {
    ToolSummary {
        name: name.to_owned(),
        description: Some(description.to_owned()),
        schema: json!({
            "type": "object",
            "properties": { "title": { "type": "string" } },
            "required": ["title"]
        }),
    }
}

fn manifest(server: &str, tools: Vec<ToolSummary>) -> ServerManifest {
    ServerManifest {
        server: server.to_owned(),
        instructions: Some("这台 server 只在工作日接受写入。".to_owned()),
        tools,
    }
}

/// 一台开了 `[mcp] enabled` 的设置，点名 `github` 这台 server。
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

/// 一个开了开关、挂上一台假连接的服务。
fn service(connection: Option<Arc<dyn McpConnection>>) -> McpService {
    match connection {
        Some(connection) => McpService::new(settings()).with_connection("github", connection),
        None => McpService::new(settings()),
    }
}

// --- 会话 fixture ---------------------------------------------------------

fn list_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: MCP_LIST_TOOL.to_owned(),
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
    /// 在整个测试期间保持活着。
    _dir: tempfile::TempDir,
}

async fn fixture(
    replies: Vec<Reply>,
    mode: Mode,
    asker: Option<Arc<dyn Asker>>,
    mcp: McpService,
) -> Fixture {
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
        config: heng::config::SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mcp"),
            // 组装期的那一步：`enabled` 与连接挂没挂是两件事。
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
        _dir: dir,
    }
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<(Decision, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided {
                    decision, reason, ..
                } => Some((*decision, reason.clone())),
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

// --- 注册与开关 -----------------------------------------------------------

#[test]
fn the_tools_stay_out_of_the_table_until_the_config_turns_it_on() {
    let off = with_mcp(builtin(false), McpService::new(McpSettings::default()));
    assert!(
        off.get(MCP_LIST_TOOL).is_none(),
        "缺省 `enabled = false`：元工具不进表"
    );

    // 一台连接都没挂时工具**仍在**表里：表不随连接状态抖动，调用给出的是结构化错误。
    let on = with_mcp(builtin(false), service(None));
    assert!(on.get(MCP_LIST_TOOL).is_some());
    assert!(
        on.for_executor().get(MCP_LIST_TOOL).is_some(),
        "执行者也拿到它（spec §9）"
    );
}

// --- 清单的形状 -----------------------------------------------------------

#[tokio::test]
async fn mcp_list_returns_the_tools_and_the_instructions_with_the_untrusted_marker() {
    let connection = FakeMcp::fixed(manifest(
        "github",
        vec![
            tool("create_issue", "创建一个 issue"),
            tool("get_issue", "读一个 issue"),
        ],
    ));
    let mut fixture = fixture(
        vec![
            list_reply("call-1", json!({ "server": "github" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection))),
    )
    .await;
    fixture.run_turn("list").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.starts_with("[外部内容："), "{output}");
    assert!(output.contains("create_issue"), "{output}");
    assert!(output.contains("get_issue"), "{output}");
    assert!(
        output.contains("创建一个 issue"),
        "工具的说明要带出来：{output}"
    );
    assert!(
        output.contains("这台 server 只在工作日接受写入。"),
        "server 的 instructions 放在结果里、不进系统提示词：{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn each_call_asks_the_server_again() {
    let connection = FakeMcp::new(vec![
        Ok(manifest(
            "github",
            vec![tool("create_issue", "创建一个 issue")],
        )),
        Ok(manifest(
            "github",
            vec![tool("create_issue", "创建一个 issue")],
        )),
    ]);
    let mut fixture = fixture(
        vec![
            list_reply("call-1", json!({ "server": "github" })),
            list_reply("call-2", json!({ "server": "github" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection.clone()))),
    )
    .await;
    fixture.run_turn("list twice").await;

    assert_eq!(
        connection.list_calls(),
        2,
        "每次现问：没有缓存就没有失效（决策票 05）"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_missing_server_name_lists_every_configured_server() {
    let connection = FakeMcp::fixed(manifest("github", vec![tool("create_issue", "建 issue")]));
    let mut fixture = fixture(
        vec![list_reply("call-1", json!({})), Reply::text("done")],
        Mode::Auto,
        None,
        service(Some(Arc::new(connection))),
    )
    .await;
    fixture.run_turn("list all").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("github"), "{output}");
    assert!(output.contains("create_issue"), "{output}");
    fixture.shutdown().await;
}

// --- 结构化错误 -----------------------------------------------------------

#[tokio::test]
async fn an_unknown_server_is_a_readable_result_with_exactly_one_event() {
    let mut fixture = fixture(
        vec![
            list_reply("call-1", json!({ "server": "nope" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(None),
    )
    .await;
    fixture.run_turn("list").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(output.contains("MCP_UNKNOWN_SERVER"), "{output}");
    assert!(output.contains("nope"), "点名是哪个 server：{output}");
    assert_eq!(completed_count(&events, "call-1"), 1);
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_server_without_a_connection_is_a_structured_error_not_a_missing_entry() {
    let mut fixture = fixture(
        vec![
            list_reply("call-1", json!({ "server": "github" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(None),
    )
    .await;
    fixture.run_turn("list").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("MCP_SERVER_UNAVAILABLE"), "{output}");
    assert!(output.contains("github"), "{output}");
    fixture.shutdown().await;
}

// --- 权限 -----------------------------------------------------------------

#[tokio::test]
async fn readonly_allows_listing_and_ask_does_not_interrupt_it() {
    let readonly_connection = FakeMcp::fixed(manifest("github", vec![tool("create_issue", "建")]));
    let mut readonly = fixture(
        vec![
            list_reply("call-1", json!({ "server": "github" })),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
        service(Some(Arc::new(readonly_connection))),
    )
    .await;
    readonly.run_turn("list").await;
    let decisions = readonly.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Allow);
    readonly.shutdown().await;

    let connection = FakeMcp::fixed(manifest("github", vec![tool("create_issue", "建")]));
    let asker = ScriptedAsker::new(Vec::new());
    let mut ask = fixture(
        vec![
            list_reply("call-1", json!({ "server": "github" })),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
        service(Some(Arc::new(connection))),
    )
    .await;
    ask.run_turn("list").await;
    assert!(
        asker.requests().is_empty(),
        "`ask` 档下一次只读的清单不该打断人：{:?}",
        asker.requests()
    );
    assert_eq!(ask.decisions()[0].0, Decision::Allow);
    ask.shutdown().await;
}

// --- 配置：两个来源与优先级 -----------------------------------------------

fn resolved_mcp(toml_text: &str) -> McpSettings {
    heng::config::resolve(Some(toml_text), &BTreeMap::new())
        .expect("这份配置是合法的")
        .mcp
}

#[test]
fn mcp_is_off_by_default_and_reads_the_settings_when_present() {
    let off = resolved_mcp("");
    assert!(!off.enabled, "缺省关：不开这一层就是零影响");
    assert!(off.servers.is_empty());

    let on = resolved_mcp(
        r#"
[mcp]
enabled = true
connect_timeout_ms = 2500

[mcp.servers.github]
transport = "stdio"
command = ["npx", "-y", "server-github"]
env = { GITHUB_TOKEN = "s3cret" }
writable_roots = ["~/.cache/github-mcp"]
"#,
    );
    assert!(on.enabled);
    assert_eq!(on.connect_timeout_ms, 2500);
    let github = &on.servers["github"];
    assert_eq!(github.command, vec!["npx", "-y", "server-github"]);
    assert_eq!(github.env["GITHUB_TOKEN"], "s3cret");
    assert!(github.sandbox, "缺省过沙箱");
    assert!(
        !github.trust_results && !github.trust_effects,
        "三个信任位各自缺省关"
    );
}

#[test]
fn the_project_file_wins_over_the_user_level_record() {
    let mut settings = resolved_mcp(
        r#"
[mcp]
enabled = true

[mcp.servers.github]
command = ["user-github"]

[mcp.servers.jira]
url = "https://jira.example.com/mcp"
"#,
    );
    assert_eq!(settings.servers.len(), 2);

    heng::config::apply_project_mcp(
        &mut settings,
        r#"{"mcpServers": {"github": {"command": ["project-github"]}}}"#,
    )
    .expect(".mcp.json 是合法的");

    assert_eq!(
        settings.servers["github"].command,
        vec!["project-github"],
        "同名时项目级盖用户级"
    );
    assert!(
        settings.servers.contains_key("jira"),
        "项目级没提到的那台留着 —— 覆盖是逐台的，不是一个整体开关"
    );
}

#[test]
fn an_unknown_key_in_either_source_is_refused() {
    let user = heng::config::resolve(
        Some("[mcp.servers.github]\ncommand = [\"x\"]\ntrust_result = true\n"),
        &BTreeMap::new(),
    );
    assert!(user.is_err(), "少一个 `s` 的信任位要被拒：{user:?}");

    let section = heng::config::resolve(Some("[mcp]\nenable = true\n"), &BTreeMap::new());
    assert!(section.is_err(), "[mcp] 段里的未知键要被拒：{section:?}");

    let project = heng::config::project_mcp_servers(
        r#"{"mcpServers": {"github": {"command": ["x"], "trusted": true}}}"#,
    );
    assert!(project.is_err(), ".mcp.json 里的未知键要被拒：{project:?}");

    let outer = heng::config::project_mcp_servers(r#"{"servers": {}}"#);
    assert!(outer.is_err(), "外层键只认 `mcpServers`：{outer:?}");
}

#[test]
fn the_project_file_is_read_from_the_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".mcp.json");
    std::fs::write(
        &path,
        r#"{"mcpServers": {"github": {"command": ["from-project"]}}}"#,
    )
    .unwrap();

    // 开着开关：仓库根的那一份生效。
    let mut on = heng::config::resolve(Some("[mcp]\nenabled = true\n"), &BTreeMap::new()).unwrap();
    on.load_project_mcp(dir.path())
        .expect("这份 .mcp.json 是合法的");
    assert_eq!(on.mcp.servers["github"].command, vec!["from-project"]);

    // 关着开关：连文件都不看 —— 一份坏掉的 `.mcp.json` 也影响不到不做它的会话。
    std::fs::write(&path, "{ 这不是 JSON").unwrap();
    let mut off = heng::config::resolve(None, &BTreeMap::new()).unwrap();
    off.load_project_mcp(dir.path())
        .expect("不开这一层就不读文件");
    assert!(off.mcp.servers.is_empty());

    // 开着开关而文件坏了：启动错误，而不是等到模型第一次调用。
    let mut broken =
        heng::config::resolve(Some("[mcp]\nenabled = true\n"), &BTreeMap::new()).unwrap();
    assert!(broken.load_project_mcp(dir.path()).is_err());
}

#[test]
fn a_broken_server_record_fails_at_startup() {
    let no_command = heng::config::resolve(
        Some("[mcp.servers.github]\ntransport = \"stdio\"\n"),
        &BTreeMap::new(),
    );
    assert!(
        no_command.is_err(),
        "点名 stdio 却没有 command 是启动错误：{no_command:?}"
    );

    let unknown_transport = heng::config::resolve(
        Some("[mcp.servers.github]\ntransport = \"sse\"\ncommand = [\"x\"]\n"),
        &BTreeMap::new(),
    );
    assert!(
        unknown_transport.is_err(),
        "不认识的传输名是启动错误：{unknown_transport:?}"
    );

    let both = heng::config::resolve(
        Some("[mcp.servers.github]\ncommand = [\"x\"]\nurl = \"https://a/mcp\"\n"),
        &BTreeMap::new(),
    );
    assert!(both.is_err(), "两种传输的字段都写了，推不出来：{both:?}");
}

#[test]
fn two_servers_with_the_same_name_are_a_startup_error() {
    // 同名意味着「我以为两台都活着、其实只有一台」。TOML 自己就拒重复表，于是这是一条启动
    // 错误，而不是最后一台静默盖掉第一台（票 12 验证 3）。
    let duplicated = heng::config::resolve(
        Some(
            "[mcp.servers.github]\ncommand = [\"a\"]\n\n\
             [mcp.servers.github]\ncommand = [\"b\"]\n",
        ),
        &BTreeMap::new(),
    );
    assert!(
        duplicated.is_err(),
        "同名 server 是启动错误：{duplicated:?}"
    );

    // `.mcp.json` 那一侧：JSON 没有「拒重复表」的语法，由 `UniqueServerMap` 挡。
    let duplicated_json = heng::config::project_mcp_servers(
        r#"{"mcpServers": {"github": {"command": ["a"]}, "github": {"command": ["b"]}}}"#,
    );
    assert!(
        duplicated_json.is_err(),
        "JSON 里同名也要被拒：{duplicated_json:?}"
    );
}
