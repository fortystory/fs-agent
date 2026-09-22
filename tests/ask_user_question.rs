//! `ask_user_question`: the model-initiated question and its answer (spec §7).
//!
//! Three seams are exercised here. The tool itself — the wire contract the model
//! speaks, and the errors it gets back without a user ever seeing them. The whole
//! loop, where the tool call goes in and exactly one result comes out (spec §19).
//! And the plain console, which answers a questionnaire line by line.
//!
//! The TUI's takeover has its own file (`ask_user_question_tui.rs`), because that
//! half is asserted through rendered frames.

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

/// A question port scripted with answers, recording what it was asked.
///
/// The port is what a front end implements; a test scripts it the way it scripts
/// provider replies, so "the user picked serde" is reproducible without a terminal.
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
            .expect("scripted questions poisoned")
            .clone()
    }
}

#[async_trait]
impl UserQuestions for ScriptedQuestions {
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String> {
        self.asked
            .lock()
            .expect("scripted questions poisoned")
            .push(questions.to_vec());
        Ok(self
            .answers
            .lock()
            .expect("scripted questions poisoned")
            .pop_front()
            .expect("ScriptedQuestions: no scripted answer left"))
    }
}

/// Call the tool with a real-by-shape context and an optional question port.
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

    let output = call(args, Some(&port)).await.expect("the tool answers");
    assert_eq!(
        output.text,
        r#"{"answers":[{"id":"framework","selected":["serde"]}]}"#
    );
    let asked = port.asked();
    assert_eq!(asked.len(), 1, "the port was asked once");
    assert_eq!(asked[0].len(), 1);
    assert_eq!(asked[0][0].id, "framework");
    assert_eq!(asked[0][0].question, "Which JSON framework?");
    assert_eq!(asked[0][0].header.as_deref(), Some("JSON"));
    assert!(!asked[0][0].multi_select, "multi_select defaults to false");
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
    // The two encodings the model must be able to tell apart: a skipped question
    // carries no custom text, and a multi-select answer may carry both selections
    // and custom text (spec §7).
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

    let output = call(args, Some(&port)).await.expect("the tool answers");
    assert_eq!(
        output.text,
        r#"{"answers":[{"id":"a","selected":[]},{"id":"b","selected":["x","y"],"custom":"and z"}]}"#
    );
    let asked = port.asked();
    assert!(asked[0][1].multi_select);
    assert!(
        asked[0][0].options.is_empty(),
        "no options means the question is free text"
    );
}

#[test]
fn the_description_carries_the_three_encoding_conventions() {
    // The model cannot read the answer without these, so they are part of the
    // wire contract, not prose (spec §7).
    let description = AskUserQuestionTool.spec().description;

    // 1. A skip and a question that was never reached are different answers.
    assert!(description.contains("skipped"), "{description}");
    assert!(description.contains("never reached"), "{description}");
    assert!(description.contains("selected: []"), "{description}");
    // 2. Single-select custom text overrides; multi-select custom text supplements.
    assert!(description.contains("overrides"), "{description}");
    assert!(description.contains("supplements"), "{description}");
    // 3. The recommended marker is display only: the answer keeps the label.
    assert!(description.contains("(Recommended)"), "{description}");
    assert!(description.contains("marker included"), "{description}");
}

#[test]
fn the_tool_is_read_only_and_only_the_main_session_may_ask() {
    // `effect` classifies workspace side effects, and asking touches no path
    // (spec §7) — the same judgement `task` gets.
    assert_eq!(
        AskUserQuestionTool.effect(&serde_json::json!({})),
        Effect::ReadOnly
    );
    assert!(
        !AskUserQuestionTool.delegable(),
        "an executor's table must have no way to ask (spec §7)"
    );
}

#[tokio::test]
async fn an_empty_question_list_is_refused() {
    let port = ScriptedQuestions::new(Vec::new());
    let error = call(serde_json::json!({"questions": []}), Some(&port))
        .await
        .expect_err("an empty questionnaire is refused");
    assert!(
        error.to_string().contains("at least one question"),
        "{error}"
    );
    assert!(
        port.asked().is_empty(),
        "the user is never shown an empty questionnaire"
    );
}

#[tokio::test]
async fn a_question_without_an_id_is_refused() {
    let port = ScriptedQuestions::new(Vec::new());
    let args = serde_json::json!({"questions": [{"question": "which?"}]});
    let error = call(args, Some(&port))
        .await
        .expect_err("an id-less question is refused");
    assert!(error.to_string().contains("non-empty `id`"), "{error}");
    assert!(port.asked().is_empty());
}

