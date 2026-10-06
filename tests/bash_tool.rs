//! 端到端看 `bash` 工具（票 20）：一条真命令的结果、非零
//! 退出码、经过 shell 包装层之后 `rm` 撞上的断路器，以及
//! 超时杀掉的那棵进程树。
//!
//! 用的接缝就是其他端到端测试用的那一条：`assemble` 配一个
//! 脚本化 provider，断言落在 JSONL 流与工作区上。这里
//! 没有开第二条接缝。

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use heng::config::{SessionConfig, DEFAULT_BASH_TIMEOUT_MS, MAX_BASH_TIMEOUT_MS};
use heng::context::TRUNCATED_MARKER;
use heng::events::{read_events, Decision, Event, EventPayload, SessionId, SpeakerId, StopReason};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{
    builtin, BashLimits, Effect, EXIT_CODE_PREFIX, STDERR_HEADER, STDOUT_HEADER, TIMEOUT_PREFIX,
};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    outputs: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, config: SessionConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);

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
            session_id: SessionId::new("s-bash"),
            tools: builtin(false),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(mode),
            // 一个什么都放行的作答者，于是这些测试里能拒掉一次调用的
            // 只剩断路器或者某一档模式。
            asker: Some(Arc::new(AlwaysAllow)),
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
        outputs: session.join("outputs"),
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

    /// 每一次已完成的调用对应的 `(tool_call_id, ok, output_or_error)`。
    fn results(&self) -> Vec<(String, bool, String)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    ok,
                    output,
                    error,
                    ..
                } => Some((
                    tool_call_id.as_str().to_owned(),
                    *ok,
                    output.clone().or_else(|| error.clone()).unwrap_or_default(),
                )),
                _ => None,
            })
            .collect()
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
}

/// 一次脚本化的 `bash` 调用。
fn bash_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: "bash".into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn run(id: &str, command: &str) -> Reply {
    bash_reply(id, serde_json::json!({ "command": command }))
}

fn run_with_timeout(id: &str, command: &str, timeout_ms: u64) -> Reply {
    bash_reply(
        id,
        serde_json::json!({ "command": command, "timeout_ms": timeout_ms }),
    )
}

// --- 声明的形状（不需要进程） ---------------------------------------------

#[test]
fn bash_declares_one_shell_argv_and_an_exclusive_effect() {
    let registry = builtin(false);
    let bash = registry.get("bash").expect("bash 是内置工具");

    assert_eq!(
        bash.effect(&serde_json::json!({ "command": "echo hi" })),
        Effect::Exclusive,
        "一条 shell 什么都能写，所以要拿住工作区锁"
    );
    assert_eq!(
        bash.command(&serde_json::json!({ "command": "echo hi" })),
        Some(vec![
            "bash".to_owned(),
            "-lc".to_owned(),
            "echo hi".to_owned()
        ]),
        "命令只占一个 argv 元素：模型插不进第二条 shell"
    );
    assert_eq!(
        bash.command(&serde_json::json!({})),
        None,
        "没有 command 的调用不声明任何 argv"
    );
    assert_eq!(
        bash.command(&serde_json::json!({ "command": "   " })),
        None,
        "空白的 command 不声明任何 argv"
    );
}

#[test]
fn the_configured_limits_are_the_default_and_a_hard_ceiling() {
    let config = SessionConfig::new("fake-model");
    assert_eq!(config.bash_timeout_ms, DEFAULT_BASH_TIMEOUT_MS);
    assert_eq!(config.max_bash_timeout_ms, MAX_BASH_TIMEOUT_MS);

    let limits = BashLimits::default();
    assert_eq!(
        limits.timeout(None),
        Duration::from_millis(DEFAULT_BASH_TIMEOUT_MS)
    );
    assert_eq!(limits.timeout(Some(25)), Duration::from_millis(25));
    assert_eq!(
        limits.timeout(Some(u64::MAX)),
        Duration::from_millis(MAX_BASH_TIMEOUT_MS),
        "模型可以要少一点，绝不可能要更多"
    );
}

// --- 一条真命令 -----------------------------------------------------------

