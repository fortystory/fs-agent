//! 第四档 `workspace`：区内自动、区外要问，以及「没有沙箱就没有这一档」
//! （`.scratch/workspace-mode/spec.md` §1、§2、§3、§6）。
//!
//! 纯函数那一层在 [`permission_gate`](../permission_gate.rs) 里；这里跑的是端到端：
//! 文件工具的区外写真的弹一次问、批准后真的写进去，以及无沙箱平台上这一档在组装期
//! 就被拒。

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs_agent::config::{SandboxAvailability, SandboxMode, SandboxSettings, SessionConfig};
use fs_agent::events::{Decision, Event, EventPayload, SessionId, SpeakerId};
use fs_agent::permissions::{Answer, Asker, Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{builtin, PathLocks};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{sandbox_available, CaptureBuf, FakeProvider, Reply, ScriptedAsker};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        fs_agent::events::read_events(&self.log_path).unwrap()
    }

    /// 每一次已完成的调用对应的 `(ok, output_or_error)`。
    fn results(&self) -> Vec<(bool, String)> {
        self.events()
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
            .collect()
    }
}

/// 一份脚手架。回复在**工作区建好之后**才构造 —— 测试要拿工作区里（或它旁边）的真实路径
/// 去写调用参数，所以这里收一个闭包而不是一份现成的回复。
async fn fixture_with<F>(
    sandbox: SandboxSettings,
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
    replies: F,
) -> Fixture
where
    F: FnOnce(&Path) -> Vec<Reply>,
{
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let replies = replies(&workspace);
    let log_path = session.join("log.jsonl");
    let mut config = SessionConfig::new("fake-model");
    config.sandbox = sandbox;

    let harness = assemble(AssemblyParts {
        provider: Box::new(FakeProvider::new(replies)),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-workspace"),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy,
            asker,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        log_path,
        workspace,
        _dir: dir,
    }
}

fn call(id: &str, tool: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: tool.into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

// --- 区外写：事前问一次（spec §3）-----------------------------------------

#[tokio::test]
async fn a_workspace_write_outside_asks_once_and_then_really_writes() {
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture_with(
        sandbox_available(),
        Policy::for_mode(Mode::Workspace),
        Some(Arc::new(asker.clone())),
        |workspace| {
            let outside = workspace.parent().unwrap().join("outside.txt");
            vec![
                call(
                    "call-1",
                    "write_file",
                    serde_json::json!({
                        "file_path": outside.display().to_string(),
                        "content": "hello"
                    }),
                ),
                Reply::text("wrote it"),
            ]
        },
    )
    .await;
    let outside = fixture.workspace.parent().unwrap().join("outside.txt");

    fixture.harness.run_turn("write it").await.unwrap();

    let (ok, message) = fixture.results().remove(0);
    assert!(ok, "{message}");
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "hello",
        "批准之后这一次调用照常跑，工作区之外的文件真的写下去了"
    );

    let requests = asker.requests();
    assert_eq!(requests.len(), 1, "区外写只问一次");
    assert!(
        requests[0].reason.contains("路径上限（写）"),
        "{}",
        requests[0].reason
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_workspace_write_inside_never_asks() {
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture_with(
        sandbox_available(),
        Policy::for_mode(Mode::Workspace),
        Some(Arc::new(asker.clone())),
        |workspace| {
            vec![
                call(
                    "call-1",
                    "write_file",
                    serde_json::json!({
                        "file_path": workspace.join("notes.txt").display().to_string(),
                        "content": "hi"
                    }),
                ),
                Reply::text("wrote it"),
            ]
        },
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();

    let (ok, message) = fixture.results().remove(0);
    assert!(ok, "{message}");
    assert!(
        asker.requests().is_empty(),
        "工作区内的写一路放行，这正是这一档存在的意义：{:?}",
        asker.requests()
    );

    fixture.harness.shutdown().await;
}

// --- 区外读：那条与档位正交的旋钮（spec §2）--------------------------------

#[tokio::test]
async fn outside_read_defaults_to_deny_and_can_be_opened() {
    // 缺省 deny：读区外文件连问都不问，直接拒。
    let denied = outside_read_case(Decision::Deny, None).await;
    assert!(!denied.0, "缺省仍然是拒绝：{}", denied.1);
    assert!(denied.2.is_empty(), "拒绝不是一次询问");

    // ask：问一次，批准后读得到。
    let asked = outside_read_case(Decision::Ask, Some(Answer::Allow)).await;
    assert!(asked.0, "{}", asked.1);
    assert_eq!(asked.2.len(), 1, "配成 ask 时弹一次问");
    assert!(
        asked.2[0].reason.contains("路径上限（读）"),
        "{}",
        asked.2[0].reason
    );

    // allow：不问，直接读得到。
    let allowed = outside_read_case(Decision::Allow, None).await;
    assert!(allowed.0, "{}", allowed.1);
    assert!(allowed.2.is_empty(), "配成 allow 时不问");
}

/// 一次区外读：返回 `(ok, message, 收到的那几次询问)`。
async fn outside_read_case(
    outside_read: Decision,
    answer: Option<Answer>,
) -> (bool, String, Vec<fs_agent::permissions::PermissionRequest>) {
    let asker = ScriptedAsker::new(answer.into_iter().collect());
    let mut fixture = fixture_with(
        sandbox_available(),
        Policy::for_mode(Mode::Ask).with_outside_read(outside_read),
        Some(Arc::new(asker.clone())),
        |workspace| {
            let outside = workspace.parent().unwrap().join("secret.txt");
            std::fs::write(&outside, "peek").unwrap();
            vec![
                call(
                    "call-1",
                    "read_file",
                    serde_json::json!({ "file_path": outside.display().to_string() }),
                ),
                Reply::text("read it"),
            ]
        },
    )
    .await;

    fixture.harness.run_turn("read it").await.unwrap();
    let (ok, message) = fixture.results().remove(0);
    let requests = asker.requests();
    fixture.harness.shutdown().await;
    (ok, message, requests)
}

// --- 没有沙箱就没有这一档（spec §6）----------------------------------------

#[tokio::test]
async fn assembling_workspace_without_a_sandbox_is_a_startup_error() {
    // 两条路：显式关掉这一层，或者探测说 `bwrap` 不可用。
    let mut off = SandboxSettings::off();
    off.mode = SandboxMode::Off;
    let mut unavailable = SandboxSettings::off();
    unavailable.mode = SandboxMode::Bwrap;
    unavailable.availability = SandboxAvailability::Unavailable {
        reason: "PATH 上没有 `bwrap`".to_owned(),
    };

    for settings in [off, unavailable] {
        let error = assembly_error(settings).await;
        for word in ["沙箱", "\"bwrap\"", "ask", "auto"] {
            assert!(
                error.contains(word),
                "文案里要写出两条出路，缺了 `{word}`：{error}"
            );
        }
    }
}

/// 组装一次 `workspace` 档会话，期望它在沙箱这一关被拒。
async fn assembly_error(sandbox: SandboxSettings) -> String {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let session = dir.path().join("session");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&session).unwrap();
    let mut config = SessionConfig::new("fake-model");
    config.sandbox = sandbox;

    let error = match assemble(AssemblyParts {
        provider: Box::new(FakeProvider::new(vec![Reply::text("unused")])),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: session.join("log.jsonl"),
            session_id: SessionId::new("s-workspace-off"),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Workspace),
            asker: Some(Arc::new(ScriptedAsker::default())),
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    {
        Ok(_) => panic!("没有沙箱就没有这一档"),
        Err(error) => error,
    };

    error.to_string()
}