#[tokio::test]
async fn duplicate_question_ids_are_refused() {
    // Ids are how the model pairs answers with questions; two questions sharing
    // one would make the answer ambiguous (spec §7).
    let port = ScriptedQuestions::new(Vec::new());
    let args = serde_json::json!({
        "questions": [
            {"id": "same", "question": "one?"},
            {"id": "same", "question": "two?"}
        ]
    });
    let error = call(args, Some(&port))
        .await
        .expect_err("duplicate ids are refused");
    assert!(
        error.to_string().contains("duplicate question id"),
        "{error}"
    );
    assert!(port.asked().is_empty());
}

#[tokio::test]
async fn without_a_question_port_the_call_fails_instead_of_hanging() {
    // The degradation floor (spec §19): a session that mounted no port gets a
    // model-readable failure rather than an answer that can never arrive.
    let args = serde_json::json!({
        "questions": [{"id": "q", "question": "which?"}]
    });
    let error = call(args, None)
        .await
        .expect_err("no port is an error, not a hang");
    assert!(error.to_string().contains("no question port"), "{error}");
}

// ---------------------------------------------------------------------------
// The table and the assembled loop
// ---------------------------------------------------------------------------

/// The tool names a table advertises, in the order the provider would see them.
fn advertised(registry: &Registry) -> Vec<String> {
    registry.specs().into_iter().map(|spec| spec.name).collect()
}

#[test]
fn the_table_decides_whether_the_model_may_ask() {
    // The headless table has no answerer, so it never advertises the tool: a call
    // that can only fail wastes a model turn (spec §19).
    assert!(
        advertised(&fs_agent::tools::builtin(true)).contains(&ASK_USER_QUESTION_TOOL.to_owned())
    );
    assert!(
        !advertised(&fs_agent::tools::builtin(false)).contains(&ASK_USER_QUESTION_TOOL.to_owned())
    );
}

#[test]
fn an_executors_table_has_no_way_to_ask() {
    // `delegable() == false` is the same mechanism that keeps `task` out of an
    // executor's table (spec §7, §16) — not a second rule.
    let executor = fs_agent::tools::builtin(true).for_executor();
    assert!(executor.get(ASK_USER_QUESTION_TOOL).is_none());
    assert!(executor.get(TASK_TOOL).is_none());
    assert!(
        fs_agent::tools::builtin(true)
            .get(ASK_USER_QUESTION_TOOL)
            .is_some(),
        "the main session's table does have it"
    );
}

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

/// Assemble a real session whose table and port are decided by `can_ask`/`port`.
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
    // The whole point of the feature: the question leaves as a tool call, the
    // answer comes back as that call's one result, and it is the answers JSON
    // (spec §7, §19).
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

    // The port is reached through the assembled session, not injected by the test
    // into the tool directly.
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
        "the ask call gets exactly one result, like every tool call"
    );
    match results[0] {
        EventPayload::ToolCallCompleted { ok, output, .. } => {
            assert!(ok);
            assert_eq!(
                output.as_deref(),
                Some(r#"{"answers":[{"id":"q","selected":["yes"]}]}"#)
            );
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
    let asked = port.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0][0].question, "which?");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_headless_session_does_not_advertise_the_tool_to_the_model() {
    // The model is what pays for a tool that can only fail, so the headless table
    // simply does not carry it (spec §19).
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
        "headless advertises no ask tool: {names:?}"
    );

    fixture.harness.shutdown().await;
}

// ---------------------------------------------------------------------------
// The plain console
// ---------------------------------------------------------------------------

/// One question, with options and the multi-select flag.
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

/// A line reader that answers from a script, `None` meaning end of input.
///
/// The plain console's only input primitive is injectable, so the line-by-line
/// front end can be driven without a pipe.
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
        .expect("the plain console answers");
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
        // A multi-select question reads two lines (the numbers and the optional
        // supplement); the single-select one reads the third.
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
        .expect("the plain console answers");
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
    // `selected` and `custom` together are legal only on a multi-select question
    // (spec §7), and the line-oriented front end's second optional line is how a
    // pipe user expresses that.
    let (handle, port, _events) = console();
    let _console = spawn_plain_console_with(
        port,
        scripted_reader(vec![Some("1, 2".to_owned()), Some("but not c".to_owned())]),
    );
    let questions = vec![plain_question("many", "Which?", &["a", "b", "c"], true)];
    let answers = ConsoleQuestions::from_handle(&handle)
        .ask(&questions)
        .await
        .expect("the plain console answers");
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
    // The degradation floor on a pipe (spec §19): end of input is not an answer,
    // and the call fails rather than waiting for a person who is gone.
    let (handle, port, _events) = console();
    let _console = spawn_plain_console_with(port, scripted_reader(vec![None]));
    let questions = vec![plain_question("q", "Which?", &["a"], false)];
    let error = ConsoleQuestions::from_handle(&handle)
        .ask(&questions)
        .await
        .expect_err("end of input is not an answer");
    assert!(error.contains("input ended"), "{error}");
}
