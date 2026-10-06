//! 真 stdio 的那条路（`.scratch/mcp-support/spec.md` §3；票 12）。
//!
//! 与 `tests/mcp_list.rs` / `tests/mcp_call.rs` / `tests/mcp_resources.rs` 的分工：那几份用
//! **进程内假连接**盖工具层与服务层的行为，这一份用**仓库自己的假 server 二进制**
//! （`tests/support/fake_mcp_server.rs`）盖假连接永远盖不到的事 —— 真 spawn、沙箱包装、协议帧、
//! `Discover` 握手，以及会话结束时那个进程组的清理。
//!
//! **零网络**：只起本地子进程，不发任何真实请求。

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use heng::config::{
    McpServerConfig, McpSettings, SandboxAvailability, SandboxMode, SandboxSettings, SessionConfig,
};
use heng::events::{read_events, Event, EventPayload, SessionId, SpeakerId};
use heng::mcp::{connect_all, ConnectOptions, McpService};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{builtin, with_mcp, Sandbox, MCP_CALL_TOOL, MCP_LIST_TOOL};
use heng::{assemble, AssemblyParts, SessionScaffold};
use serde_json::json;
use support::{CaptureBuf, FakeProvider, Reply};

// --- 组装 -----------------------------------------------------------------

/// 会话环境里白名单那三个键的那一份快照。
fn base_env() -> BTreeMap<String, String> {
    ["PATH", "HOME", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect()
}

/// 起一份连接：会话环境就是上面那一份，stderr 交给测试（真前端交给诊断通道）。
async fn connect(settings: &McpSettings, cwd: &Path, sandbox: &Sandbox) -> McpService {
    let env = base_env();
    let options = ConnectOptions::new(cwd, sandbox, &env);
    connect_all(settings, &options).await
}

/// 仓库自己的假 server 二进制。Cargo 把它的路径喂给集成测试。
fn fake_server() -> &'static str {
    env!("CARGO_BIN_EXE_fake-mcp-server")
}

fn stdio(name: &str, env: &[(&str, &str)]) -> McpServerConfig {
    let mut server = McpServerConfig::stdio(name, vec![fake_server().to_owned()]);
    for (key, value) in env {
        server.env.insert((*key).to_owned(), (*value).to_owned());
    }
    server
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
///
/// 有 `bwrap` 的机器上这条路径才是票 12 要验的那条（真 spawn + 沙箱包装）；没有的机器上，
/// 「不可用就 fail closed」那条由 `tests/sandbox.rs` 覆盖。
fn sandbox_for(cwd: &Path) -> Option<Sandbox> {
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.search_path = std::env::var_os("PATH");
    settings.availability = heng::tools::sandbox::probe(settings.search_path.as_deref(), cwd);
    if matches!(
        settings.availability,
        SandboxAvailability::Unavailable { .. }
    ) {
        return None;
    }
    Some(Sandbox::new(&settings))
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    (dir, cwd)
}

/// 没有 `bwrap` 时的那一句跳过。集中一处，好让每个用例的写法一致。
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

// --- 服务层那条真路 -------------------------------------------------------

#[tokio::test]
async fn a_real_stdio_server_lists_calls_and_reads() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let service = connect(
        &settings(vec![stdio(
            "fake",
            &[("FAKE_MCP_INSTRUCTIONS", "只在工作日接受写入")],
        )]),
        &cwd,
        &sandbox,
    )
    .await;

    assert_eq!(
        service.unavailable_reason("fake"),
        None,
        "真 server 该连上（`Discover` 握手 + 沙箱包装 + spawn）"
    );

    let listings = service.list_tools(Some("fake")).await.unwrap();
    let manifest = listings[0].result.as_ref().expect("这台该有清单");
    assert!(
        manifest.tools.iter().any(|tool| tool.name == "echo"),
        "假 server 声明的工具该列出来：{:?}",
        manifest
            .tools
            .iter()
            .map(|tool| &tool.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        manifest.instructions.as_deref(),
        Some("只在工作日接受写入"),
        "握手时的自述跟着清单回来"
    );

    let text = service
        .call_tool("fake", "echo", json!({ "text": "你好" }))
        .await
        .unwrap();
    assert_eq!(text, "echo: 你好");

    let resources = service.list_resources(Some("fake")).await.unwrap();
    let listed = resources[0].result.as_ref().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].uri, "db://users/42");

    let body = service
        .read_resource("fake", "db://users/42")
        .await
        .unwrap();
    assert_eq!(body, "{\"id\":42}");
}

#[tokio::test]
async fn a_legacy_only_server_is_refused_not_silently_downgraded() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let service = connect(
        &settings(vec![stdio("legacy", &[("FAKE_MCP_LEGACY", "1")])]),
        &cwd,
        &sandbox,
    )
    .await;

    assert!(
        service.unavailable_reason("legacy").is_some(),
        "只认旧版 `initialize` 的 server 在 `Discover` 下是硬错误，不回退"
    );

    let listings = service.list_tools(Some("legacy")).await.unwrap();
    let error = listings[0].result.as_ref().expect_err("这台不可用");
    assert_eq!(error.code_str(), "MCP_SERVER_UNAVAILABLE");
}

