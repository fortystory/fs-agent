//! 内建的 `git` 工具：一条入口、`op` 分档、`args` 原样透传
//! （`.scratch/tool-coverage/spec.md` §3；票 10）。
//!
//! 三条接缝：
//!
//! * **分档** —— 读 op 是 `Effect::ReadOnly`（四档放行、不取工作区锁）、写 op 是
//!   `Effect::Exclusive`（写的是 `.git/index` 之类，枚举不出「恰好这些路径」）；
//! * **参数面** —— 两个字段，`op` 必填且是枚举，`args` 原样进 argv；未知 op 是一条**参数
//!   错误**，而且在权限门之前就判出来（用户不会为一次注定失败的调用点一次头）；
//! * **真仓库** —— 结果是 git 自己的输出，模型看见的就是它拼 shell 时会看见的那份。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::{SandboxAvailability, SandboxMode, SandboxSettings, SessionConfig};
use heng::events::{Event, EventPayload, SessionId, SpeakerId, read_events};
use heng::permissions::{Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::sandbox::{Sandbox, probe};
use heng::tools::{Effect, GIT_TOOL, builtin, process};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply, ScriptedAsker};

/// 一次工具调用的脚本化回复。
fn git_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: GIT_TOOL.to_owned(),
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

/// 在工作区里跑一条真的 git 命令（建仓库与铺初始内容，不经模型）。
fn git(workspace: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        // 这一层只在测试里用，所以身份直接写在这里：让提交有一个可预测的作者。`-c` 必须
        // 排在子命令**之前**，git 才把它当全局配置。
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("能跑起 git");
    assert!(
        status.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// 给仓库一个可预测的提交身份 —— 测试里不写进全局配置。
fn identity(workspace: &std::path::Path) {
    git(workspace, &["config", "user.name", "test"]);
    git(workspace, &["config", "user.email", "test@example.com"]);
}

struct Fixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    workspace: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, asker: Option<Arc<dyn Asker>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    // 一个真仓库：这条工具输出的是 git 自己的东西，替身没有意义。
    git(&workspace, &["init", "--quiet"]);
    identity(&workspace);
    std::fs::write(workspace.join("tracked.txt"), "一行\n").unwrap();
    git(&workspace, &["add", "tracked.txt"]);
    git(&workspace, &["commit", "--quiet", "-m", "第一笔"]);
    let log_path = session.join("log.jsonl");

    let harness = assemble(AssemblyParts {
        provider: Box::new(FakeProvider::new(replies)),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-git"),
            tools: builtin(false),
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
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.workspace.join(name), content).unwrap();
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
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

/// 一次调用之后，有没有人**问过**用户。
fn asked(fixture: &Fixture) -> usize {
    fixture
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionAsked { .. }))
        .count()
}

// --- 声明与分档 -----------------------------------------------------------

#[test]
fn read_ops_are_read_only_and_write_ops_take_the_workspace() {
    let registry = builtin(false);
    let tool = registry.get(GIT_TOOL).expect("git 是内建工具");
    for op in ["status", "diff", "log", "show"] {
        assert_eq!(
            tool.effect(&serde_json::json!({ "op": op })),
            Effect::ReadOnly,
            "`{op}` 只读，所以四档都放行、不取工作区锁"
        );
    }
    for op in ["add", "commit", "stash"] {
        assert_eq!(
            tool.effect(&serde_json::json!({ "op": op })),
            Effect::Exclusive,
            "`{op}` 写的是 `.git/index` 之类，枚举不出恰好这些路径"
        );
    }
    assert!(
        tool.command(&serde_json::json!({ "op": "log", "args": ["--oneline"] }))
            .is_some(),
        "命令类工具要在进程起来之前就让门看见 argv"
    );
}

#[test]
fn the_declaration_names_two_fields_and_says_which_ops_are_which() {
    let spec = builtin(false).get(GIT_TOOL).unwrap().spec();
    assert_eq!(
        spec.parameters.get("required").unwrap(),
        &serde_json::json!(["op"]),
        "只有 `op` 必填"
    );
    let op = &spec.parameters["properties"]["op"];
    assert_eq!(op["type"], serde_json::json!("string"));
    let choices: Vec<&str> = op["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        choices,
        ["status", "diff", "log", "show", "add", "commit", "stash"],
        "op 的取值是枚举，一次定死"
    );
    assert_eq!(
        spec.parameters["properties"]["args"]["type"],
        serde_json::json!("array"),
        "`args` 原样透传，所以是字符串数组"
    );
    assert!(
        !spec.parameters["properties"]
            .as_object()
            .unwrap()
            .contains_key("workdir"),
        "不给 `workdir`：站位固定是会话 cwd，换目录用 `args` 透传 `-C`"
    );
    for op in choices {
        assert!(
            spec.description.contains(op),
            "描述里要逐个点名 `{op}`，否则模型只能猜：{description}",
            description = spec.description
        );
    }
}

// --- 读侧 -----------------------------------------------------------------

#[tokio::test]
async fn a_read_op_comes_back_as_git_printed_it() {
    let mut fixture = fixture(
        vec![
            git_reply("call-1", serde_json::json!({ "op": "status" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("untracked.txt", "新东西\n");
    fixture.run_turn("看看仓库").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.contains("untracked.txt"),
        "输出原样，没有加工：{output}"
    );
    assert!(
        !output.starts_with("cwd:"),
        "git 不加 `cwd:` 首行 —— 站位固定，没有歧义：{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn args_reach_git_verbatim() {
    let mut fixture = fixture(
        vec![
            git_reply(
                "call-1",
                serde_json::json!({ "op": "log", "args": ["--oneline", "-1"] }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.run_turn("看看历史").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.contains("第一笔"),
        "git 自己的旗标原样透传，模型不必学第二套参数名：{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_read_op_passes_the_readonly_mode() {
    let mut fixture = fixture(
        vec![
            git_reply("call-1", serde_json::json!({ "op": "diff" })),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
    )
    .await;
    fixture.write("tracked.txt", "改了\n");
    fixture.run_turn("看看改动").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.contains("tracked.txt"),
        "只读档下看改动照常放行：{output}"
    );
    assert_eq!(asked(&fixture), 0, "只读 op 不问人");
    fixture.shutdown().await;
}

// --- 写侧 -----------------------------------------------------------------

#[tokio::test]
async fn a_write_op_is_refused_by_the_readonly_mode() {
    let mut fixture = fixture(
        vec![
            git_reply(
                "call-1",
                serde_json::json!({ "op": "add", "args": ["tracked.txt"] }),
            ),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
    )
    .await;
    fixture.run_turn("暂存一下").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(!error.is_empty(), "写 op 在只读档下被拒：{error}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_write_op_stages_and_commits_through_the_same_tool() {
    let mut fixture = fixture(
        vec![
            git_reply(
                "call-1",
                serde_json::json!({ "op": "add", "args": ["tracked.txt"] }),
            ),
            git_reply(
                "call-2",
                serde_json::json!({ "op": "commit", "args": ["-m", "第二笔"] }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        Some(Arc::new(AlwaysAllow)),
    )
    .await;
    fixture.write("tracked.txt", "改了\n");
    fixture.run_turn("提交一下").await;

    let events = fixture.events();
    completed_output(&events, "call-1").unwrap();
    completed_output(&events, "call-2").unwrap();

    let log = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&fixture.workspace)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    assert!(log.contains("第二笔"), "同一条工具里做完了提交：{log}");
    fixture.shutdown().await;
}

// --- 参数面 ---------------------------------------------------------------

#[tokio::test]
async fn an_unknown_op_is_an_argument_error_that_never_asks_anybody() {
    let mut fixture = fixture(
        vec![
            git_reply("call-1", serde_json::json!({ "op": "rebase" })),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(ScriptedAsker::new(vec![]))),
    )
    .await;
    fixture.run_turn("改改历史").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("rebase"), "{error}");
    assert!(error.contains("op"), "文案要点名是 `op` 写错了：{error}");
    assert_eq!(
        asked(&fixture),
        0,
        "参数错误在权限门之前判出来，用户不会为它点一次头"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_missing_op_is_an_argument_error() {
    let mut fixture = fixture(
        vec![
            git_reply("call-1", serde_json::json!({})),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.run_turn("看看仓库").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("op"), "{error}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_non_string_arg_is_an_argument_error() {
    let mut fixture = fixture(
        vec![
            git_reply(
                "call-1",
                serde_json::json!({ "op": "log", "args": ["-n", 5] }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.run_turn("看看历史").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("args"), "{error}");
    fixture.shutdown().await;
}

// --- 与沙箱的关系 ---------------------------------------------------------

#[tokio::test]
async fn the_model_side_writes_go_through_the_sandbox() {
    // 模型侧必须过沙箱，而渲染层那条 git 明确不过（`docs/git.md` 里写死了这条分界线）。
    // 真机这一层在有 `bwrap` 的机器上跑：写 op 要能真的写 `.git/index`（沙箱不能把整个
    // `.git` 压只读），而 `.git/config` 那一处保护还得在。
    let probe_dir = tempfile::tempdir().unwrap();
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    let availability = probe(std::env::var_os("PATH").as_deref(), probe_dir.path());
    let SandboxAvailability::Available { bwrap } = availability else {
        eprintln!("跳过：这台机器上没有可用的 bwrap（{availability:?}）");
        return;
    };
    settings.availability = SandboxAvailability::Available { bwrap };

    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    git(&workspace, &["init", "--quiet"]);
    identity(&workspace);
    std::fs::write(workspace.join("tracked.txt"), "一行\n").unwrap();
    git(&workspace, &["add", "tracked.txt"]);
    git(&workspace, &["commit", "--quiet", "-m", "第一笔"]);

    let boundary = workspace.canonicalize().unwrap();
    // 闭包把 `boundary` 拿走了，宿主上读历史另留一份。
    let host = boundary.clone();
    let sandbox = Sandbox::new(&settings);
    let staged = vec!["git".to_owned(), "add".to_owned(), "tracked.txt".to_owned()];
    let commit = vec![
        "git".to_owned(),
        "commit".to_owned(),
        "-m".to_owned(),
        "沙箱里的第二笔".to_owned(),
    ];
    let run = |argv: Vec<String>| {
        let (boundary, sandbox) = (boundary.clone(), sandbox.clone());
        async move {
            process::run(
                &boundary,
                &boundary,
                &argv,
                std::time::Duration::from_secs(30),
                &sandbox,
            )
            .await
            .unwrap()
        }
    };
    std::fs::write(workspace.join("tracked.txt"), "改了\n").unwrap();
    let added = run(staged).await;
    let committed = run(commit).await;

    assert!(
        added.status.success(),
        "写 op 要能写 `.git/index` —— 把整个 `.git` 压只读会让提交全线失败：{}",
        added.report()
    );
    assert!(
        committed.status.success(),
        "沙箱下也要能提交：{}",
        committed.report()
    );
    let history = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&host)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&history.stdout);
    assert!(log.contains("沙箱里的第二笔"), "{log}");

    let hooked = run(vec![
        "git".to_owned(),
        "config".to_owned(),
        "core.hooksPath".to_owned(),
        "/tmp".to_owned(),
    ])
    .await;
    assert!(
        !hooked.status.success(),
        "`.git/config` 仍是只读的保护路径：{}",
        hooked.report()
    );
}
