//! Context budget and trimming (spec §10; ticket 07).
//!
//! The pure half tests `trim`, `usable_input` and the pre-stream truncation
//! directly — they are pure functions and need no seam. The end-to-end half
//! drives the one assembly seam with a scripted provider and asserts what the
//! model actually received plus what stayed on the stream.

mod support;

use std::path::PathBuf;

use async_trait::async_trait;
use fs_agent::config::SessionConfig;
use fs_agent::context::{
    estimate_tokens, load_agents_md, trim, truncate_result, usable_input, TrimError, TrimPolicy,
    DROPPED_TOOL_RESULT,
};
use fs_agent::events::{read_events, Event, EventPayload, SessionId, SpeakerId, StopReason};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::capability::{caps_for, ModelCaps};
use fs_agent::provider::{ChatRequest, FinishReason, Message, StreamEvent, ToolSpec};
use fs_agent::render::RenderSinks;
use fs_agent::tools::{Effect, Tool, ToolContext, ToolError, ToolOutput};
use fs_agent::{assemble, AssemblyParts, Harness};
use serde_json::Value;
use support::{CaptureBuf, FakeProvider, Reply};

// --- message builders ------------------------------------------------------

fn pinned(content: &str) -> Message {
    // What `ContextInjected` projects to: a `user` message with no `name`.
    Message::User {
        content: content.to_owned(),
        name: None,
    }
}

