//! server 进程那三件事：**环境白名单**、**可写根**、**stderr 接住**（`.scratch/mcp-support/spec.md`
//! §4；票 13）。
//!
//! 走的是与 `tests/mcp_stdio.rs` 同一条真路径（真 spawn + 沙箱），只是把断言落在子进程的
//! 环境、它能写到哪里、以及它的诊断去哪了。

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use heng::config::{
    McpServerConfig, McpSettings, SandboxAvailability, SandboxMode, SandboxSettings,
};
use heng::mcp::{connect_all, ConnectOptions, McpService, StderrSink};
use heng::tools::Sandbox;
use serde_json::json;

// --- 组装 -----------------------------------------------------------------

fn fake_server() -> &'static str {
    env!("CARGO_BIN_EXE_fake-mcp-server")
}

fn stdio(name: &str) -> McpServerConfig {
    McpServerConfig::stdio(name, vec![fake_server().to_owned()])
}

fn settings(server: McpServerConfig) -> McpSettings {
    McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([(server.name.clone(), server)]),
    }
}

/// 这台机器上真沙箱路径的沙箱；没有可用的 `bwrap` 时给 `None`，调用方跳过。
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

/// 父进程那一份环境快照：整份都交进去，连接层自己只挑白名单那三个键。
fn full_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

async fn connect(
    settings: &McpSettings,
    cwd: &Path,
    sandbox: &Sandbox,
    env: &BTreeMap<String, String>,
    stderr: Option<StderrSink>,
) -> McpService {
    let mut options = ConnectOptions::new(cwd, sandbox, env);
    if let Some(sink) = stderr {
        options = options.with_stderr(sink);
    }
    connect_all(settings, &options).await
}

/// 问假 server 一个环境变量的值。它没有那个变量时回 `(没有)`。
async fn remote_env(service: &McpService, key: &str) -> String {
    service
        .call_tool("fake", "env", json!({ "name": key }))
        .await
        .unwrap()
}

// --- 环境白名单 -----------------------------------------------------------

#[tokio::test]
async fn the_child_gets_the_declared_env_and_nothing_else() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);

    // 父进程里确实有这把「假密钥」——测试进程自己导出的，且会被原样交进环境快照。
    // SAFETY：这个测试进程里没有别的线程在读环境（tokio 的 worker 不读）。
    unsafe { std::env::set_var("MCP_PARENT_ONLY_SECRET", "sk-parent-should-not-travel") };
    let env = full_env();
    assert!(
        env.contains_key("MCP_PARENT_ONLY_SECRET"),
        "前提：父进程环境里有这把密钥"
    );

    let mut server = stdio("fake");
    // 白名单里显式声明的那一条。
    server
        .env
        .insert("MCP_DECLARED".to_owned(), "declared-value".to_owned());
    let service = connect(&settings(server), &cwd, &sandbox, &env, None).await;
    assert_eq!(service.unavailable_reason("fake"), None);

    assert_eq!(
        remote_env(&service, "MCP_DECLARED").await,
        "MCP_DECLARED=declared-value"
    );
    assert_eq!(
        remote_env(&service, "MCP_PARENT_ONLY_SECRET").await,
        "MCP_PARENT_ONLY_SECRET=(没有)",
        "父进程导出的东西不该顺进 server 进程"
    );
}

// --- 可写根 ---------------------------------------------------------------

#[tokio::test]
async fn a_declared_writable_root_is_the_only_way_out_of_the_workspace() {
    let (dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let target = outside.join("cache.txt");
    let env = full_env();

    // 没声明：工作区外写不进去。
    let service = connect(&settings(stdio("fake")), &cwd, &sandbox, &env, None).await;
    assert_eq!(service.unavailable_reason("fake"), None);
    let refused = service
        .call_tool(
            "fake",
            "write",
            json!({ "path": target.display().to_string(), "text": "x" }),
        )
        .await;
    assert!(
        refused.is_err(),
        "没声明可写根时区外写该失败，实际是：{refused:?}"
    );
    assert!(!target.exists(), "沙箱没有让它落到盘上");

    // 声明之后：同一个路径写得进去。
    let mut server = stdio("fake");
    server.writable_roots.push(outside.display().to_string());
    let service = connect(&settings(server), &cwd, &sandbox, &env, None).await;
    assert_eq!(service.unavailable_reason("fake"), None);
    let written = service
        .call_tool(
            "fake",
            "write",
            json!({ "path": target.display().to_string(), "text": "写在区外" }),
        )
        .await
        .expect("声明了可写根，这一笔该成");
    assert!(written.contains("written"), "{written}");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "写在区外");
}

// --- stderr ---------------------------------------------------------------

#[tokio::test]
async fn the_child_stderr_reaches_the_diagnostics() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let env = full_env();

    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink: StderrSink = {
        let lines = Arc::clone(&lines);
        Arc::new(move |line: &str| {
            lines.lock().expect("收集器已中毒").push(line.to_owned());
        })
    };

    let service = connect(&settings(stdio("fake")), &cwd, &sandbox, &env, Some(sink)).await;
    assert_eq!(service.unavailable_reason("fake"), None);
    // 起进程时那一行会先到；给它一点时间穿过管道。
    for _ in 0..50 {
        if !lines.lock().expect("收集器已中毒").is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let collected = lines.lock().expect("收集器已中毒").clone();
    assert!(
        collected
            .iter()
            .any(|line| line.contains("fake-mcp-server")),
        "server 的 stderr 该被接住：{collected:?}"
    );
}

// --- sandbox 位 -----------------------------------------------------------

/// 假 server 往上那一串父进程的名字。
async fn ancestors(service: &McpService) -> String {
    service
        .call_tool("fake", "ancestors", json!({}))
        .await
        .unwrap()
}

#[tokio::test]
async fn sandbox_false_takes_the_server_out_of_bwrap() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let env = full_env();

    // 缺省：过沙箱 —— 假 server 的**直接父进程**就是 `bwrap`。
    let service = connect(&settings(stdio("fake")), &cwd, &sandbox, &env, None).await;
    assert_eq!(service.unavailable_reason("fake"), None);
    let inside = ancestors(&service).await;
    assert_eq!(
        inside.split('>').next().unwrap_or(""),
        "bwrap",
        "缺省该过沙箱，实际祖先链是：{inside}"
    );

    // 显式 `sandbox = false`：同一个 server 的直接父进程不再是 bwrap（这一位**默认不关**）。
    //
    // 只断言**链首**：测试进程自己可能跑在更外层的容器里（这一轮就有一个），链尾出现 `bwrap`
    // 不代表这一层包了。
    let mut server = stdio("fake");
    server.sandbox = false;
    let service = connect(&settings(server), &cwd, &sandbox, &env, None).await;
    assert_eq!(service.unavailable_reason("fake"), None);
    let outside = ancestors(&service).await;
    assert_ne!(
        outside.split('>').next().unwrap_or(""),
        "bwrap",
        "显式关了沙箱，直接父进程却是 bwrap：{outside}"
    );
}
