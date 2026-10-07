//! 上下文预算与裁剪（spec §10；票 07）。
//!
//! 纯函数那一半直接测 `trim`、`usable_input` 与流前截断 ——
//! 它们是纯函数，不需要接缝。端到端那一半用脚本化 provider
//! 驱动那唯一一条组装接缝，断言模型真正收到的东西，
//! 以及在流上留住了什么。

mod support;

use std::path::PathBuf;

use async_trait::async_trait;
use heng::config::SessionConfig;
use heng::context::{
    DROPPED_TOOL_RESULT, TRUNCATED_MARKER, TrimError, TrimPolicy, estimate_tokens, load_agents_md,
    trim, truncate_result, usable_input,
};
use heng::events::{Event, EventPayload, SessionId, SpeakerId, StopReason, read_events};
use heng::permissions::{Mode, Policy};
use heng::provider::capability::{ModelCaps, caps_for};
use heng::provider::{ChatRequest, FinishReason, Message, StreamEvent, ToolSpec};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{Effect, Tool, ToolContext, ToolError, ToolOutput};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use serde_json::Value;
use support::{CaptureBuf, FakeProvider, Reply};

// --- 消息构造器 ------------------------------------------------------------

/// 一条 `ContextInjected` 投影成什么：一条没有 `name` 的 `user` 消息，
/// 标成 harness 注入而不是发言，所以裁剪把它钉在它所在的位置上
/// （票 15）。
fn injected(content: &str) -> Message {
    Message::User {
        content: content.to_owned(),
        name: None,
        injected: true,
    }
}

/// 一条开头的注入：形状与 [`injected`] 一样，落在被钉住的头部。
fn pinned(content: &str) -> Message {
    injected(content)
}

fn user(content: &str) -> Message {
    Message::User {
        content: content.to_owned(),
        name: Some("user".to_owned()),
        injected: false,
    }
}

fn assistant(content: &str) -> Message {
    Message::Assistant {
        content: Some(content.to_owned()),
        reasoning_content: None,
        tool_calls: Vec::new(),
        name: Some("kimi".to_owned()),
    }
}

fn assistant_calling(calls: &[(&str, &str)]) -> Message {
    Message::Assistant {
        content: None,
        reasoning_content: None,
        tool_calls: calls
            .iter()
            .map(|(id, name)| heng::provider::ToolCall {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                arguments: "{}".to_owned(),
            })
            .collect(),
        name: Some("kimi".to_owned()),
    }
}

fn tool_result(id: &str, content: &str) -> Message {
    Message::Tool {
        tool_call_id: id.to_owned(),
        content: content.to_owned(),
    }
}

fn tool_content(message: &Message) -> Option<&str> {
    match message {
        Message::Tool { content, .. } => Some(content),
        _ => None,
    }
}

/// 每一次 `tool_call` 仍然有、且只有一条带着它 id 的结果。
fn pairing_is_intact(messages: &[Message]) -> bool {
    let mut results: Vec<&str> = Vec::new();
    for message in messages {
        if let Message::Tool { tool_call_id, .. } = message {
            results.push(tool_call_id);
        }
    }
    messages.iter().all(|message| match message {
        Message::Assistant { tool_calls, .. } => tool_calls
            .iter()
            .all(|call| results.contains(&call.id.as_str())),
        _ => true,
    })
}

// --- 可用输入 --------------------------------------------------------------

#[test]
fn usable_input_reserves_output_space_per_model() {
    let mut caps = caps_for("deepseek-flash").unwrap();
    assert_eq!(usable_input(&caps), u64::from(caps.context_window) - 20_000);

    // 输出上限低于预留量的模型，只留下它写得出来的那么多。
    caps.max_output_tokens = 8_000;
    assert_eq!(usable_input(&caps), u64::from(caps.context_window) - 8_000);
}

#[test]
fn usable_input_never_underflows_on_a_tiny_window() {
    let mut caps = caps_for("deepseek-flash").unwrap();
    caps.context_window = 100;
    assert_eq!(usable_input(&caps), 0);
}

