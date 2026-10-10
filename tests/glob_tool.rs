//! 内建的 `glob` 工具：文件枚举、忽略规则与搜索工具同一套、按路径稳定排序、上限
//! （`.scratch/tool-coverage/spec.md` §4；票 11）。
//!
//! 三条接缝：
//!
//! * **只读** —— `effect()` 恒为 `Effect::ReadOnly`，四档放行、不取工作区锁；
//! * **同一个世界** —— `.gitignore` 与隐藏文件的处理照 [`grep_tool`] 那一套，两条工具在
//!   同一个工作区上看见的必须是同一批文件；
//! * **形状** —— 一行一条相对路径、按路径排序、上限 500 与末尾那句收尾。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::SessionConfig;
use heng::events::{Event, EventPayload, SessionId, SpeakerId, read_events};
use heng::permissions::{Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{Effect, GLOB_TOOL, MAX_PATHS, builtin};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{CaptureBuf, FakeProvider, Reply};

fn glob_reply(id: &str, pattern: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: GLOB_TOOL.to_owned(),
            arguments: serde_json::json!({ "pattern": pattern }).to_string(),
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
    // 与搜索工具的 fixture 一样：工作区看起来像一个 git 仓库，`ignore` 的 `require_git`
    // 默认才肯应用 `.gitignore`。
    std::fs::create_dir(workspace.join(".git")).unwrap();
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
            session_id: SessionId::new("s-glob"),
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

// --- 声明 -----------------------------------------------------------------

#[test]
fn the_tool_is_read_only_and_the_pattern_is_the_only_field() {
    let registry = builtin(false);
    let tool = registry.get(GLOB_TOOL).expect("glob 是内建工具");
    assert_eq!(tool.effect(&serde_json::json!({})), Effect::ReadOnly);
    let spec = tool.spec();
    assert_eq!(
        spec.parameters.get("required").unwrap(),
        &serde_json::json!(["pattern"]),
        "只有 `pattern` 必填"
    );
    assert!(
        !spec.parameters["properties"]
            .as_object()
            .unwrap()
            .contains_key("path"),
        "不加 `path`：路径前缀就写在 pattern 里，「只扫工作区」那个支点原样保留"
    );
    assert!(
        tool.read_paths(&serde_json::json!({ "pattern": "*.rs" }))
            .is_empty(),
        "枚举到的文件不登记进读集合"
    );
    assert!(tool.delegable(), "执行者也要能列文件");
}

#[test]
fn the_declaration_says_which_question_each_tool_answers() {
    let description = builtin(false).get(GLOB_TOOL).unwrap().spec().description;
    for expected in ["符号地图", "grep", "有哪些文件"] {
        assert!(
            description.contains(expected),
            "描述里要写清三方分工，缺了 `{expected}`：{description}"
        );
    }
    assert!(
        description.contains("ls") && description.contains("find"),
        "劝阻要指名模型今天会走的那两条路：{description}"
    );
}

// --- 形状 -----------------------------------------------------------------

#[tokio::test]
async fn one_relative_path_per_line_in_path_order() {
    let mut fixture = fixture(
        vec![glob_reply("call-1", "src/render/*.rs"), Reply::text("done")],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/render/beta.rs", "");
    fixture.write("src/render/alpha.rs", "");
    fixture.write("src/render/sub/deep.rs", "");
    fixture.write("src/render/notes.md", "");
    fixture.write("src/tools/grep.rs", "");
    fixture.run_turn("看看那个目录").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert_eq!(
        output, "src/render/alpha.rs\nsrc/render/beta.rs\n",
        "一行一条相对路径、按路径排序，`*` 只吃一层：{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn double_star_crosses_directories_and_a_bare_star_does_not() {
    let mut fixture = fixture(
        vec![
            glob_reply("call-1", "src/**"),
            glob_reply("call-2", "src/*.rs"),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("src/top.rs", "");
    fixture.write("src/deep/inner.rs", "");
    fixture.run_turn("看看那棵子树").await;

    let events = fixture.events();
    let tree = completed_output(&events, "call-1").unwrap();
    assert_eq!(
        tree, "src/deep/inner.rs\nsrc/top.rs\n",
        "`**` 跨目录：{tree}"
    );
    let one_level = completed_output(&events, "call-2").unwrap();
    assert_eq!(
        one_level, "src/top.rs\n",
        "`*` 不跨 `/`，模式就是模型写下的那个意思：{one_level}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn no_match_is_a_readable_answer_rather_than_an_empty_string() {
    let mut fixture = fixture(
        vec![glob_reply("call-1", "docs/**/*.toml"), Reply::text("done")],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("README.md", "");
    fixture.run_turn("找找").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.contains("没有匹配") && output.contains("docs/**/*.toml"),
        "空结果与「工具坏了」必须分得开：{output}"
    );
    fixture.shutdown().await;
}

// --- 忽略规则（与搜索工具同一套）-------------------------------------------

#[tokio::test]
async fn gitignored_and_hidden_files_are_skipped_just_like_the_search_tool() {
    let mut fixture = fixture(
        vec![glob_reply("call-1", "**/*.txt"), Reply::text("done")],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write(".gitignore", "ignored.txt\n");
    fixture.write("ignored.txt", "");
    fixture.write(".hidden.txt", "");
    fixture.write("plain.txt", "");
    fixture.write("nested/deep.txt", "");
    fixture.run_turn("找找").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert_eq!(
        output, "nested/deep.txt\nplain.txt\n",
        "两个只读工具看见的必须是同一个世界：{output}"
    );
    fixture.shutdown().await;
}

// --- 上限 -----------------------------------------------------------------

#[tokio::test]
async fn more_paths_than_the_limit_are_counted_rather_than_dumped() {
    let mut fixture = fixture(
        vec![glob_reply("call-1", "many/*.txt"), Reply::text("done")],
        Mode::Auto,
        None,
    )
    .await;
    for index in 0..(MAX_PATHS + 20) {
        fixture.write(&format!("many/f{index:04}.txt"), "");
    }
    fixture.run_turn("列一列").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    let listed: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("many/"))
        .collect();
    assert_eq!(listed.len(), MAX_PATHS, "列到上限就停：{}", listed.len());
    assert!(
        output.contains(&format!("还有 20 条未列出")),
        "末尾如实写清省掉了多少，并给一句能照做的事：{output}"
    );
    fixture.shutdown().await;
}

// --- 参数面 ---------------------------------------------------------------

#[tokio::test]
async fn a_malformed_pattern_is_a_tool_error_rather_than_a_panic() {
    let mut fixture = fixture(
        vec![glob_reply("call-1", "src/["), Reply::text("done")],
        Mode::Auto,
        None,
    )
    .await;
    fixture.run_turn("列一列").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("不是合法的 glob"), "{error}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_empty_pattern_is_an_argument_error() {
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".to_owned(),
                    name: GLOB_TOOL.to_owned(),
                    arguments: serde_json::json!({ "pattern": "  " }).to_string(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.run_turn("列一列").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("pattern"), "{error}");
    fixture.shutdown().await;
}

// --- 门与效果 -------------------------------------------------------------

#[tokio::test]
async fn the_readonly_mode_lets_an_enumeration_through() {
    let mut fixture = fixture(
        vec![glob_reply("call-1", "*.txt"), Reply::text("done")],
        Mode::Readonly,
        None,
    )
    .await;
    fixture.write("plain.txt", "");
    fixture.run_turn("列一列").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("plain.txt"), "{output}");
    let asked = fixture
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionAsked { .. }))
        .count();
    assert_eq!(asked, 0, "只读工具不问人");
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_enumeration_does_not_count_as_reading_the_files() {
    let mut fixture = fixture(
        vec![
            glob_reply("call-1", "*.txt"),
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-2".to_owned(),
                    name: "edit_file".to_owned(),
                    arguments: serde_json::json!({
                        "file_path": "plain.txt",
                        "old_string": "原样",
                        "new_string": "改过",
                    })
                    .to_string(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
    )
    .await;
    fixture.write("plain.txt", "原样\n");
    fixture.run_turn("列一列再改").await;

    let error = completed_output(&fixture.events(), "call-2").unwrap_err();
    assert!(error.contains("改前先读"), "列出来不等于读过：{error}");
    fixture.shutdown().await;
}
