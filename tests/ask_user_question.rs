//! `ask_user_question`：模型发起的问句与它的答案（spec §7）。
//!
//! 这里练三条接缝。工具本身 —— 模型讲的那份线上契约，
//! 以及用户从没看见过的那些报错。整个循环，
//! 工具调用进去、正好一条结果出来（spec §19）。
//! 还有 plain 控制台，它逐行回答一份问卷。
//!
//! TUI 的接管有它自己的文件（`ask_user_question_tui.rs`），因为那一半
//! 是通过画出来的帧来断言的。

mod support;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::config::SessionConfig;
use fs_agent::context::repo_map::RepoMapInput;
use fs_agent::context::skills::Skills;
use fs_agent::events::{read_events, EventPayload, SessionId, SpeakerId};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::questions::{Choice, UserAnswer, UserAnswers, UserQuestion, UserQuestions};
use fs_agent::render::{
    console, spawn_plain_console_with, ConsoleQuestions, LineReader, RenderSinks, Renderer,
};
use fs_agent::tools::{
    AskUserQuestionTool, BashLimits, Effect, PathLocks, Registry, SessionPaths, Tool, ToolContext,
    ToolError, ToolOutput, ASK_USER_QUESTION_TOOL, TASK_TOOL,
};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::Value;
use support::{AlwaysAllow, FakeProvider, Reply};

/// 一个用答案脚本化的问题端口，记下它被问了什么。
///
/// 端口是前端实现的东西；测试脚本化它的方式，与脚本化
/// provider 回复一样，于是「用户选了 serde」不用终端也能复现。
struct ScriptedQuestions {
    answers: Mutex<VecDeque<UserAnswers>>,
    asked: Mutex<Vec<Vec<UserQuestion>>>,
}

