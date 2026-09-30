//! 端到端看权限模式：会话从哪一档开始、`Shift+Tab` 手势
//! 对它做了什么，以及它拒掉什么
//! （`.scratch/todo-and-modes/spec.md` §1）。
//!
//! 这里手势是一次库调用，因为按下它的那个键属于一个渲染器；
//! 其余一切都是其他端到端测试用的同一份契约：
//! 一个脚本化 provider、一个脚本化作答者，加上对 JSONL 流
//! 与工作区的断言。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, Decision, DecisionSource, Event, EventPayload, ParticipantId, SessionId,
    SpeakerId, StopReason,
};
use fs_agent::permissions::{Asker, Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    /// 在整个测试期间保持活着；当一个更早的 fixture 还拥有
    /// 这个会话目录时是 `None`。
    _dir: Option<tempfile::TempDir>,
}

/// 一份「沙箱可用」的会话配置：`workspace` 档的存在与否只问这一件事（spec §6）。
fn sandbox_available() -> fs_agent::config::SandboxSettings {
    use fs_agent::config::{SandboxAvailability, SandboxMode, SandboxSettings};

    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Available {
        bwrap: PathBuf::from("/bin/true"),
    };
    settings
}

async fn fixture(replies: Vec<Reply>, mode: Mode, asker: Option<Arc<dyn Asker>>) -> Fixture {
    fixture_at(replies, mode, asker, None).await
}

/// 搭出一个会话，可选地续上一份已有的日志，这样测试就能断言
/// 一次 `--continue` 是从什么开始的。
async fn fixture_at(
    replies: Vec<Reply>,
    mode: Mode,
    asker: Option<Arc<dyn Asker>>,
    existing_log: Option<&std::path::Path>,
) -> Fixture {
    let dir = match existing_log {
        Some(_) => None,
        None => Some(tempfile::tempdir().unwrap()),
    };
    let (session, workspace) = match existing_log {
        Some(path) => (
            path.parent().unwrap().to_path_buf(),
            path.parent().unwrap().join("workspace"),
        ),
        None => {
            let root = dir.as_ref().unwrap().path();
            (root.join("session"), root.join("workspace"))
        }
    };
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);
    // 内置工具表，`bash` 也在里面：`readonly` 必须拒掉的正是
    // 一次 `Exclusive` 调用，用真工具钉住这一点最诚实。
    let tools = fs_agent::tools::builtin(false);

    // `workspace` 档要求一层可用的沙箱（`.scratch/workspace-mode/spec.md` §6），而这份
    // fixture 要能组装四档，所以这里给一份「可用」的状态；它不会被真跑 —— 这些测试里的
    // 调用都是文件工具。
    let mut config = SessionConfig::new("fake-model");
    config.sandbox = sandbox_available();

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mode"),
            tools,
            locks: fs_agent::tools::PathLocks::new(),
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
        harness,
        log_path,
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<(Decision, DecisionSource, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided {
                    decision,
                    source,
                    reason,
                    ..
                } => Some((*decision, *source, reason.clone())),
                _ => None,
            })
            .collect()
    }
}

fn tool_reply(id: &str, name: &str, arguments: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn write_reply(id: &str, file: &str) -> Reply {
    tool_reply(
        id,
        "write_file",
        &serde_json::json!({ "file_path": file, "content": "written\n" }).to_string(),
    )
}

// --- 会话跑在哪一档模式上 -------------------------------------------------

#[tokio::test]
async fn a_session_starts_in_the_mode_it_was_configured_with() {
    for mode in [Mode::Readonly, Mode::Ask, Mode::Workspace, Mode::Auto] {
        let fixture = fixture(vec![], mode, Some(Arc::new(AlwaysAllow))).await;
        assert_eq!(fixture.harness.mode(), mode);
        fixture.harness.shutdown().await;
    }
}

#[tokio::test]
async fn cycling_moves_the_policy_and_writes_nothing_to_the_stream() {
    // 这里钉住的那个选择：模式是一个会话值，所以这个手势不追加
    // 事件、也不注入任何指令 —— 在 `messages` 头上加一行会让
    // 每按一次就把前缀缓存扔掉（ADR 0003）。审计改为
    // 去看 `PermissionDecided.reason`。
    let fixture = fixture(vec![], Mode::Readonly, Some(Arc::new(AlwaysAllow))).await;
    let before = fixture.events().len();

    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Ask);
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Workspace);
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Auto);
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Readonly);
    assert_eq!(
        fixture.harness.mode(),
        Mode::Readonly,
        "按四次让会话回到它开始的地方"
    );
    assert_eq!(fixture.events().len(), before, "手势没有碰过历史");
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_continue_returns_to_the_configured_mode() {
    // 模式熬不过一次续接，因为它不在流里：重新打开的会话
    // 跑在配置里的那一档上（spec §12）。
    let fixture = fixture(vec![], Mode::Readonly, Some(Arc::new(AlwaysAllow))).await;
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Ask);
    let log_path = fixture.log_path.clone();
    fixture.harness.shutdown().await;

    let resumed = fixture_at(
        vec![write_reply("call-notes", "notes.txt"), Reply::text("ok")],
        Mode::Auto,
        Some(Arc::new(AlwaysAllow)),
        Some(&log_path),
    )
    .await;
    assert_eq!(resumed.harness.mode(), Mode::Auto);

    // 而且**权限模式**不是流上任何地方的字段。沙箱那一档（`SandboxStatus`）是另一件
    // 事 —— 它正好也有一个叫 `mode` 的字段，所以这里把它排除掉，其余事件照旧严格。
    for event in resumed.events() {
        if matches!(event.payload, EventPayload::SandboxStatus { .. }) {
            continue;
        }
        let value = serde_json::to_value(&event).unwrap();
        assert!(!has_key_named_mode(&value), "没有任何事件带着模式：{value}");
    }
    resumed.harness.shutdown().await;
}

