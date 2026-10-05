//! `fs-agent-mcp-time` 的两条验收路（`.scratch/time-mcp/spec.md` §6.1–§6.2；票 01）。
//!
//! 一路是**裸协议**：spawn 那个二进制、自己按行读写 JSON-RPC —— 看它答得对不对。
//! 另一路是**端到端**：让 fs-agent 自己连它（沙箱包装 + `Discover` 握手 + 协议帧），
//! 再让工具层真调一次。**端到端那一半才是握手形状的真验收**：手写的帧与 client 的期望差一个
//! 字段，它就红。
//!
//! **零网络**：只起本地子进程。

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use fs_agent::config::{
    McpServerConfig, McpSettings, SandboxAvailability, SandboxMode, SandboxSettings, SessionConfig,
};
use fs_agent::events::{read_events, Event, EventPayload, SessionId, SpeakerId};
use fs_agent::mcp::{connect_all, ConnectOptions, McpService};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{builtin, with_mcp, Sandbox, MCP_CALL_TOOL};
use fs_agent::{assemble, AssemblyParts, SessionScaffold};
use serde_json::{json, Value};
use support::{CaptureBuf, FakeProvider, Reply};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

/// 仓库自带的时间 server。Cargo 把它的路径喂给集成测试。
fn server_bin() -> &'static str {
    env!("CARGO_BIN_EXE_fs-agent-mcp-time")
}

// --- 裸协议那条路 ---------------------------------------------------------

/// 一个裸对端：直接对着那个二进制按行写读，中间不经过我们的 client。
struct Raw {
    child: Child,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
}

impl Raw {
    async fn start() -> Self {
        let mut child = tokio::process::Command::new(server_bin())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
        }
    }

    async fn send(&mut self, frame: Value) {
        self.stdin
            .write_all(format!("{frame}\n").as_bytes())
            .await
            .unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn recv(&mut self) -> Value {
        let line = self
            .lines
            .next_line()
            .await
            .unwrap()
            .expect("server 该回一条");
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("不是一条 JSON：{line}（{error}）"))
    }

    /// 关掉 stdin 之后它该自己退出，退出码 0。
    async fn shutdown(mut self) {
        drop(self.stdin);
        let status = self.child.wait().await.unwrap();
        assert_eq!(status.code(), Some(0), "stdin 关掉之后该退出 0");
    }
}

fn request(id: u32, method: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": {} })
}

#[tokio::test]
async fn the_handshake_names_the_one_version_and_the_tools_capability() {
    let mut raw = Raw::start().await;
    raw.send(request(1, "server/discover")).await;
    let reply = raw.recv().await;

    assert_eq!(reply["id"], 1);
    assert_eq!(
        reply["result"]["supportedVersions"],
        json!(["2026-07-28"]),
        "协议只谈这一版：{reply}"
    );
    assert!(
        reply["result"]["capabilities"]["tools"].is_object(),
        "它只声明工具这一种原语：{reply}"
    );
    raw.shutdown().await;
}

#[tokio::test]
async fn the_list_holds_one_tool_with_no_required_arguments() {
    let mut raw = Raw::start().await;
    raw.send(request(1, "tools/list")).await;
    let reply = raw.recv().await;

    let tools = reply["result"]["tools"].as_array().expect("该有一张清单");
    assert_eq!(tools.len(), 1, "这台 server 只有一个工具：{reply}");
    assert_eq!(tools[0]["name"], "get_current_time");
    assert_eq!(tools[0]["inputSchema"]["required"], json!([]));
    assert_eq!(tools[0]["inputSchema"]["properties"], json!({}));
    raw.shutdown().await;
}

#[tokio::test]
async fn a_call_answers_a_local_time_line() {
    let mut raw = Raw::start().await;
    raw.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "get_current_time", "arguments": {} }
    }))
    .await;
    let reply = raw.recv().await;

    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("该回一个 text block：{reply}"));
    assert_looks_like_a_time_line(text);
    raw.shutdown().await;
}

#[tokio::test]
async fn an_unknown_tool_and_an_unknown_method_are_both_minus_32601() {
    let mut raw = Raw::start().await;

    raw.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "nope" }
    }))
    .await;
    assert_eq!(raw.recv().await["error"]["code"], -32601, "没有这个工具");

    raw.send(request(2, "resources/list")).await;
    assert_eq!(raw.recv().await["error"]["code"], -32601, "它没有资源原语");

    raw.shutdown().await;
}

#[tokio::test]
async fn a_notification_gets_no_reply() {
    let mut raw = Raw::start().await;
    // 通知（没有 `id`）先发，紧跟着一条要回应的请求：读回来的第一条必须是后者。
    raw.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    raw.send(request(7, "tools/list")).await;

    assert_eq!(raw.recv().await["id"], 7, "通知不该占用一条响应");
    raw.shutdown().await;
}