impl ScriptedQuestions {
    fn new(answers: Vec<UserAnswers>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<Vec<UserQuestion>> {
        self.asked
            .lock()
            .expect("脚本化问题端口已中毒")
            .clone()
    }
}

#[async_trait]
impl UserQuestions for ScriptedQuestions {
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String> {
        self.asked
            .lock()
            .expect("脚本化问题端口已中毒")
            .push(questions.to_vec());
        Ok(self
            .answers
            .lock()
            .expect("脚本化问题端口已中毒")
            .pop_front()
            .expect("ScriptedQuestions: 没有脚本答案剩下了"))
    }
}

/// 用一个形状上真实的上下文与一个可选的问题端口调用这个工具。
async fn call(args: Value, port: Option<&dyn UserQuestions>) -> Result<ToolOutput, ToolError> {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    let paths = SessionPaths::new(cwd);
    let skills = Skills::discover(cwd, None);
    let repo_map = RepoMapInput::default();
    let bash = BashLimits::default();
    let passed = args.clone();
    let ctx = ToolContext {
        read_paths: &paths,
        write_paths: &paths,
        outputs_dir: cwd,
        cwd,
        skills: &skills,
        repo_map: &repo_map,
        bash: &bash,
        executor: None,
        questions: port,
        tool_call_id: "call-1",
        args: &args,
    };
    AskUserQuestionTool.call(&ctx, passed).await
}

#[tokio::test]
async fn a_question_round_trips_as_the_answers_json() {
    let port = ScriptedQuestions::new(vec![UserAnswers {
        answers: vec![UserAnswer {
            id: "framework".to_owned(),
            selected: vec!["serde".to_owned()],
            custom: None,
        }],
    }]);
    let args = serde_json::json!({
        "questions": [{
            "id": "framework",
            "question": "Which JSON framework?",
            "header": "JSON",
            "options": [{"label": "serde", "description": "the standard"}]
        }]
    });

    let output = call(args, Some(&port)).await.expect("工具作答了");
    assert_eq!(
        output.text,
        r#"{"answers":[{"id":"framework","selected":["serde"]}]}"#
    );
    let asked = port.asked();
    assert_eq!(asked.len(), 1, "端口被问了一次");
    assert_eq!(asked[0].len(), 1);
    assert_eq!(asked[0][0].id, "framework");
    assert_eq!(asked[0][0].question, "Which JSON framework?");
    assert_eq!(asked[0][0].header.as_deref(), Some("JSON"));
    assert!(!asked[0][0].multi_select, "multi_select 默认是 false");
    assert_eq!(
        asked[0][0].options,
        vec![Choice {
            label: "serde".to_owned(),
            description: Some("the standard".to_owned()),
        }]
    );
}

#[tokio::test]
async fn custom_text_and_multi_select_round_trip() {
    // 模型必须能分辨的两种编码：跳过的一题
    // 不带自定义文本，而多选那个答案可以同时带上选中的项
    // 与自定义文本（spec §7）。
    let port = ScriptedQuestions::new(vec![UserAnswers {
        answers: vec![
            UserAnswer {
                id: "a".to_owned(),
                selected: Vec::new(),
                custom: None,
            },
            UserAnswer {
                id: "b".to_owned(),
                selected: vec!["x".to_owned(), "y".to_owned()],
                custom: Some("and z".to_owned()),
            },
        ],
    }]);
    let args = serde_json::json!({
        "questions": [
            {"id": "a", "question": "anything?"},
            {"id": "b", "question": "which?", "options": [{"label": "x"}, {"label": "y"}],
             "multi_select": true}
        ]
    });

    let output = call(args, Some(&port)).await.expect("工具作答了");
    assert_eq!(
        output.text,
        r#"{"answers":[{"id":"a","selected":[]},{"id":"b","selected":["x","y"],"custom":"and z"}]}"#
    );
    let asked = port.asked();
    assert!(asked[0][1].multi_select);
    assert!(
        asked[0][0].options.is_empty(),
        "没有选项意味着这一题是自由文本"
    );
}

#[test]
fn the_description_carries_the_three_encoding_conventions() {
    // 没有这些，模型就读不懂那个答案，所以它们是
    // 线上契约的一部分，不是散文（spec §7）。
    let description = AskUserQuestionTool.spec().description;

    // 1. 跳过，与根本没走到的那一题，是两种不同的答案。
    assert!(description.contains("skipped"), "{description}");
    assert!(description.contains("never reached"), "{description}");
    assert!(description.contains("selected: []"), "{description}");
    // 2. 单选的,自定义文本是覆盖；多选的自定义文本是补充。
    assert!(description.contains("overrides"), "{description}");
    assert!(description.contains("supplements"), "{description}");
    // 3. `(Recommended)` 标记只做显示：答案留下的是那个 label。
    assert!(description.contains("(Recommended)"), "{description}");
    assert!(description.contains("marker included"), "{description}");
}

#[test]
fn the_tool_is_read_only_and_only_the_main_session_may_ask() {
    // `effect` 归类的是工作区副作用，而发问不碰任何路径
    // （spec §7）—— 与 `task` 得到的是同一个判定。
    assert_eq!(
        AskUserQuestionTool.effect(&serde_json::json!({})),
        Effect::ReadOnly
    );
    assert!(
        !AskUserQuestionTool.delegable(),
        "执行者的工具表里必须没有任何发问的路子（spec §7）"
    );
}

#[tokio::test]
async fn an_empty_question_list_is_refused() {
    let port = ScriptedQuestions::new(Vec::new());
    let error = call(serde_json::json!({"questions": []}), Some(&port))
        .await
        .expect_err("一份空问卷被拒");
    assert!(
        error.to_string().contains("at least one question"),
        "{error}"
    );
    assert!(
        port.asked().is_empty(),
        "用户永远不会看到一份空问卷"
    );
}

#[tokio::test]
async fn a_question_without_an_id_is_refused() {
    let port = ScriptedQuestions::new(Vec::new());
    let args = serde_json::json!({"questions": [{"question": "which?"}]});
    let error = call(args, Some(&port))
        .await
        .expect_err("没有 id 的一题被拒");
    assert!(error.to_string().contains("non-empty `id`"), "{error}");
    assert!(port.asked().is_empty());
}

#[tokio::test]
async fn duplicate_question_ids_are_refused() {
    // 靠 id 把答案与问题配起来的是模型；两题共用一个
    // 会让答案有歧义（spec §7）。
    let port = ScriptedQuestions::new(Vec::new());
    let args = serde_json::json!({
        "questions": [
            {"id": "same", "question": "one?"},
            {"id": "same", "question": "two?"}
        ]
    });
    let error = call(args, Some(&port))
        .await
        .expect_err("重复的 id 被拒");
    assert!(
        error.to_string().contains("duplicate question id"),
        "{error}"
    );
    assert!(port.asked().is_empty());
}

#[tokio::test]
async fn without_a_question_port_the_call_fails_instead_of_hanging() {
    // 降级地板（spec §19）：没有挂端口的会话拿到的是一条
    // 模型读得懂的失败，而不是一个永远到不了的答案。
    let args = serde_json::json!({
        "questions": [{"id": "q", "question": "which?"}]
    });
    let error = call(args, None)
        .await
        .expect_err("没有端口是一个错误，不是挂住");
    assert!(error.to_string().contains("no question port"), "{error}");
}

// ---------------------------------------------------------------------------
// 工具表与组装起来的循环
// ---------------------------------------------------------------------------

/// 一张工具表对外声明的工具名，按 provider 会看到的顺序。
fn advertised(registry: &Registry) -> Vec<String> {
    registry.specs().into_iter().map(|spec| spec.name).collect()
}

#[test]
fn the_table_decides_whether_the_model_may_ask() {
    // headless 那张表没有作答者，所以它从不声明这个工具：
    // 一次只会失败的调用白费模型一个回合（spec §19）。
    assert!(
        advertised(&fs_agent::tools::builtin(true)).contains(&ASK_USER_QUESTION_TOOL.to_owned())
    );
    assert!(
        !advertised(&fs_agent::tools::builtin(false)).contains(&ASK_USER_QUESTION_TOOL.to_owned())
    );
}

#[test]
fn an_executors_table_has_no_way_to_ask() {
    // `delegable() == false` 与把 `task` 挡在执行者工具表外的
    // 是同一个机制（spec §7、§16）—— 不是第二条规矩。
    let executor = fs_agent::tools::builtin(true).for_executor();
    assert!(executor.get(ASK_USER_QUESTION_TOOL).is_none());
    assert!(executor.get(TASK_TOOL).is_none());
    assert!(
        fs_agent::tools::builtin(true)
            .get(ASK_USER_QUESTION_TOOL)
            .is_some(),
        "主会话的表里确实有它"
    );
}

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

/// 组装一个真会话，它的工具表与端口由 `can_ask`/`port` 决定。
async fn fixture(
    replies: Vec<Reply>,
    port: Option<Arc<ScriptedQuestions>>,
    can_ask: bool,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);
    let questions: Option<Arc<dyn UserQuestions>> = port.map(|port| port as Arc<dyn UserQuestions>);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(std::io::sink()),
            stderr_diagnostic: Box::new(std::io::sink()),
        }),
        scaffold: SessionScaffold {
            cwd,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-1"),
            tools: fs_agent::tools::builtin(can_ask),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(Arc::new(AlwaysAllow)),
            hook: None,
            home: None,
            questions,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        provider,
        log_path,
        _dir: dir,
    }
}