#[tokio::test]
async fn a_real_command_reports_its_stdout_and_exit_code() {
    let mut fixture = fixture(
        vec![
            run("call-bash", "printf 'made\\n' > made.txt; echo hello"),
            Reply::text("done"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("run it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "命令成功了：{output}");
    assert!(
        output.starts_with(&format!("{EXIT_CODE_PREFIX}0\n")),
        "状态走在结果最前面：{output:?}"
    );
    assert!(output.contains(STDOUT_HEADER), "{output:?}");
    assert!(output.contains(STDERR_HEADER), "{output:?}");
    assert!(output.contains("hello"), "{output:?}");

    // 工作区里的副作用才是 `bash` 的意义所在，而不只是它的文本。
    assert_eq!(fixture.read("made.txt"), "made\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_non_zero_exit_is_a_result_the_model_can_read() {
    let mut fixture = fixture(
        vec![
            run("call-bash", "echo oops >&2; exit 3"),
            Reply::text("saw it"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("fail it").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "失败的命令是数据，不是 ToolError：{output:?}");
    assert!(
        output.contains(&format!("{EXIT_CODE_PREFIX}3")),
        "{output:?}"
    );
    assert!(output.contains("oops"), "stderr 在结果里：{output:?}");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_zero_timeout_is_refused_as_an_argument_error() {
    let mut fixture = fixture(
        vec![
            run_with_timeout("call-bash", "echo never", 0),
            Reply::text("ok"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("try it").await.unwrap();

    let (_, ok, message) = fixture.results().remove(0);
    assert!(!ok);
    assert!(message.contains("必须是正的毫秒数"), "{message}");
    assert!(!fixture.exists("made.txt"), "参数被拒，什么都还没有跑");

    fixture.harness.shutdown().await;
}

// --- 断路器 ---------------------------------------------------------------

#[tokio::test]
async fn rm_rf_root_is_refused_by_the_circuit_breaker() {
    // `auto` 加上一个总是放行的作答者：这里只有断路器能拒它。
    let mut fixture = fixture(
        vec![run("call-bash", "rm -rf /"), Reply::text("it refused")],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("wipe it").await.unwrap();

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Deny);
    assert!(
        decisions[0].1.as_deref().unwrap().contains("断路器"),
        "审计点了断路器的名：{:?}",
        decisions[0].1
    );

    let (_, ok, message) = fixture.results().remove(0);
    assert!(!ok, "这条 shell 从没跑过");
    assert!(message.contains("断路器"), "{message}");
}

// --- 超时与进程树 ---------------------------------------------------------

/// 一个 pid 有没有结束：它的 `/proc` 条目没了，或者它是个僵尸
/// （被杀掉了，但还没被继承它的谁收走 —— 仍然不算在跑）。
fn process_is_gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit_once(')')
            .map(|(_, rest)| rest.trim_start().starts_with(['Z', 'X']))
            .unwrap_or(false),
    }
}

async fn wait_until_gone(pid: u32) {
    for _ in 0..200 {
        if process_is_gone(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("进程 {pid} 在它的进程组被杀之后还在跑");
}

#[tokio::test]
async fn a_timed_out_command_is_killed_with_its_process_tree() {
    // 默认来自配置，不是脚本写死的值：模型没有发 `timeout_ms`，
    // 唱主角的是会话配的那个上限。
    let mut fixture = fixture(
        vec![
            run("call-bash", "sleep 30 & echo $! > child.pid; wait"),
            Reply::text("timed out"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model").with_bash_timeout_ms(300),
    )
    .await;

    let outcome = fixture.harness.run_turn("sleep").await.unwrap();
    assert_eq!(
        outcome.reason,
        StopReason::Completed,
        "超时是一条结果，不是一个失败的回合"
    );

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "半截的结果也被报出来了：{output:?}");
    assert!(output.contains(TIMEOUT_PREFIX), "{output:?}");
    assert!(
        output.contains("被信号"),
        "shell 是被信号带走的，不是自己退出：{output:?}"
    );

    // 命令放到后台的那个孙子进程是真的没了：落在 shell 自己那个进程组上的
    // 一次 `killpg` 够到了它，而只杀 shell 是够不到的。
    let child = fixture.read("child.pid").trim().parse::<u32>().unwrap();
    wait_until_gone(child).await;

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_backgrounded_child_cannot_outlive_the_timeout() {
    // `bash -lc "sleep 30 & echo …"` 立刻就退出，但后台那个子进程
    // 继承了输出的管道。一个只盯着 shell 的超时会把整个回合
    // 挂住，一直挂到那个孩子寿终。
    let mut fixture = fixture(
        vec![
            run("call-bash", "sleep 30 & echo $! > child.pid"),
            Reply::text("timed out"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model").with_bash_timeout_ms(300),
    )
    .await;

    let started = std::time::Instant::now();
    fixture.harness.run_turn("background it").await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "这次调用在它自己的期限上返回了，而不是那个孩子的"
    );

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "结果被报出来了：{output:?}");
    assert!(output.contains(TIMEOUT_PREFIX), "{output:?}");

    let child = fixture.read("child.pid").trim().parse::<u32>().unwrap();
    wait_until_gone(child).await;

    fixture.harness.shutdown().await;
}

// --- 已有的裁剪流水线 -----------------------------------------------------

#[tokio::test]
async fn an_oversized_result_is_spilled_before_it_reaches_the_stream() {
    let mut fixture = fixture(
        vec![
            run("call-bash", "for i in {1..2000}; do printf b; done"),
            Reply::text("done"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model").with_max_tool_result_tokens(50),
    )
    .await;

    fixture.harness.run_turn("print a lot").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok);
    assert!(output.contains(TRUNCATED_MARKER), "{output}");
    let pointer = fixture.outputs.join("call-bash.txt");
    assert!(
        output.contains(&pointer.display().to_string()),
        "流上扛的是那个指针：{output}"
    );
    let spilled = std::fs::read_to_string(&pointer).unwrap();
    assert!(spilled.contains(&"b".repeat(100)), "整个正文都在磁盘上");

    fixture.harness.shutdown().await;
}