#[tokio::test]
async fn the_banner_goes_to_stderr_and_stdout_stays_clean() {
    // stdin 一关它就退出：这一条同时盖住「stdin 结束 → 退出 0」与两路输出的分工。
    let child = tokio::process::Command::new(server_bin())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let output = child.wait_with_output().await.unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("fs-agent-mcp-time"),
        "自述该走 stderr：{:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout 只该出现协议帧：{:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// 「本机现在：YYYY-MM-DD HH:MM:SS ±HH:MM 星期X（时区名）」—— 时区名可以缺席。
///
/// 不引正则：日期那一半交给 `chrono` 自己解析，剩下的按形状断言。
fn assert_looks_like_a_time_line(text: &str) {
    let rest = text
        .strip_prefix("本机现在：")
        .unwrap_or_else(|| panic!("该以「本机现在：」开头：{text}"));

    let stamp = rest.get(..19).unwrap_or_else(|| panic!("太短：{text}"));
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S")
        .unwrap_or_else(|error| panic!("时间戳不是 `%Y-%m-%d %H:%M:%S`：{stamp}（{error}）"));

    let tail = rest[19..]
        .strip_prefix(' ')
        .unwrap_or_else(|| panic!("时间戳后面该有一个空格：{text}"));
    let (offset, rest) = tail
        .split_once(' ')
        .unwrap_or_else(|| panic!("偏移与星期几之间该有空格：{text}"));
    assert_eq!(offset.len(), 6, "偏移该写成 `±HH:MM`：{offset}");
    assert!(offset.starts_with(['+', '-']), "偏移该带符号：{offset}");
    assert_eq!(offset.as_bytes()[3], b':', "偏移该写成 `±HH:MM`：{offset}");

    let weekday = rest.get(..9).unwrap_or_else(|| panic!("缺星期几：{text}"));
    assert!(
        [
            "星期一",
            "星期二",
            "星期三",
            "星期四",
            "星期五",
            "星期六",
            "星期日"
        ]
        .contains(&weekday),
        "星期几该是中文：{weekday}"
    );

    let zone = &rest[9..];
    assert!(
        zone.is_empty() || (zone.starts_with('（') && zone.ends_with('）')),
        "时区名要嘛省掉、要嘛整个括起来：{zone}"
    );
}

// --- 端到端那条路 ---------------------------------------------------------

/// 会话环境里白名单那三个键的那一份快照（照 `tests/mcp_stdio.rs`）。
fn base_env() -> BTreeMap<String, String> {
    ["PATH", "HOME", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect()
}

async fn connect(settings: &McpSettings, cwd: &Path, sandbox: &Sandbox) -> McpService {
    let env = base_env();
    let options = ConnectOptions::new(cwd, sandbox, &env);
    connect_all(settings, &options).await
}

/// 一台叫 `time` 的 stdio server，就是那个二进制。
fn time_server() -> McpServerConfig {
    McpServerConfig::stdio("time", vec![server_bin().to_owned()])
}

fn settings(servers: Vec<McpServerConfig>) -> McpSettings {
    McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: servers
            .into_iter()
            .map(|server| (server.name.clone(), server))
            .collect(),
    }
}

/// 这台机器上**真沙箱路径**的沙箱；没有可用的 `bwrap` 时给 `None`，调用方跳过。
fn sandbox_for(cwd: &Path) -> Option<Sandbox> {
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.search_path = std::env::var_os("PATH");
    settings.availability = fs_agent::tools::sandbox::probe(settings.search_path.as_deref(), cwd);
    if matches!(
        settings.availability,
        SandboxAvailability::Unavailable { .. }
    ) {
        return None;
    }
    Some(Sandbox::new(&settings))
}

macro_rules! skip_without_bwrap {
    ($cwd:expr) => {
        match sandbox_for($cwd) {
            Some(sandbox) => sandbox,
            None => {
                eprintln!("跳过：这台机器上没有可用的 bwrap");
                return;
            }
        }
    };
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    (dir, cwd)
}

#[tokio::test]
async fn the_service_reaches_the_real_server_end_to_end() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let service = connect(&settings(vec![time_server()]), &cwd, &sandbox).await;

    assert_eq!(
        service.unavailable_reason("time"),
        None,
        "真 server 该连上（`Discover` 握手 + 沙箱包装 + spawn）"
    );

    let listings = service.list_tools(Some("time")).await.unwrap();
    let manifest = listings[0].result.as_ref().expect("这台该有清单");
    assert!(
        manifest
            .tools
            .iter()
            .any(|tool| tool.name == "get_current_time"),
        "工具该列出来：{:?}",
        manifest
            .tools
            .iter()
            .map(|tool| &tool.name)
            .collect::<Vec<_>>()
    );

    let text = service
        .call_tool("time", "get_current_time", json!({}))
        .await
        .unwrap();
    assert_looks_like_a_time_line(&text);
}

#[tokio::test]
async fn the_meta_tool_brings_the_time_back_into_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let sandbox = skip_without_bwrap!(&cwd);
    let service = connect(&settings(vec![time_server()]), &cwd, &sandbox).await;

    let log_path = dir.path().join("log.jsonl");
    let provider = FakeProvider::new(vec![
        tool_call(
            "call-1",
            MCP_CALL_TOOL,
            json!({ "server": "time", "tool": "get_current_time", "arguments": {} }),
        ),
        Reply::text("done"),
    ]);
    let mut harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-time-mcp"),
            tools: with_mcp(builtin(false), service),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    harness.run_turn("现在几点").await.unwrap();
    let events = read_events(&log_path).unwrap();

    let output = completed_output(&events, "call-1").unwrap();
    assert!(output.starts_with("[外部内容："), "{output}");
    let line = output
        .lines()
        .find(|line| line.starts_with("本机现在："))
        .unwrap_or_else(|| panic!("结果里该有时间那一行：{output}"));
    assert_looks_like_a_time_line(line);
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

fn tool_call(id: &str, name: &str, arguments: Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}