#[tokio::test]
async fn the_model_can_ask_and_the_answer_is_the_tools_one_result() {
    // 这个特性的全部意义：问句以一次工具调用离开，答案
    // 作为那次调用的唯一结果回来，而它就是那个 answers JSON
    // （spec §7、§19）。
    let port = Arc::new(ScriptedQuestions::new(vec![UserAnswers {
        answers: vec![UserAnswer {
            id: "q".to_owned(),
            selected: vec!["yes".to_owned()],
            custom: None,
        }],
    }]));
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-ask".to_owned(),
                    name: ASK_USER_QUESTION_TOOL.to_owned(),
                    arguments: r#"{"questions":[{"id":"q","question":"which?"}]}"#.to_owned(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("thanks"),
        ],
        Some(Arc::clone(&port)),
        true,
    )
    .await;

    // 端口是通过组装起来的会话够到的，而不是测试直接
    // 往工具里注入的。
    fixture.harness.run_turn("ask me").await.unwrap();

    let events = read_events(&fixture.log_path).unwrap();
    let results: Vec<&EventPayload> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted { tool_call_id, .. }
                if tool_call_id.as_str() == "call-ask" =>
            {
                Some(&event.payload)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        results.len(),
        1,
        "这次 ask 调用正好拿到一条结果，与每一次工具调用一样"
    );
    match results[0] {
        EventPayload::ToolCallCompleted { ok, output, .. } => {
            assert!(ok);
            assert_eq!(
                output.as_deref(),
                Some(r#"{"answers":[{"id":"q","selected":["yes"]}]}"#)
            );
        }
        other => panic!("期望 ToolCallCompleted，实际得到 {other:?}"),
    }
    let asked = port.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0][0].question, "which?");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_headless_session_does_not_advertise_the_tool_to_the_model() {
    // 一个只会失败的工具，买单的是模型，所以 headless 那张表
    // 干脆不带它（spec §19）。
    let mut fixture = fixture(vec![Reply::text("hello")], None, false).await;
    fixture.harness.run_turn("hi").await.unwrap();

    let requests = fixture.provider.requests();
    let names: Vec<String> = requests[0]
        .tools
        .iter()
        .map(|spec| spec.name.clone())
        .collect();
    assert!(
        !names.contains(&ASK_USER_QUESTION_TOOL.to_owned()),
        "headless 不声明任何 ask 工具：{names:?}"
    );

    fixture.harness.shutdown().await;
}