// --- 模式拒掉什么 ---------------------------------------------------------

#[tokio::test]
async fn readonly_refuses_a_write_and_cycling_to_ask_lets_the_same_call_through() {
    let mut fixture = fixture(
        vec![
            write_reply("call-notes", "notes.txt"),
            Reply::text("blocked"),
            write_reply("call-again", "notes.txt"),
            Reply::text("wrote it"),
        ],
        Mode::Readonly,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();
    assert!(!fixture.exists("notes.txt"), "readonly 拒掉这次写");
    let refused = &fixture.decisions()[0];
    assert_eq!(refused.0, Decision::Deny);
    assert_eq!(refused.1, DecisionSource::Policy);
    assert!(
        refused.2.as_deref().unwrap().contains("readonly"),
        "审计说得出是哪一档拒的：{:?}",
        refused.2
    );

    // 按一次就把 `readonly` 挪到 `ask`，而作答者放行：同一次调用
    // 现在过去了。中间什么都没注入 —— 权限门读到了新的
    // 立场，因为模式是它每次调用都读的一个值。
    assert_eq!(fixture.harness.mode_cycle().cycle(), Mode::Ask);
    fixture.harness.run_turn("write it again").await.unwrap();
    assert_eq!(fixture.read("notes.txt"), "written\n");
    assert_eq!(
        fixture.decisions()[1].0,
        Decision::Allow,
        "用户的放行让它在 ask 档下过掉"
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_readonly_session_denies_a_shell_call_too() {
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-bash",
                "bash",
                &serde_json::json!({ "command": "echo hi > notes.txt" }).to_string(),
            ),
            Reply::text("I could not run it"),
        ],
        Mode::Readonly,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture.harness.run_turn("run it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert!(
        !fixture.exists("notes.txt"),
        "被拒的 shell 从没跑过，所以它什么都没写"
    );
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_executor_inherits_the_session_mode() {
    // 派发者在 `readonly` 档，而且一切都放行，所以能拒掉执行者这次写的
    // 只剩继承来的模式：派发本身是 `ReadOnly`，
    // 拒绝只能出自子会话自己的策略。
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-task",
                "task",
                &serde_json::json!({"brief": "write notes.txt"}).to_string(),
            ),
            write_reply("exec-1", "notes.txt"),
            Reply::text("I could not write it"),
            Reply::text("the executor reported back"),
        ],
        Mode::Readonly,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    let outcome = fixture.harness.run_turn("delegate it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert!(!fixture.exists("notes.txt"), "执行者继承了这一档模式");

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 2, "先是派发，然后是执行者那次写");
    assert_eq!(decisions[0].0, Decision::Allow, "派发是一次读");
    assert_eq!(decisions[1].0, Decision::Deny);
    assert!(
        decisions[1].2.as_deref().unwrap().contains("readonly"),
        "子会话这次拒绝出自会话模式：{:?}",
        decisions[1].2
    );

    // 这次拒绝记在执行者头上，而不是派发它的那个
    // 讨论者头上。
    let speakers: Vec<SpeakerId> = fixture
        .events()
        .into_iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionDecided { .. }))
        .map(|event| event.speaker_id)
        .collect();
    assert_eq!(
        speakers,
        vec![
            SpeakerId::Debater("kimi".into()),
            SpeakerId::Executor(ParticipantId::new("kimi-1")),
        ]
    );
    fixture.harness.shutdown().await;
}

/// 这份 JSON 里任何地方有没有哪个对象带一个叫 `mode` 的键。
fn has_key_named_mode(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.contains_key("mode") || map.values().any(has_key_named_mode)
        }
        serde_json::Value::Array(items) => items.iter().any(has_key_named_mode),
        _ => false,
    }
}
