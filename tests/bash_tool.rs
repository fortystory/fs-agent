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

use heng::config::{DEFAULT_BASH_TIMEOUT_MS, MAX_BASH_TIMEOUT_MS, SessionConfig};
use heng::context::TRUNCATED_MARKER;
use heng::events::{Decision, Event, EventPayload, SessionId, SpeakerId, StopReason, read_events};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{
    BashLimits, EXIT_CODE_PREFIX, Effect, STDERR_HEADER, STDOUT_HEADER, TIMEOUT_PREFIX, builtin,
};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    outputs: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, config: SessionConfig) -> Fixture {
    fixture_in(tempfile::tempdir().unwrap(), replies, mode, config).await
}

/// 同上，但临时目录由调用方给：需要在**装配之前**就拿到工作区里一条绝对路径时用它。
async fn fixture_in(
    dir: tempfile::TempDir,
    replies: Vec<Reply>,
    mode: Mode,
    config: SessionConfig,
) -> Fixture {
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

// --- `workdir`：站位在工作区之内（`.scratch/bash-workdir` 票 01）----------

/// 一次带 `workdir` 的脚本化 `bash` 调用。
fn run_in(id: &str, command: &str, workdir: &str) -> Reply {
    bash_reply(
        id,
        serde_json::json!({ "command": command, "workdir": workdir }),
    )
}

/// 一次脚本化的 `read_file` 调用。
fn read(id: &str, path: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "file_path": path }).to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// 一条路径的物理形：`pwd -P` 打印出来的正是这一种。
fn physical(path: &std::path::Path) -> String {
    std::fs::canonicalize(path).unwrap().display().to_string()
}

/// 一次调用跑完之后，事件流里有没有人**问过**用户。
fn asked(fixture: &Fixture) -> usize {
    fixture
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionAsked { .. }))
        .count()
}