// ---------------------------------------------------------------------------
// plain 控制台
// ---------------------------------------------------------------------------

/// 一道题，带选项与多选旗标。
fn plain_question(id: &str, text: &str, options: &[&str], multi_select: bool) -> UserQuestion {
    UserQuestion {
        id: id.to_owned(),
        question: text.to_owned(),
        header: None,
        options: options
            .iter()
            .map(|label| Choice {
                label: (*label).to_owned(),
                description: None,
            })
            .collect(),
        multi_select,
    }
}

/// 一个照脚本读行的读行器，`None` 表示输入结束。
///
/// plain 控制台唯一的输入原语是可注入的，所以那个逐行的
/// 前端不用管道也能驱动。
fn scripted_reader(lines: Vec<Option<String>>) -> LineReader {
    let mut lines = VecDeque::from(lines);
    Box::new(move || {
        let line = lines.pop_front().unwrap_or(None);
        Box::pin(async move { line })
    })
}

#[tokio::test]
async fn the_plain_console_answers_a_questionnaire_line_by_line() {
    let (handle, port, _events) = console();
    let _console = spawn_plain_console_with(
        port,
        scripted_reader(vec![Some("1".to_owned()), Some("my-name".to_owned())]),
    );

    let questions = vec![
        plain_question("which", "Which one?", &["serde", "manual"], false),
        plain_question("name", "How should it be named?", &[], false),
    ];
    let answers = ConsoleQuestions::from_handle(&handle)
        .ask(&questions)
        .await
        .expect("plain 控制台作答了");
    assert_eq!(
        answers.answers,
        vec![
            UserAnswer {
                id: "which".to_owned(),
                selected: vec!["serde".to_owned()],
                custom: None,
            },
            UserAnswer {
                id: "name".to_owned(),
                selected: Vec::new(),
                custom: Some("my-name".to_owned()),
            },
        ]
    );
}

#[tokio::test]
async fn the_plain_console_reads_a_multi_select_and_a_skip() {
    let (handle, port, _events) = console();
    let _console = spawn_plain_console_with(
        port,
        // 多选那一题读两行（编号，以及那句可选的
        // 补充）；单选那一题读第三行。
        scripted_reader(vec![
            Some("1, 2".to_owned()),
            Some(String::new()),
            Some(String::new()),
        ]),
    );
    let questions = vec![
        plain_question("many", "Which?", &["a", "b", "c"], true),
        plain_question("none", "Which?", &["a"], false),
    ];
    let answers = ConsoleQuestions::from_handle(&handle)
        .ask(&questions)
        .await
        .expect("plain 控制台作答了");
    assert_eq!(
        answers.answers,
        vec![
            UserAnswer {
                id: "many".to_owned(),
                selected: vec!["a".to_owned(), "b".to_owned()],
                custom: None,
            },
            UserAnswer {
                id: "none".to_owned(),
                selected: Vec::new(),
                custom: None,
            },
        ]
    );
}

#[tokio::test]
async fn the_plain_console_lets_a_multi_select_answer_options_and_text_together() {
    // `selected` 与 `custom` 同时出现只在多选题上合法
    // （spec §7），而面向行的前端那第二条可选行，就是管道那边的
    // 用户表达这件事的方式。
    let (handle, port, _events) = console();
    let _console = spawn_plain_console_with(
        port,
        scripted_reader(vec![Some("1, 2".to_owned()), Some("but not c".to_owned())]),
    );
    let questions = vec![plain_question("many", "Which?", &["a", "b", "c"], true)];
    let answers = ConsoleQuestions::from_handle(&handle)
        .ask(&questions)
        .await
        .expect("plain 控制台作答了");
    assert_eq!(
        answers.answers,
        vec![UserAnswer {
            id: "many".to_owned(),
            selected: vec!["a".to_owned(), "b".to_owned()],
            custom: Some("but not c".to_owned()),
        }]
    );
}

#[tokio::test]
async fn the_plain_console_fails_at_end_of_input_instead_of_looping() {
    // 管道上的降级地板（spec §19）：输入结束不是一个答案，
    // 于是这次调用失败，而不是去等一个已经不在了的人。
    let (handle, port, _events) = console();
    let _console = spawn_plain_console_with(port, scripted_reader(vec![None]));
    let questions = vec![plain_question("q", "Which?", &["a"], false)];
    let error = ConsoleQuestions::from_handle(&handle)
        .ask(&questions)
        .await
        .expect_err("输入结束不是一个答案");
    assert!(error.contains("input ended"), "{error}");
}
