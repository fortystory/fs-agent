//! MCP 加载结果进上下文（`.scratch/mcp-support/issues/19-mcp-catalog-in-context.md`）。
//!
//! 两件事：服务层写出的那段文本说清了「这次会话加载了哪些 server、哪些没连上」，以及它确实以
//! **一条** `ContextInjected { source: McpCatalog }` 落进流（详情弹窗读的就是同一条事件）。

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use heng::config::{McpServerConfig, McpSettings, SessionConfig};
use heng::events::{read_events, ContextSource, EventPayload, SessionId, SpeakerId};
use heng::mcp::{McpConnection, McpError, McpService, ServerManifest};
use heng::permissions::{Mode, Policy};
use heng::render::{RenderSinks, Renderer};
use heng::tools::builtin;
use heng::{assemble, AssemblyParts, SessionScaffold};
use support::{CaptureBuf, FakeProvider};

/// 一个最简的假连接：这份测试只关心「这台连上了」这个事实。
struct FakeMcp;

#[async_trait]
impl McpConnection for FakeMcp {
    async fn list_tools(&self) -> Result<ServerManifest, McpError> {
        Err(McpError::unsupported("列工具"))
    }
}

fn service() -> McpService {
    let settings = McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([
            (
                "fake".to_owned(),
                McpServerConfig::stdio("fake", vec!["true".to_owned()]),
            ),
            (
                "broken".to_owned(),
                McpServerConfig::stdio("broken", vec!["nope".to_owned()]),
            ),
        ]),
    };
    McpService::new(settings)
        .with_connection("fake", Arc::new(FakeMcp))
        .with_unavailable("broken", "起不了 server 进程：No such file or directory")
}

#[test]
fn the_catalog_names_every_server_and_says_which_ones_are_down() {
    let text = service().catalog_text().expect("开了开关就该有这一段");
    assert!(text.contains("`fake`（stdio）：已连接"), "{text}");
    assert!(
        text.contains("`broken`（stdio）：连不上 —— 起不了 server 进程"),
        "没连上的那台要点名说清为什么：{text}"
    );
    assert!(text.contains("沙箱：过"), "{text}");
    assert!(text.contains("结果标记：带"), "{text}");
    assert!(text.contains("副作用放宽：不允许"), "{text}");
}

#[test]
fn the_catalog_is_silent_when_the_switch_is_off_or_nothing_is_configured() {
    assert!(
        McpService::new(McpSettings::default())
            .catalog_text()
            .is_none(),
        "缺省关着：不注入这一条，零影响"
    );

    let enabled = McpSettings {
        enabled: true,
        ..McpSettings::default()
    };
    assert!(
        McpService::new(enabled).catalog_text().is_none(),
        "开了开关但一台 server 都没配：也没什么可说的"
    );
}

#[tokio::test]
async fn the_catalog_lands_in_the_stream_as_one_injection() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("log.jsonl");
    let mut harness = assemble(AssemblyParts {
        provider: Box::new(FakeProvider::new(Vec::new())),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mcp-catalog"),
            tools: builtin(false),
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

    let text = service().catalog_text().unwrap();
    harness
        .inject_context(ContextSource::McpCatalog, &text)
        .unwrap();

    let events = read_events(&log_path).unwrap();
    let injected: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ContextInjected { source, content }
                if *source == ContextSource::McpCatalog =>
            {
                Some(content.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(injected, vec![text.as_str()], "恰好一条，正文就是那段");
    harness.shutdown().await;
}