#[test]
fn there_is_no_global_budget_two_agents_get_two_budgets() {
    let kimi = caps_for("k3-256k").unwrap();
    let deepseek = caps_for("deepseek-flash").unwrap();
    assert_ne!(usable_input(&kimi), usable_input(&deepseek));
}

#[test]
fn tokens_are_estimated_as_characters_over_four() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abcde"), 2);
    assert_eq!(estimate_tokens(&"x".repeat(400)), 100);
}

// --- 裁剪 ------------------------------------------------------------------

#[test]
fn trim_leaves_a_request_that_fits_alone() {
    let messages = vec![pinned("rules"), user("hello"), assistant("hi")];
    let before = messages.clone();
    let trimmed = trim(messages, 1_000, &TrimPolicy::default()).unwrap();
    assert_eq!(trimmed, before);
}

#[test]
fn trim_stubs_old_ordinary_tool_results_before_touching_anything_else() {
    let messages = vec![
        pinned("rules"),
        user("first question"),
        assistant_calling(&[("a", "read_file")]),
        tool_result("a", &"x".repeat(400)),
        assistant("answer"),
        user("second question"),
    ];
    let trimmed = trim(messages, 60, &TrimPolicy::default()).unwrap();

    assert_eq!(
        tool_content(&trimmed[3]),
        Some(DROPPED_TOOL_RESULT),
        "走掉的是那条旧结果的正文"
    );
    assert_eq!(trimmed.len(), 6, "变的只有正文，消息本身没变");
    assert_eq!(trimmed[1], user("first question"));
    assert_eq!(trimmed[2], assistant_calling(&[("a", "read_file")]));
    assert_eq!(trimmed[4], assistant("answer"));
    assert_eq!(trimmed[5], user("second question"));
    assert!(pairing_is_intact(&trimmed));
}

#[test]
fn trim_prefers_ordinary_results_over_skill_bodies() {
    let messages = vec![
        pinned("rules"),
        user("first question"),
        assistant_calling(&[("o", "read_file"), ("s", "skill")]),
        tool_result("o", &"x".repeat(400)),
        tool_result("s", &"y".repeat(400)),
        user("second question"),
    ];
    let trimmed = trim(messages, 140, &TrimPolicy::default()).unwrap();

    assert_eq!(tool_content(&trimmed[3]), Some(DROPPED_TOOL_RESULT));
    assert_eq!(
        tool_content(&trimmed[4]),
        Some("y".repeat(400).as_str()),
        "已加载的技能正文比普通结果更难被丢掉"
    );
}

#[test]
fn trim_reaches_a_skill_body_only_after_every_ordinary_result_is_stubbed() {
    let messages = vec![
        pinned("rules"),
        user("first question"),
        assistant_calling(&[("o", "read_file"), ("s", "skill")]),
        tool_result("o", &"x".repeat(400)),
        tool_result("s", &"y".repeat(400)),
        user("second question"),
    ];
    let trimmed = trim(messages, 60, &TrimPolicy::default()).unwrap();

    assert_eq!(tool_content(&trimmed[3]), Some(DROPPED_TOOL_RESULT));
    assert_eq!(tool_content(&trimmed[4]), Some(DROPPED_TOOL_RESULT));
}

#[test]
fn trim_drops_old_whole_rounds_only_once_the_bodies_are_gone() {
    let messages = vec![
        pinned("rules"),
        user("first question"),
        assistant_calling(&[("a", "read_file")]),
        tool_result("a", &"x".repeat(400)),
        assistant(&"answer ".repeat(200)),
        user("second question"),
        assistant("second answer"),
    ];
    let trimmed = trim(messages, 60, &TrimPolicy::default()).unwrap();

    // 最旧的那一轮整块走掉：它的 user、它的调用与它的结果。
    assert_eq!(trimmed[0], pinned("rules"));
    assert_eq!(trimmed[1], user("second question"));
    assert_eq!(trimmed[2], assistant("second answer"));
    assert!(pairing_is_intact(&trimmed));
}

