//! 内建的 `grep` 工具：只读的门裁决、命中的形状、忽略规则
//! （`.scratch/grep-tool/spec.md` §1–§6；票 01）。
//!
//! 三条接缝：
//!
//! * **门与效果** —— `effect()` 恒为 `Effect::ReadOnly`，于是四档里它都放行、
//!   也不取工作区锁；反向锚是 `bash` 的一次纯搜索仍要审批；
//! * **命中与范围** —— 结果形如 `path:line:文本`、只扫会话 cwd、
//!   遵守 `.gitignore` 并跳过隐藏文件；
//! * **不登记读集合** —— 搜到的文件不算「已读」，随后的 `edit_file`
//!   仍要求先 `read_file`。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::SessionConfig;
use heng::events::{
    Decision, DecisionSource, Event, EventPayload, SessionId, SpeakerId, read_events,
};
use heng::permissions::{Answer, Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{Effect, GREP_TOOL, MAX_MATCHES, builtin};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{CaptureBuf, FakeProvider, Reply, ScriptedAsker};

/// 一次工具调用的脚本化回复。
fn tool_reply(id: &str, name: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn grep_reply(id: &str, args: serde_json::Value) -> Reply {
    tool_reply(id, GREP_TOOL, args)
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

struct Fixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    workspace: PathBuf,
    /// 在整个测试期间保持活着。
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, asker: Option<Arc<dyn Asker>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    // 工作区看起来像一个 git 仓库：`ignore` 默认要求路径在 git 仓库里才应用
    // `.gitignore`（`require_git`），而 `rg` 的默认与它一致 —— 这条工具如实复制那套默认。
    std::fs::create_dir(workspace.join(".git")).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-grep"),
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
        let path = self.workspace.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
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

// --- 命中与形状 -----------------------------------------------------------

#[tokio::test]
async fn a_search_lands_as_one_ordinary_result_shaped_like_path_line_text() {
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/thing.rs", "fn first() {}\nlet needle = 1;\n");
    fixture.run_turn("find the needle").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert_eq!(output, "src/thing.rs:2:let needle = 1;\n");
    assert!(
        !output.contains('\\'),
        "路径是相对的、不带 cwd 前缀：{output}"
    );

    // 一次调用一条 `ToolCallStarted`、恰好一条结果（既有不变量）。
    let started = events
        .iter()
        .filter(|event| matches!(&event.payload, EventPayload::ToolCallStarted { tool_call_id, .. } if tool_call_id.as_str() == "call-1"))
        .count();
    let completed = events
        .iter()
        .filter(|event| matches!(&event.payload, EventPayload::ToolCallCompleted { tool_call_id, .. } if tool_call_id.as_str() == "call-1"))
        .count();
    assert_eq!((started, completed), (1, 1));

    fixture.shutdown().await;
}

#[tokio::test]
async fn no_match_is_a_readable_answer_rather_than_an_empty_string() {
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "nothing_here" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/thing.rs", "fn first() {}\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.contains("没有匹配") && output.contains("nothing_here"),
        "空结果与「工具坏了」必须分得开：{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn gitignored_and_hidden_files_are_skipped_while_plain_ones_are_found() {
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write(".gitignore", "ignored.txt\n");
    fixture.write("ignored.txt", "needle in an ignored file\n");
    fixture.write(".hidden.txt", "needle in a hidden file\n");
    fixture.write("plain.txt", "needle in a plain file\n");
    fixture.write("nested/deep.txt", "needle deeper down\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.contains("plain.txt:1:needle in a plain file"),
        "{output}"
    );
    assert!(output.contains("nested/deep.txt"), "子目录照扫：{output}");
    assert!(!output.contains("ignored.txt"), "{output}");
    assert!(!output.contains(".hidden.txt"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_malformed_pattern_is_a_tool_error_rather_than_a_panic() {
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "a(" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.run_turn("look").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("不是合法的正则"), "{error}");
    fixture.shutdown().await;
}

// --- 与读集合的关系 -------------------------------------------------------

#[tokio::test]
async fn a_search_result_does_not_count_as_reading_the_file() {
    // 钉住 spec §3 的那条决定：`read_paths()` 是调用前的纯函数，声明不了运行时才知道的
    // 命中文件，所以搜到的文件不算「已读」—— 随后的 `edit_file` 仍要求先 `read_file`。
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            tool_reply(
                "call-2",
                "edit_file",
                serde_json::json!({
                    "file_path": "src/thing.rs",
                    "old_string": "let needle = 1;",
                    "new_string": "let needle = 2;",
                }),
            ),
            tool_reply(
                "call-3",
                "read_file",
                serde_json::json!({ "file_path": "src/thing.rs" }),
            ),
            tool_reply(
                "call-4",
                "edit_file",
                serde_json::json!({
                    "file_path": "src/thing.rs",
                    "old_string": "let needle = 1;",
                    "new_string": "let needle = 2;",
                }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/thing.rs", "fn first() {}\nlet needle = 1;\n");

    fixture.run_turn("search then edit").await;

    let events = fixture.events();
    let refused = completed_output(&events, "call-2").unwrap_err();
    assert!(refused.starts_with("改前先读："), "{refused}");
    completed_output(&events, "call-4").expect("读过之后这次编辑过了");
    assert_eq!(
        fixture.read("src/thing.rs"),
        "fn first() {}\nlet needle = 2;\n"
    );
    fixture.shutdown().await;
}

// --- 门与效果 -------------------------------------------------------------

#[test]
fn the_tool_is_read_only_and_declares_no_read_paths() {
    let registry = builtin(false);
    let tool = registry.get(GREP_TOOL).expect("grep 是内建工具");
    assert_eq!(tool.effect(&serde_json::json!({})), Effect::ReadOnly);
    assert!(
        tool.read_paths(&serde_json::json!({ "pattern": "x" }))
            .is_empty(),
        "命中的文件不登记进读集合"
    );
    assert!(
        tool.delegable(),
        "执行者也能搜（spec §6）：`task` 之外的工具都委派得下去"
    );
}

#[test]
fn the_declaration_steers_the_model_away_from_assembling_shell_searches() {
    let registry = builtin(false);
    let spec = registry.get(GREP_TOOL).unwrap().spec();
    let description = spec.description;
    assert!(description.contains("不要用 `bash` 拼"), "{description}");
    assert!(description.contains("path:line:文本"), "{description}");
    assert_eq!(
        spec.parameters.get("required").unwrap(),
        &serde_json::json!(["pattern"]),
        "只有 `pattern` 必填"
    );
    assert!(
        spec.parameters["properties"].get("glob").is_some(),
        "`glob` 是可选的第二个参数"
    );
}

#[tokio::test]
async fn readonly_allows_a_search_and_ask_does_not_interrupt_it() {
    // 这条工具存在的第一理由：同一个搜索在 `readonly` 档放行、在 `ask` 档不问人。
    let mut readonly = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
    )
    .await;
    readonly.write("plain.txt", "needle\n");
    readonly.run_turn("look").await;
    let decisions = readonly.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Allow);
    assert_eq!(decisions[0].1, DecisionSource::Policy);
    assert!(
        decisions[0].2.as_deref().unwrap().contains("readonly"),
        "审计说得出是哪一档放行的：{:?}",
        decisions[0].2
    );
    readonly.shutdown().await;

    let asker = ScriptedAsker::new(Vec::new());
    let mut ask = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
    )
    .await;
    ask.write("plain.txt", "needle\n");
    ask.run_turn("look").await;
    assert!(
        asker.requests().is_empty(),
        "`ask` 档下一次只读搜索不该打断人：{:?}",
        asker.requests()
    );
    assert_eq!(ask.decisions()[0].0, Decision::Allow);
    ask.shutdown().await;
}

#[tokio::test]
async fn a_shell_search_still_asks_in_ask_mode() {
    // 反向锚：这条工具修的是「纯读取的搜索在 `ask` 档要审批」。同一件事用 `bash` 拼出来，
    // 仍然过审批 —— `bash` 是 `Exclusive`，一个 shell 什么都能写。
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-1",
                "bash",
                serde_json::json!({ "command": "rg needle ." }),
            ),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
    )
    .await;
    fixture.write("plain.txt", "needle\n");
    fixture.run_turn("look").await;

    assert_eq!(asker.requests().len(), 1, "拼 shell 仍然要人点头");
    fixture.shutdown().await;
}

#[test]
fn executors_get_the_tool_too() {
    let executor = builtin(false).for_executor();
    assert!(executor.get(GREP_TOOL).is_some());
}

/// `grep` 的结果确实由那条唯一的截断流水线管（票 03 在工具内另收一刀之前，
/// 这里先钉住「溢出走落盘」这条既有行为）。
#[tokio::test]
async fn a_search_too_large_for_the_result_budget_spills_to_a_file() {
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    // 一行就够长：几百行就能顶破 25k token 的缺省上限。
    let long = "needle ".repeat(400);
    for index in 0..400 {
        fixture.write(&format!("src/file_{index:03}.rs"), &format!("{long}\n"));
    }
    fixture.run_turn("look").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(
        output.contains("[已截断："),
        "{}",
        &output[..200.min(output.len())]
    );
    let spilled = fixture
        .harness
        .as_ref()
        .expect("harness 还活着")
        .outputs_dir()
        .join("call-1.txt");
    assert!(spilled.exists(), "全文落在 {}", spilled.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&spilled).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "溢出文件是 owner-only");
    }
    fixture.shutdown().await;
}