#[tokio::test]
async fn a_workdir_moves_the_command_into_a_subdirectory() {
    let mut fixture = fixture(
        vec![run_in("call-bash", "pwd -P", "sub"), Reply::text("done")],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;
    std::fs::create_dir_all(fixture.workspace.join("sub")).unwrap();

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(
        output.contains(&physical(&fixture.workspace.join("sub"))),
        "命令站在相对工作区解析出来的 `sub` 里：{output}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_absolute_workdir_inside_the_workspace_is_the_same_thing() {
    // 模型从 `pwd` 或报错信息里抄到绝对路径很常见，所以同一个站位换一种写法照样成立。
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("workspace/sub");
    std::fs::create_dir_all(&sub).unwrap();
    let absolute = physical(&sub);

    let mut fixture = fixture_in(
        dir,
        vec![
            run_in("call-bash", "pwd -P", &absolute),
            Reply::text("done"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(output.contains(&absolute), "{output}");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_workdir_outside_the_workspace_is_a_tool_error() {
    // 三种写法都算区外：相对的回退、区外的绝对路径、以及写了一大串 `..` 的。判定按**解析后的
    // 真实位置**做，所以 `sub/../../..` 不会因为字面上还带着工作区的前缀就蒙混过去。
    for workdir in ["../", "/", "sub/../../.."] {
        let mut fixture = fixture(
            vec![run_in("call-bash", "pwd", workdir), Reply::text("ack")],
            Mode::Auto,
            SessionConfig::new("fake-model"),
        )
        .await;
        std::fs::create_dir_all(fixture.workspace.join("sub")).unwrap();

        fixture.harness.run_turn("run it").await.unwrap();

        let (_, ok, message) = fixture.results().remove(0);
        assert!(!ok, "`{workdir}` 是区外，必须是工具错误：{message}");
        assert!(message.contains("workdir"), "{message}");
        assert_eq!(
            asked(&fixture),
            0,
            "`workdir` 不构成写，所以它自己的错误一条都不弹审批"
        );

        fixture.harness.shutdown().await;
    }
}

#[tokio::test]
async fn an_empty_workdir_is_an_argument_error() {
    let mut fixture = fixture(
        vec![
            bash_reply(
                "call-bash",
                serde_json::json!({ "command": "pwd > made.txt", "workdir": "" }),
            ),
            Reply::text("ack"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, message) = fixture.results().remove(0);
    assert!(
        !ok,
        "空串与 `timeout_ms` / `escalation` 的半截写法同一种形状：{message}"
    );
    assert!(message.contains("workdir"), "{message}");
    assert!(!fixture.exists("made.txt"), "参数被拒，命令没有跑");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_dot_workdir_is_the_workspace_itself() {
    let mut fixture = fixture(
        vec![run_in("call-bash", "pwd -P", "."), Reply::text("done")],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(
        output.contains(&physical(&fixture.workspace)),
        "`\".\"` 等价于不写：{output}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_missing_workdir_is_a_tool_error_and_is_never_created() {
    let mut fixture = fixture(
        vec![
            run_in("call-bash", "pwd > made.txt", "no-such-dir"),
            Reply::text("ack"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, message) = fixture.results().remove(0);
    assert!(!ok, "{message}");
    assert!(message.contains("workdir"), "{message}");
    assert!(
        !fixture.exists("made.txt"),
        "目录不存在时命令不跑，也不替它建任何东西"
    );
    assert!(
        !fixture.workspace.join("no-such-dir").exists(),
        "隐式写入不该藏在参数里：那是另一条命令的事"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_workdir_that_is_a_file_is_a_tool_error() {
    let mut fixture = fixture(
        vec![
            run_in("call-bash", "pwd > made.txt", "notes.txt"),
            Reply::text("ack"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;
    std::fs::write(fixture.workspace.join("notes.txt"), "不是目录\n").unwrap();

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, message) = fixture.results().remove(0);
    assert!(!ok, "{message}");
    assert!(message.contains("workdir"), "{message}");
    assert!(!fixture.exists("made.txt"), "命令没有跑");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_workdir_alongside_an_escalation_is_an_argument_error() {
    let mut fixture = fixture(
        vec![
            bash_reply(
                "call-bash",
                serde_json::json!({
                    "command": "echo hi",
                    "workdir": "sub",
                    "escalation": {
                        "justification": "想写缓存目录",
                        "writable_paths": ["/tmp/heng-workdir-probe"],
                    }
                }),
            ),
            Reply::text("ack"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;
    std::fs::create_dir_all(fixture.workspace.join("sub")).unwrap();

    fixture.harness.run_turn("run it").await.unwrap();

    let (_, ok, message) = fixture.results().remove(0);
    assert!(
        !ok,
        "一个是站位、一个是额外可写根，混起来等于让模型自选工作区：{message}"
    );
    assert!(message.contains("workdir"), "{message}");
    assert!(message.contains("escalation"), "{message}");
    assert_eq!(asked(&fixture), 0, "参数错误不弹审批");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_workdir_does_not_move_the_base_of_sibling_tools() {
    let mut fixture = fixture(
        vec![
            run_in("call-bash", "pwd -P", "sub"),
            read("call-read", "made.txt"),
            Reply::text("done"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;
    std::fs::create_dir_all(fixture.workspace.join("sub")).unwrap();
    std::fs::write(fixture.workspace.join("made.txt"), "工作区根上的那份\n").unwrap();

    fixture.harness.run_turn("run it").await.unwrap();

    let results = fixture.results();
    assert_eq!(results.len(), 2, "{results:?}");
    let (_, ok, output) = &results[1];
    assert!(ok, "`workdir` 只影响那一条命令的进程：{output}");
    assert!(
        output.contains("工作区根上的那份"),
        "兄弟工具的基准仍按工作区解析：{output}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn the_stream_records_the_workdir_as_the_model_wrote_it() {
    let mut fixture = fixture(
        vec![run_in("call-bash", "pwd", "sub"), Reply::text("done")],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;
    std::fs::create_dir_all(fixture.workspace.join("sub")).unwrap();

    fixture.harness.run_turn("run it").await.unwrap();

    let args = fixture
        .events()
        .into_iter()
        .find_map(|event| match event.payload {
            EventPayload::ToolCallStarted { args, .. } => Some(args),
            _ => None,
        })
        .expect("一条 ToolCallStarted 事件");
    assert_eq!(
        args.get("workdir").and_then(|value| value.as_str()),
        Some("sub"),
        "回放要能重算这次调用站在哪，所以记的是模型发的原文：{args}"
    );

    fixture.harness.shutdown().await;
}