#[test]
fn trim_keeps_the_pinned_injection_and_the_active_round() {
    let messages = vec![
        pinned("rules"),
        user("first question"),
        assistant(&"answer ".repeat(200)),
        user("second question"),
    ];
    // 只有当前轮加上被钉住的头部装得下；旧的那一轮必须走。
    let trimmed = trim(messages, 30, &TrimPolicy::default()).unwrap();

    assert_eq!(trimmed.first(), Some(&pinned("rules")));
    assert!(trimmed.contains(&user("second question")));
}

#[test]
fn trim_keeps_a_mid_session_injection_while_dropping_the_round_around_it() {
    // 计划模式的指令注入在一轮的中途，而不是在头部：
    // 它必须被钉在它所在的位置，而且它不能把这一轮切成
    // 两半、让其中一半逃过丢弃（spec §13）。
    let messages = vec![
        pinned("rules"),
        user("first question"),
        injected("plan mode: only PLAN.md may be written"),
        assistant(&"answer ".repeat(200)),
        user("second question"),
    ];
    let trimmed = trim(messages, 60, &TrimPolicy::default()).unwrap();

    assert_eq!(
        trimmed,
        vec![
            pinned("rules"),
            injected("plan mode: only PLAN.md may be written"),
            user("second question"),
        ],
        "旧的那一轮整块走掉，而注入留着"
    );
}

#[test]
fn trim_hard_fails_when_the_pinned_injection_alone_exceeds_the_budget() {
    let messages = vec![pinned(&"r".repeat(400)), user("hello")];
    let error = trim(messages, 10, &TrimPolicy::default()).unwrap_err();
    assert!(matches!(error, TrimError::OverBudget { budget: 10, .. }));
}

#[test]
fn trim_hard_fails_rather_than_dropping_the_question_being_answered() {
    let messages = vec![pinned("rules"), user(&"q".repeat(4_000))];
    assert!(trim(messages, 10, &TrimPolicy::default()).is_err());
}

// --- 流前截断 --------------------------------------------------------------

#[test]
fn an_oversized_result_is_spilled_and_replaced_by_a_preview_with_a_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let outputs = dir.path().join("outputs");
    let text = "abcdefghij".repeat(400);
    let spilled = truncate_result(&text, "call-1", &outputs, 100);

    assert!(spilled.truncated);
    let pointer = spilled.pointer.clone().expect("溢出的内容落在磁盘上");
    assert_eq!(std::fs::read_to_string(&pointer).unwrap(), text);
    assert!(spilled.preview.chars().count() < text.chars().count());
    assert!(
        spilled.preview.contains(TRUNCATED_MARKER),
        "{}",
        spilled.preview
    );
    assert!(
        spilled.preview.contains(&pointer.display().to_string()),
        "那个指针必须在流上：{}",
        spilled.preview
    );
}

#[test]
fn a_result_under_the_cap_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let spilled = truncate_result("short", "call-1", &dir.path().join("outputs"), 100);
    assert!(!spilled.truncated);
    assert_eq!(spilled.preview, "short");
    assert!(spilled.pointer.is_none());
}

#[test]
fn a_failed_spill_degrades_to_the_preview_and_never_fails() {
    let dir = tempfile::tempdir().unwrap();
    // `outputs` 是个文件，所以建溢出目录必然失败。
    let blocked = dir.path().join("outputs");
    std::fs::write(&blocked, "not a directory").unwrap();
    let text = "x".repeat(4_000);
    let spilled = truncate_result(&text, "call-1", &blocked, 100);

    assert!(spilled.truncated);
    assert!(spilled.pointer.is_none());
    assert!(
        spilled.preview.contains("没能溢出落盘"),
        "{}",
        spilled.preview
    );
    assert!(spilled.preview.chars().count() < text.chars().count());
}