#[tokio::test]
async fn one_dead_server_does_not_take_the_others_down() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let dead = McpServerConfig::stdio("dead", vec!["/nonexistent/heng-fake-mcp-server".to_owned()]);
    // 三台一起发：两台好的、一台起不来的（票 12 验证 2）。
    let service = connect(
        &settings(vec![stdio("good", &[]), stdio("also-good", &[]), dead]),
        &cwd,
        &sandbox,
    )
    .await;

    assert_eq!(service.unavailable_reason("good"), None);
    assert_eq!(service.unavailable_reason("also-good"), None);
    assert!(
        service.unavailable_reason("dead").is_some(),
        "起不来的那台被跳过、原因记下来"
    );

    let listings = service.list_tools(None).await.unwrap();
    assert_eq!(listings.len(), 3, "三台各占一条结论");
    for name in ["good", "also-good"] {
        let listing = listings
            .iter()
            .find(|listing| listing.server == name)
            .unwrap();
        assert!(listing.result.is_ok(), "{name} 该照常给清单");
    }
    let dead = listings
        .iter()
        .find(|listing| listing.server == "dead")
        .unwrap();
    assert_eq!(
        dead.result.as_ref().unwrap_err().code_str(),
        "MCP_SERVER_UNAVAILABLE"
    );
}

// --- 工具层那条真路 -------------------------------------------------------

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

fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> Reply {
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

#[tokio::test]
async fn the_meta_tools_reach_a_real_server_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let sandbox = skip_without_bwrap!(&cwd);
    let service = connect(&settings(vec![stdio("fake", &[])]), &cwd, &sandbox).await;

    let log_path = dir.path().join("log.jsonl");
    let provider = FakeProvider::new(vec![
        tool_call("call-1", MCP_LIST_TOOL, json!({ "server": "fake" })),
        tool_call(
            "call-2",
            MCP_CALL_TOOL,
            json!({ "server": "fake", "tool": "echo", "arguments": { "text": "hi" } }),
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
            session_id: SessionId::new("s-mcp-stdio"),
            tools: with_mcp(builtin(false), service),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    harness.run_turn("list and call").await.unwrap();
    let events = read_events(&log_path).unwrap();

    let list = completed_output(&events, "call-1").unwrap();
    assert!(list.contains("echo"), "{list}");
    assert!(list.contains("要回显的东西"), "参数说明也带出来：{list}");

    let call = completed_output(&events, "call-2").unwrap();
    assert!(call.starts_with("[外部内容："), "{call}");
    assert!(call.contains("echo: hi"), "{call}");

    harness.shutdown().await;
}

// --- 提示词模板（`/` 菜单那一半） -----------------------------------------

#[tokio::test]
async fn a_real_server_declares_prompt_templates_for_the_menu() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let service = connect(&settings(vec![stdio("fake", &[])]), &cwd, &sandbox).await;
    assert_eq!(service.unavailable_reason("fake"), None);

    // `/` 菜单要的就是这一份：名字是 `server:模板名`。
    let entries = service.prompt_entries().await;
    let names: Vec<String> = entries
        .iter()
        .map(|(server, prompt)| format!("{server}:{}", prompt.name))
        .collect();
    assert!(names.contains(&"fake:user_report".to_owned()), "{names:?}");
    assert!(names.contains(&"fake:standup".to_owned()), "{names:?}");
    let report = entries
        .iter()
        .find(|(_, prompt)| prompt.name == "user_report")
        .unwrap();
    assert_eq!(report.1.arguments[0].name, "id");
    assert!(report.1.arguments[0].required);

    // 渲染：参数过去，文本回来（它会成为这一轮的一条消息）。
    let rendered = service
        .get_prompt("fake", "user_report", json!({ "id": "42" }))
        .await
        .unwrap();
    assert_eq!(rendered, "请给用户 42 出一份报告");
    let standup = service
        .get_prompt("fake", "standup", json!({}))
        .await
        .unwrap();
    assert_eq!(standup, "把今天做的事写成三条");
}

// --- 进程组清理 -----------------------------------------------------------

/// 这一路心跳在跳吗：文件在，而且 mtime 最近动过。
async fn heartbeat_alive(path: &Path) -> bool {
    for _ in 0..50 {
        if let Ok(modified) = std::fs::metadata(path).and_then(|meta| meta.modified()) {
            if modified.elapsed().unwrap_or_default() < Duration::from_secs(2) {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// 这一路心跳停了吗：连着两个采样点的 mtime 一样（也就是没人再写了）。
///
/// 刻意**不看 pid**：这个沙箱里 pid 命名空间是每次调用一套，`/proc/<pid>` 不指向你以为的那个人。
/// 心跳文件走的还是宿主看得见的那个工作区，所以它是这里唯一可靠的证据。
async fn heartbeat_stopped(path: &Path) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let before = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok();
        tokio::time::sleep(Duration::from_millis(700)).await;
        let after = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok();
        if before.is_some() && before == after {
            return true;
        }
        if tokio::time::Instant::now() > deadline {
            return false;
        }
    }
}

#[tokio::test]
async fn the_whole_process_group_stops_when_the_session_ends() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let self_beat = cwd.join("self.beat");
    let child_beat = cwd.join("child.beat");
    let service = connect(
        &settings(vec![stdio(
            "fake",
            &[
                ("FAKE_MCP_SELF_HEARTBEAT", self_beat.to_str().unwrap()),
                ("FAKE_MCP_CHILD_HEARTBEAT", child_beat.to_str().unwrap()),
            ],
        )]),
        &cwd,
        &sandbox,
    )
    .await;
    assert_eq!(service.unavailable_reason("fake"), None);
    service.list_tools(Some("fake")).await.unwrap();

    assert!(heartbeat_alive(&self_beat).await, "假 server 该在跳");
    assert!(
        heartbeat_alive(&child_beat).await,
        "它 spawn 的那个长跑子进程也该在跳"
    );

    // 关掉这一次连接 —— 会话结束时走的就是这条路（`RunningService` 的 Drop 兜底）。
    drop(service);

    assert!(
        heartbeat_stopped(&self_beat).await,
        "会话结束后假 server 还活着"
    );
    assert!(
        heartbeat_stopped(&child_beat).await,
        "会话结束后它的子进程还在写心跳：整组没被收掉"
    );
}