fn user(content: &str) -> Message {
    Message::User {
        content: content.to_owned(),
        name: Some("user".to_owned()),
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
            .map(|(id, name)| fs_agent::provider::ToolCall {
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

/// Every `tool_call` still has exactly one result with its id.
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

// --- usable input ----------------------------------------------------------

#[test]
fn usable_input_reserves_output_space_per_model() {
    let mut caps = caps_for("deepseek-flash").unwrap();
    assert_eq!(usable_input(&caps), u64::from(caps.context_window) - 20_000);

    // A model whose output cap is under the reserve keeps only what it can write.
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

// --- trim ------------------------------------------------------------------

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
        "the old result body is the thing that goes"
    );
    assert_eq!(trimmed.len(), 6, "only the body changes, not the messages");
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
        "a loaded skill body is stickier than an ordinary result"
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

    // The oldest round went as a unit: its user, its call and its result.
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
    // Only the active round + the pinned head fit; the old round must go.
    let trimmed = trim(messages, 30, &TrimPolicy::default()).unwrap();

    assert_eq!(trimmed.first(), Some(&pinned("rules")));
    assert!(trimmed.contains(&user("second question")));
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

// --- pre-stream truncation -------------------------------------------------

#[test]
fn an_oversized_result_is_spilled_and_replaced_by_a_preview_with_a_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let outputs = dir.path().join("outputs");
    let text = "abcdefghij".repeat(400);
    let spilled = truncate_result(&text, "call-1", &outputs, 100);

    assert!(spilled.truncated);
    let pointer = spilled.pointer.clone().expect("the overflow lands on disk");
    assert_eq!(std::fs::read_to_string(&pointer).unwrap(), text);
    assert!(spilled.preview.chars().count() < text.chars().count());
    assert!(spilled.preview.contains("truncated"), "{}", spilled.preview);
    assert!(
        spilled.preview.contains(&pointer.display().to_string()),
        "the pointer must be in the stream: {}",
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
    // `outputs` is a file, so creating the spill directory must fail.
    let blocked = dir.path().join("outputs");
    std::fs::write(&blocked, "not a directory").unwrap();
    let text = "x".repeat(4_000);
    let spilled = truncate_result(&text, "call-1", &blocked, 100);

    assert!(spilled.truncated);
    assert!(spilled.pointer.is_none());
    assert!(
        spilled.preview.contains("could not be spilled"),
        "{}",
        spilled.preview
    );
    assert!(spilled.preview.chars().count() < text.chars().count());
}

#[test]
fn truncation_never_grows_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let outputs = dir.path().join("outputs");
    // Just over a tiny cap, but shorter than a preview plus its pointer note.
    let text = "x".repeat(180);
    let spilled = truncate_result(&text, "call-1", &outputs, 10);

    assert!(
        spilled.preview.chars().count() <= text.chars().count(),
        "a preview must never be longer than the body it replaces"
    );
    assert_eq!(
        spilled.preview, text,
        "the body is small enough that replacing it with a pointer would grow the stream"
    );
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

// --- the assembly seam -----------------------------------------------------

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

    let mut tools = fs_agent::tools::builtin();
    for tool in extra_tools {
        tools.register(tool);
    }

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        cwd: workspace.clone(),
        log_path: log_path.clone(),
        session_id: SessionId::new("s-context"),
        config,
        tools,
        locks: fs_agent::tools::PathLocks::new(),
        sinks: RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        },
        // `auto` keeps a test-only read-only tool allowed without an answerer.
        policy: Policy::for_mode(Mode::Auto),
        asker: None,
        hook: None,
        home: None,
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
    async fn run_turn(&mut self, input: &str) -> fs_agent::agent::TurnOutcome {
        self.harness
            .as_mut()
            .expect("harness already shut down")
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

/// A test-only read-only tool that returns a body of a fixed size.
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

/// A response that asks for one `blob` call.
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

/// Caps whose usable input is exactly `usable` tokens.
fn caps_with_usable_input(usable: u32) -> ModelCaps {
    let mut caps = caps_for("deepseek-flash").unwrap();
    caps.max_output_tokens = 20_000;
    caps.context_window = 20_000 + usable;
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
        // Just enough for one full round, not for two once the tool bodies can
        // no longer absorb the difference.
        caps_with_usable_input(190),
        SessionConfig::new("fake-model"),
        vec![Box::new(Blob { size: 400 })],
        None,
    )
    .await;

    fixture.run_turn("first").await;
    fixture.run_turn("second").await;
    fixture.shutdown().await;

    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 4, "two iterations per turn");
    // Turn 2's first iteration still had the whole first round...
    assert_eq!(
        tool_body(&requests[2], "call-1"),
        Some("b".repeat(400).as_str())
    );

    // ...and the second iteration dropped it as a unit, not as a stubbed body:
    // stubbing every old result was not enough, so the oldest round went.
    let messages = &requests[3].messages;
    assert!(
        tool_body(&requests[3], "call-1").is_none(),
        "the old round is removed whole, not stubbed"
    );
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::User { content, .. } if content == "first")),
        "the old round's question went with it"
    );
    assert!(
        !messages
            .iter()
            .any(|message| tool_content(message) == Some(DROPPED_TOOL_RESULT)),
        "no stub is left behind: the round went as a unit"
    );
    // The active round stays whole.
    assert_eq!(
        tool_body(&requests[3], "call-2"),
        Some("b".repeat(400).as_str())
    );
    assert!(messages
        .iter()
        .any(|message| matches!(message, Message::User { content, .. } if content == "second")));

    // Still read-only: the stream keeps both full bodies.
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
        // Turn 1 calls the tool, then answers; turn 2 is a long question that
        // pushes the *old* result out of the budget.
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
    assert_eq!(requests.len(), 3, "one request per iteration");

    // In turn 1 the model still saw the whole result.
    assert_eq!(
        message_tool_content(&requests[1]),
        vec!["b".repeat(400).as_str()]
    );

    // In turn 2 the oldest tool body was dropped, and only that.
    assert_eq!(
        message_tool_content(&requests[2]),
        vec![DROPPED_TOOL_RESULT],
        "the old ordinary tool result is the class that goes first"
    );
    assert!(
        requests[2].messages.iter().any(
            |message| matches!(message, Message::User { content, .. } if content.len() == 260)
        ),
        "the question being answered stays"
    );

    // Trimming is read-only: the stream still holds the full result body, and
    // the turn appended its own events and nothing else.
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

    // The injection is on the stream, with its source.
    let events = fixture.events();
    let injection = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ContextInjected { source, content } => Some((*source, content.clone())),
            _ => None,
        })
        .expect("AGENTS.md was injected");
    assert_eq!(injection.0, fs_agent::events::ContextSource::AgentsMd);
    assert_eq!(injection.1, rules);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::ContextInjected { .. }))
            .count(),
        1,
        "the rules are recorded once per session, not once per turn"
    );

    // It is the first user message, and trimming never removed it.
    for request in fixture.provider.requests() {
        assert_eq!(
            request.messages.first(),
            Some(&Message::User {
                content: rules.to_owned(),
                name: None,
            }),
            "identity -> rules -> history, every turn"
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
        .expect("the call has a result");
    assert!(
        output.chars().count() < 2_000,
        "the stream carries a preview, not the whole body"
    );
    assert!(output.contains("truncated"), "{output}");
    let pointer = outputs_dir.join("call-blob.txt");
    assert!(
        output.contains(&pointer.display().to_string()),
        "the stream carries the pointer: {output}"
    );
    assert_eq!(
        std::fs::read_to_string(&pointer).unwrap(),
        "b".repeat(2_000),
        "the overflow is on disk, byte for byte"
    );

    // The model sees the same preview the stream recorded.
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
        "a request that cannot fit is never sent"
    );
    assert!(
        fixture.stderr.text().contains("context budget"),
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