#[test]
fn truncation_never_grows_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let outputs = dir.path().join("outputs");
    // 刚过一个很小的上限，但比一条预览加上它的指针附注还短。
    let text = "x".repeat(180);
    let spilled = truncate_result(&text, "call-1", &outputs, 10);

    assert!(
        spilled.preview.chars().count() <= text.chars().count(),
        "预览绝不能比它替换掉的正文更长"
    );
    assert_eq!(spilled.preview, text, "正文够小，换成指针反而会把流撑大");
}

#[test]
fn agents_md_is_read_when_present_and_skipped_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_agents_md(dir.path()), None);

    std::fs::write(dir.path().join("AGENTS.md"), "build with cargo\n").unwrap();
    assert_eq!(
        load_agents_md(dir.path()).as_deref(),
        Some("build with cargo\n")
    );

    std::fs::write(dir.path().join("AGENTS.md"), "   \n").unwrap();
    assert_eq!(load_agents_md(dir.path()), None);
}

// --- 组装接缝 --------------------------------------------------------------

struct Fixture {
    harness: Option<Harness>,
    provider: FakeProvider,
    stderr: CaptureBuf,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(
    replies: Vec<Reply>,
    caps: ModelCaps,
    config: SessionConfig,
    extra_tools: Vec<Box<dyn Tool>>,
    agents_md: Option<&str>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    if let Some(text) = agents_md {
        std::fs::write(workspace.join("AGENTS.md"), text).unwrap();
    }
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::with_caps(replies, caps);
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();

    let mut tools = heng::tools::builtin(false);
    for tool in extra_tools {
        tools.register(tool);
    }

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-context"),
            tools,
            locks: heng::tools::PathLocks::new(),
            // `auto` 让一个只给测试用的只读工具，不用作答者也被放行。
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness: Some(harness),
        provider,
        stderr,
        log_path,
        _dir: dir,
    }
}

impl Fixture {
    async fn run_turn(&mut self, input: &str) -> heng::agent::TurnOutcome {
        self.harness
            .as_mut()
            .expect("harness 已经关掉了")
            .run_turn(input)
            .await
            .unwrap()
    }

    async fn shutdown(&mut self) {
        if let Some(harness) = self.harness.take() {
            harness.shutdown().await;
        }
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }
}

/// 一个只给测试用的只读工具，返回一份固定大小的正文。
struct Blob {
    size: usize,
}

#[async_trait]
impl Tool for Blob {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "blob".to_owned(),
            description: "test-only large output".to_owned(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(&self, _ctx: &ToolContext<'_>, _args: Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::new("b".repeat(self.size)))
    }
}

/// 一条要求调用一次 `blob` 的响应。
fn blob_reply(id: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: "blob".to_owned(),
            arguments: "{}".to_owned(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// 一份能力表，它的可用输入正好是在被钉住的身份**之外**还有
/// `usable` 个 token —— 那个身份走在每个请求最前面，也占预算。
fn caps_with_usable_input(usable: u32) -> ModelCaps {
    let identity = heng::context::estimate_tokens(&heng::agent::agent_identity());
    let mut caps = caps_for("deepseek-flash").unwrap();
    caps.max_output_tokens = 20_000;
    caps.context_window = 20_000 + usable + identity as u32;
    caps
}

fn message_tool_content(request: &ChatRequest) -> Vec<&str> {
    request.messages.iter().filter_map(tool_content).collect()
}

fn tool_body<'a>(request: &'a ChatRequest, tool_call_id: &str) -> Option<&'a str> {
    request.messages.iter().find_map(|message| match message {
        Message::Tool {
            tool_call_id: id,
            content,
        } if id == tool_call_id => Some(content.as_str()),
        _ => None,
    })
}

#[tokio::test]
async fn over_budget_history_drops_an_old_whole_round_only_after_the_bodies_are_gone() {
    let mut fixture = fixture(
        vec![
            blob_reply("call-1"),
            Reply::text(&"a".repeat(300)),
            blob_reply("call-2"),
            Reply::text("done"),
        ],
        // 正好够跑满一轮；等工具正文再也吸不掉那份差额之后，
        // 就跑不下两轮了。这个数字是按**生成文本的 token 估计**调出来的
        // （中文更短，所以账变小，预算跟着收）。
        caps_with_usable_input(185),
        SessionConfig::new("fake-model"),
        vec![Box::new(Blob { size: 400 })],
        None,
    )
    .await;

    fixture.run_turn("first").await;
    fixture.run_turn("second").await;
    fixture.shutdown().await;

    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 4, "每个回合两个迭代");
    // 第二轮的第一个迭代还带着完整的第一轮……
    assert_eq!(
        tool_body(&requests[2], "call-1"),
        Some("b".repeat(400).as_str())
    );