// --- `glob` 过滤（票 02）---------------------------------------------------

#[tokio::test]
async fn a_glob_limits_the_search_to_the_files_it_matches() {
    let mut fixture = fixture(
        vec![
            grep_reply(
                "call-1",
                serde_json::json!({ "pattern": "needle", "glob": "*.rs" }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/thing.rs", "let needle = 1;\n");
    fixture.write("src/thing.txt", "let needle = 1;\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("src/thing.rs:1:"), "{output}");
    assert!(
        !output.contains("thing.txt"),
        "`glob` 只选文件，不改 pattern：{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn without_a_glob_every_file_is_searched_again() {
    // 回归锚：不带 `glob` 时默认行为一个字没变。
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/thing.rs", "let needle = 1;\n");
    fixture.write("src/thing.txt", "let needle = 1;\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("src/thing.rs:1:"), "{output}");
    assert!(output.contains("src/thing.txt:1:"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_glob_does_not_reopen_what_the_ignore_rules_close() {
    // `glob` 不是「无视 `.gitignore`」的逃生口：点名一个被忽略的文件也搜不到它。
    let mut fixture = fixture(
        vec![
            grep_reply(
                "call-1",
                serde_json::json!({ "pattern": "needle", "glob": "ignored.txt" }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write(".gitignore", "ignored.txt\n");
    fixture.write("ignored.txt", "needle\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(!output.contains("ignored.txt:1:"), "{output}");
    assert!(output.contains("没有匹配 glob"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_malformed_glob_is_a_tool_error_rather_than_a_panic() {
    let mut fixture = fixture(
        vec![
            grep_reply(
                "call-1",
                serde_json::json!({ "pattern": "needle", "glob": "a[" }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("plain.txt", "needle\n");
    fixture.run_turn("look").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("不是合法的 glob"), "{error}");
    fixture.shutdown().await;
}

// --- 命中太多时的收尾（票 03）---------------------------------------------

#[tokio::test]
async fn more_matches_than_the_limit_are_counted_rather_than_dumped() {
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    // 600 条短命中：顶破条数上限，但离 token 上限还很远 —— 两条界是分开的两件事。
    let extra = 100;
    fixture.write("big.txt", &"needle\n".repeat(MAX_MATCHES + extra));
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines.len(),
        MAX_MATCHES + 1,
        "列出的条数就是那个常量上限，外加一句收尾"
    );
    let last = lines.last().unwrap();
    assert!(last.contains(&format!("还有 {extra} 条未列出")), "{last}");
    assert!(last.contains("glob"), "收尾要给一句能照做的事：{last}");
    assert!(
        !output.contains("[已截断："),
        "走的是工具内那条收尾，不是 token 那条截断流水线"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_search_that_stays_under_both_limits_is_verbatim() {
    // 回归锚：两个上限都没碰到时，结果与票 01 的形状逐字相同 —— 没有收尾行、没有标记。
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/thing.rs", "fn first() {}\nlet needle = 1;\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert_eq!(output, "src/thing.rs:2:let needle = 1;\n");
    fixture.shutdown().await;
}
#[tokio::test]
async fn a_binary_file_is_dropped_rather_than_dumped() {
    // rg 的默认那一档：文件里见到 NUL 就放弃它，于是二进制不会被倒进上下文。
    let mut fixture = fixture(
        vec![
            grep_reply("call-1", serde_json::json!({ "pattern": "needle" })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("plain.txt", "needle\n");
    fixture.write("blob.bin", "\u{0}needle\n");
    fixture.run_turn("look").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("plain.txt:1:needle"), "{output}");
    assert!(!output.contains("blob.bin"), "{output}");
    fixture.shutdown().await;
}