    // ……第二个迭代把它整块丢掉了，而不是留个被替换的正文：
    // 把每一条旧结果都换成替身还不够，所以最旧的那一轮走了。
    let messages = &requests[3].messages;
    assert!(
        tool_body(&requests[3], "call-1").is_none(),
        "旧的那一轮被整块去掉，而不是留个替身"
    );
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::User { content, .. } if content == "first")),
        "旧那一轮的问题跟着它一起走了"
    );
    assert!(
        !messages
            .iter()
            .any(|message| tool_content(message) == Some(DROPPED_TOOL_RESULT)),
        "没有留下任何替身：那一轮整块走了"
    );
    // 当前轮保持完整。
    assert_eq!(
        tool_body(&requests[3], "call-2"),
        Some("b".repeat(400).as_str())
    );
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::User { content, .. } if content == "second"))
    );

    // 仍然只读：流上保留着两份完整的正文。
    let events = fixture.events();
    let outputs: Vec<&String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                output: Some(output),
                ..
            } => Some(output),
            _ => None,
        })
        .collect();
    assert_eq!(outputs.len(), 2);
    assert!(outputs.iter().all(|output| output.len() == 400));
}

#[tokio::test]
async fn an_over_budget_history_is_trimmed_without_deleting_a_log_line() {
    let mut fixture = fixture(
        // 第一个回合调用工具、然后作答；第二个回合是一个很长的问题，
        // 把**旧的**那条结果挤出预算。
        vec![
            blob_reply("call-blob"),
            Reply::text("done"),
            Reply::text("second"),
        ],
        caps_with_usable_input(140),
        SessionConfig::new("fake-model"),
        vec![Box::new(Blob { size: 400 })],
        None,
    )
    .await;

    fixture.run_turn("first").await;
    let before = fixture.events().len();
    let outcome = fixture.run_turn(&"q".repeat(260)).await;
    assert_eq!(outcome.reason, StopReason::Completed);
    fixture.shutdown().await;

    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 3, "每个迭代一个请求");

    // 在第一个回合里，模型看到的还是完整的结果。
    assert_eq!(
        message_tool_content(&requests[1]),
        vec!["b".repeat(400).as_str()]
    );

    // 在第二个回合里，最旧的工具正文被丢掉了，而且只丢了它。
    assert_eq!(
        message_tool_content(&requests[2]),
        vec![DROPPED_TOOL_RESULT],
        "最旧的普通工具结果是先走的那一类"
    );
    assert!(
        requests[2].messages.iter().any(
            |message| matches!(message, Message::User { content, .. } if content.len() == 260)
        ),
        "正在被回答的那个问题留着"
    );

    // 裁剪是只读的：流上仍然握着完整的工具结果正文，
    // 而这个回合只追加了它自己那些事件，别的什么都没加。
    let events = fixture.events();
    let outputs: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                output: Some(output),
                ..
            } => Some(output.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(outputs, vec!["b".repeat(400)]);
    let tail: Vec<&str> = events[before..]
        .iter()
        .map(|event| event.payload.kind())
        .collect();
    assert_eq!(
        tail,
        vec![
            "MessageCompleted",
            "TurnStarted",
            "UsageRecorded",
            "MessageCompleted",
            "TurnEnded",
        ]
    );
}

#[tokio::test]
async fn the_agents_md_injection_is_recorded_once_and_stays_the_first_message() {
    let rules = "PROJECT RULES: always run cargo test before claiming done.\n";
    let mut fixture = fixture(
        vec![
            blob_reply("call-blob"),
            Reply::text("done"),
            Reply::text("second"),
        ],
        caps_with_usable_input(140),
        SessionConfig::new("fake-model"),
        vec![Box::new(Blob { size: 400 })],
        Some(rules),
    )
    .await;

    fixture.run_turn("first").await;
    fixture.run_turn(&"q".repeat(260)).await;
    fixture.shutdown().await;

    // 注入在流上，带着它的来源。
    let events = fixture.events();
    let injection = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ContextInjected { source, content } => {
                Some((source.clone(), content.clone()))
            }
            _ => None,
        })
        .expect("AGENTS.md 被注入了");
    assert_eq!(injection.0, heng::events::ContextSource::AgentsMd);
    assert_eq!(injection.1, rules);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::ContextInjected { .. }))
            .count(),
        1,
        "规则每个会话记一次，而不是每个回合记一次"
    );

    // 身份走在最前面，规则是第一条 user 消息，而裁剪
    // 从来没把这两样里任何一样删掉。
    for request in fixture.provider.requests() {
        assert_eq!(
            request.messages.first(),
            Some(&Message::System {
                content: heng::agent::agent_identity(),
                name: None,
            }),
            "程序的身份走在每个请求最前面"
        );
        assert_eq!(
            request.messages.get(1),
            Some(&Message::User {
                content: rules.to_owned(),
                name: None,
                injected: true,
            }),
            "每个回合都是：身份 -> 规则 -> 历史"
        );
    }
}

#[tokio::test]
async fn an_oversized_tool_result_is_spilled_before_it_reaches_the_stream() {
    let mut fixture = fixture(
        vec![blob_reply("call-blob"), Reply::text("done")],
        caps_for("deepseek-flash").unwrap(),
        SessionConfig::new("fake-model").with_max_tool_result_tokens(50),
        vec![Box::new(Blob { size: 2_000 })],
        None,
    )
    .await;
    let outputs_dir = fixture
        .harness
        .as_ref()
        .unwrap()
        .outputs_dir()
        .to_path_buf();

    fixture.run_turn("read the blob").await;
    fixture.shutdown().await;

    let events = fixture.events();
    let output = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                output: Some(output),
                ..
            } => Some(output.clone()),
            _ => None,
        })
        .expect("这次调用有一条结果");
    assert!(
        output.chars().count() < 2_000,
        "流上扛的是预览，不是整份正文"
    );
    assert!(output.contains(TRUNCATED_MARKER), "{output}");
    let pointer = outputs_dir.join("call-blob.txt");
    assert!(
        output.contains(&pointer.display().to_string()),
        "流上扛着那个指针：{output}"
    );
    assert_eq!(
        std::fs::read_to_string(&pointer).unwrap(),
        "b".repeat(2_000),
        "溢出的内容在磁盘上，逐字节都在"
    );

    // 模型看到的就是流上记下的那条预览。
    let requests = fixture.provider.requests();
    assert_eq!(message_tool_content(&requests[1]), vec![output.as_str()]);
}

#[tokio::test]
async fn a_turn_hard_fails_when_even_the_pinned_injection_does_not_fit() {
    let mut fixture = fixture(
        vec![Reply::text("never sent")],
        caps_with_usable_input(0),
        SessionConfig::new("fake-model"),
        Vec::new(),
        None,
    )
    .await;

    let outcome = fixture.run_turn("hello").await;
    assert_eq!(outcome.reason, StopReason::Error);
    fixture.shutdown().await;

    assert!(
        fixture.provider.requests().is_empty(),
        "装不下的请求永远不会被发出去"
    );
    assert!(
        fixture.stderr.text().contains("上下文预算"),
        "{}",
        fixture.stderr.text()
    );
    let events = fixture.events();
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(EventPayload::TurnEnded {
            reason: StopReason::Error
        })
    ));
}
