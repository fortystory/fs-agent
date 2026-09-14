//! Projection and speaker attribution, tested as a pure function of the stream
//! (spec §5; Testing Decisions' "directly tested pure functions" seam).
//!
//! The contract under test is the `messages` an agent would replay: who becomes
//! `assistant`, who is downgraded to `user`, what survives from another
//! speaker's tool round-trip, and how the speaker's own round-trip plus hook
//! feedback merge. The exact spelling of the model-side prefix is deliberately
//! not asserted (spec's Testing Decisions): the tests pin the properties that
//! matter — attribution, visibility, merge boundaries and ordering.

use fs_agent::config::GenerationParams;
use fs_agent::events::{
    hook_format, ContextSource, EventLog, EventPayload, Role, RoundMode, SessionId, SpeakerId,
    StopReason, ToolCallId,
};
use fs_agent::provider::capability::{caps_for, ModelCaps};
use fs_agent::provider::openai::build_body;
use fs_agent::provider::projection::project;
use fs_agent::provider::{ChatRequest, Message, ToolChoice};

fn deepseek() -> SpeakerId {
    SpeakerId::Debater("deepseek".into())
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

/// A third participant's viewpoint: the synthesizer, which is `System` (spec §2).
fn synthesizer() -> SpeakerId {
    SpeakerId::System
}

fn caps() -> ModelCaps {
    caps_for("deepseek-flash").unwrap()
}

/// Build a log from a script, keeping the temp directory alive with it.
fn log(script: impl FnOnce(&mut EventLog)) -> (tempfile::TempDir, EventLog) {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::create(dir.path().join("log.jsonl")).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: SessionId::new("s-1"),
            cwd: "/workspace".to_owned(),
            schema_version: fs_agent::events::SCHEMA_VERSION,
        },
    )
    .unwrap();
    script(&mut log);
    (dir, log)
}

fn user_says(log: &mut EventLog, text: &str) {
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
}

fn say(log: &mut EventLog, speaker: &SpeakerId, text: &str) {
    say_with_reasoning(log, speaker, text, None);
}

fn say_with_reasoning(
    log: &mut EventLog,
    speaker: &SpeakerId,
    text: &str,
    reasoning: Option<&str>,
) {
    log.append(
        speaker.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),
        },
    )
    .unwrap();
}

fn call(log: &mut EventLog, speaker: &SpeakerId, id: &str, tool: &str, args: serde_json::Value) {
    log.append(
        speaker.clone(),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    )
    .unwrap();
}

fn result(log: &mut EventLog, speaker: &SpeakerId, id: &str, output: &str) {
    log.append(
        speaker.clone(),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok: true,
            output: Some(output.to_owned()),
            error: None,
            duration_ms: 1,
        },
    )
    .unwrap();
}

fn user_messages(messages: &[Message]) -> Vec<&Message> {
    messages
        .iter()
        .filter(|message| matches!(message, Message::User { .. }))
        .collect()
}

fn text_of(message: &Message) -> &str {
    match message {
        Message::User { content, .. } | Message::System { content, .. } => content,
        Message::Assistant { content, .. } => content.as_deref().unwrap_or(""),
        Message::Tool { content, .. } => content,
    }
}

// --- another speaker's turn -------------------------------------------------

#[test]
fn another_speakers_text_is_kept_but_its_tool_round_trip_shrinks_to_one_summary_line() {
    let (_dir, log) = log(|log| {
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
        )
        .unwrap();
        say_with_reasoning(log, &deepseek(), "I checked the file", Some("SECRET-THINK"));
        call(
            log,
            &deepseek(),
            "call-1",
            "read_file",
            serde_json::json!({ "file_path": "src/lib.rs" }),
        );
        result(log, &deepseek(), "call-1", "SECRET-BODY");
    });

    let messages = project(&log, &kimi(), &caps());

    assert_eq!(
        messages.len(),
        1,
        "only the merged other block: {messages:?}"
    );
    let user = user_messages(&messages)[0];
    let content = text_of(user);
    assert!(content.contains("I checked the file"), "{content}");
    assert!(
        content.contains("read_file") && content.contains("src/lib.rs"),
        "the tool call survives as a one-line summary: {content}"
    );
    assert!(
        content.contains("轮 1") && content.contains("deepseek"),
        "each other-speaker segment is attributed: {content}"
    );
    assert!(
        !content.contains("SECRET-BODY"),
        "result body is not projected"
    );
    assert!(
        !content.contains("SECRET-THINK"),
        "another speaker's reasoning is not projected"
    );
    assert!(
        !messages.iter().any(|m| matches!(m, Message::Tool { .. })),
        "the paired tool result is dropped, not rewritten into a broken pair"
    );
}

// --- the speaker's own turn -------------------------------------------------

#[test]
fn own_reasoning_is_replayed_and_the_tool_round_trip_merges_with_post_hook_feedback() {
    let (_dir, log) = log(|log| {
        say_with_reasoning(log, &kimi(), "reading", Some("my-thoughts"));
        call(
            log,
            &kimi(),
            "call-1",
            "read_file",
            serde_json::json!({ "file_path": "src/lib.rs" }),
        );
        result(log, &kimi(), "call-1", "file body");
        log.append(
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "lint".to_owned(),
                outcome: hook_format::feedback("trailing whitespace"),
            },
        )
        .unwrap();
    });

    let messages = project(&log, &kimi(), &caps());

    assert_eq!(messages.len(), 2, "{messages:?}");
    match &messages[0] {
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            name,
        } => {
            assert_eq!(content.as_deref(), Some("reading"));
            assert_eq!(
                reasoning_content.as_deref(),
                Some("my-thoughts"),
                "the model's own reasoning must replay"
            );
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call-1");
            assert_eq!(name.as_deref(), Some("kimi"));
        }
        other => panic!("expected an assistant message, got {other:?}"),
    }
    match &messages[1] {
        Message::Tool {
            tool_call_id,
            content,
        } => {
            assert_eq!(tool_call_id, "call-1");
            // Exactly one tool message carries both the result and the hook's
            // feedback: a provider allows one `tool` message per `tool_call`.
            assert!(content.starts_with("file body"), "{content}");
            assert!(
                content.contains(hook_format::FEEDBACK_MARKER)
                    && content.contains("trailing whitespace"),
                "{content}"
            );
        }
        other => panic!("expected the merged tool message, got {other:?}"),
    }
}

#[test]
fn a_failed_post_hook_loses_its_feedback_but_keeps_the_result() {
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "reading");
        call(log, &kimi(), "call-1", "read_file", serde_json::json!({}));
        result(log, &kimi(), "call-1", "file body");
        log.append(
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "lint".to_owned(),
                outcome: hook_format::failed("lint exploded"),
            },
        )
        .unwrap();
    });

    let messages = project(&log, &kimi(), &caps());

    let tool = messages
        .iter()
        .find_map(|message| match message {
            Message::Tool { content, .. } => Some(content),
            _ => None,
        })
        .expect("the result still has its one tool message");
    assert_eq!(tool, "file body");
    assert!(!tool.contains("lint exploded"));
}

#[test]
fn an_interleaved_other_speaker_never_leaves_a_tool_call_without_its_result() {
    // A valid turn never interleaves, but a hand-built or future-broken log
    // must not produce the unpaired `tool_call` the wire check rejects.
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "working");
        call(log, &kimi(), "call-1", "read_file", serde_json::json!({}));
        say(log, &deepseek(), "interjecting");
        result(log, &kimi(), "call-1", "body");
    });

    let messages = project(&log, &kimi(), &caps());

    let calls: Vec<String> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { tool_calls, .. } => Some(tool_calls.iter().map(|c| c.id.clone())),
            _ => None,
        })
        .flatten()
        .collect();
    let results: Vec<String> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Tool { tool_call_id, .. } => Some(tool_call_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, vec!["call-1".to_owned()]);
    assert_eq!(
        results,
        vec!["call-1".to_owned()],
        "the assistant group keeps its one tool message: {messages:?}"
    );
}

// --- self versus other, on the same events ----------------------------------

#[test]
fn the_same_events_project_differently_for_each_speaker_and_each_projection_is_stable() {
    let (_dir, log) = log(|log| {
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
        )
        .unwrap();
        say_with_reasoning(log, &deepseek(), "deepseek's answer", Some("d-think"));
        call(
            log,
            &deepseek(),
            "d-call",
            "read_file",
            serde_json::json!({}),
        );
        result(log, &deepseek(), "d-call", "d-body");
        say_with_reasoning(log, &kimi(), "kimi's answer", Some("k-think"));
    });

    let for_kimi = project(&log, &kimi(), &caps());
    let for_deepseek = project(&log, &deepseek(), &caps());

    assert_ne!(
        for_kimi, for_deepseek,
        "the same events must project to different windows"
    );

    // The projection is a pure function: re-running it is byte-for-byte stable,
    // which is what "recomputable from the stream + the rules" buys.
    assert_eq!(for_kimi, project(&log, &kimi(), &caps()));
    assert_eq!(for_deepseek, project(&log, &deepseek(), &caps()));

    // kimi sees deepseek compressed to one user block, and its own turn with
    // its own reasoning.
    let kimi_text = for_kimi.iter().map(text_of).collect::<Vec<_>>().join("\n");
    assert!(kimi_text.contains("deepseek's answer"), "{kimi_text}");
    assert!(!kimi_text.contains("d-think"), "{kimi_text}");
    let kimi_own = for_kimi
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("kimi's own message");
    match kimi_own {
        Message::Assistant {
            reasoning_content, ..
        } => assert_eq!(reasoning_content.as_deref(), Some("k-think")),
        other => panic!("expected assistant, got {other:?}"),
    }
    // deepseek sees its own tool round-trip, kimi sees only a summary.
    assert!(for_deepseek
        .iter()
        .any(|message| matches!(message, Message::Tool { .. })));
    assert!(!for_kimi
        .iter()
        .any(|message| matches!(message, Message::Tool { .. })));
}

// --- merge rules ------------------------------------------------------------

#[test]
fn the_pinned_head_never_merges_and_a_round_is_a_hard_boundary() {
    let (_dir, log) = log(|log| {
        user_says(log, "the task");
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
        )
        .unwrap();
        say(log, &kimi(), "round 1 from kimi");
        say(log, &deepseek(), "round 1 from deepseek");
        log.append(
            SpeakerId::System,
            EventPayload::RoundEnded {
                round: 1,
                reason: StopReason::NoDivergence,
            },
        )
        .unwrap();
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 2,
                mode: RoundMode::Targeted,
            },
        )
        .unwrap();
        say(log, &kimi(), "round 2 from kimi");
    });

    let messages = project(&log, &synthesizer(), &caps());
    let users = user_messages(&messages);

    assert_eq!(users.len(), 3, "{messages:?}");
    assert_eq!(
        text_of(users[0]),
        "the task",
        "the pinned head is byte-stable and absorbs nothing"
    );
    let round_1 = text_of(users[1]);
    assert!(
        round_1.contains("round 1 from kimi") && round_1.contains("round 1 from deepseek"),
        "consecutive others merge with no count threshold: {round_1}"
    );
    assert!(!round_1.contains("round 2"), "{round_1}");

    let round_2 = text_of(users[2]);
    assert!(round_2.contains("round 2 from kimi"), "{round_2}");
    assert!(!round_2.contains("round 1"), "{round_2}");
}

#[test]
fn the_first_user_message_does_not_absorb_a_later_speakers_speech() {
    // No round boundary here, so the pin is the only thing keeping the head
    // from growing with another speaker's text.
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "first");
        say(log, &deepseek(), "second");
    });

    let messages = project(&log, &synthesizer(), &caps());
    let users = user_messages(&messages);

    assert_eq!(users.len(), 2, "{messages:?}");
    assert_eq!(text_of(users[0]), "first");
    assert_eq!(text_of(users[1]), "second");
}

#[test]
fn a_context_injection_is_pinned_and_does_not_merge() {
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "before");
        log.append(
            SpeakerId::User,
            EventPayload::ContextInjected {
                source: ContextSource::PlanMode,
                content: "read PLAN.md before acting".to_owned(),
            },
        )
        .unwrap();
        say(log, &kimi(), "after");
    });

    let messages = project(&log, &synthesizer(), &caps());
    let users = user_messages(&messages);

    assert_eq!(
        users.len(),
        3,
        "the injection stays its own message: {messages:?}"
    );
    assert_eq!(text_of(users[1]), "read PLAN.md before acting");
    assert!(text_of(users[0]).contains("before"));
    assert!(text_of(users[2]).contains("after"));
}

// --- executor visibility ----------------------------------------------------

#[test]
fn an_executors_events_stay_out_of_a_debaters_projection_but_not_its_own() {
    let executor = SpeakerId::Executor("e-1".into());
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "kimi's take");
        say_with_reasoning(log, &executor, "executor working", Some("e-think"));
        call(log, &executor, "e-call", "read_file", serde_json::json!({}));
        result(log, &executor, "e-call", "e-body");
    });

    let for_synthesizer = project(&log, &synthesizer(), &caps());
    let synthesizer_text = for_synthesizer
        .iter()
        .map(text_of)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(synthesizer_text.contains("kimi's take"));
    assert!(
        !synthesizer_text.contains("executor working"),
        "{synthesizer_text}"
    );
    assert!(!synthesizer_text.contains("e-body"), "{synthesizer_text}");

    let for_executor = project(&log, &executor, &caps());
    assert!(for_executor
        .iter()
        .any(|message| matches!(message, Message::Assistant { .. })));
    assert!(for_executor
        .iter()
        .any(|message| matches!(message, Message::Tool { .. })));
}

// --- names ------------------------------------------------------------------

#[test]
fn names_are_sanitized_and_never_carried_on_tool_messages() {
    let odd = SpeakerId::Debater("ki mi/../x\u{4f60}".into());
    let (_dir, log) = log(|log| {
        say(log, &odd, "hello");
        call(log, &odd, "call-1", "read_file", serde_json::json!({}));
        result(log, &odd, "call-1", "body");
    });

    let messages = project(&log, &odd, &caps());
    let own = messages
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("the speaker's own assistant message");
    let name = match own {
        Message::Assistant { name, .. } => name.as_deref().unwrap(),
        other => panic!("expected assistant, got {other:?}"),
    };
    assert!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "name must be sanitized to [A-Za-z0-9_-]: {name}"
    );
    assert!(name.len() <= 64, "name must stay short: {name}");
}

// --- history ----------------------------------------------------------------

#[test]
fn superseded_ranges_are_excluded_from_the_projection() {
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "old answer");
        say(log, &kimi(), "replacement answer");
        log.append(
            SpeakerId::System,
            EventPayload::HistorySuperseded {
                targets: vec![2],
                reason: fs_agent::events::HistoryReason::Regenerate,
                summary: None,
            },
        )
        .unwrap();
    });

    let messages = project(&log, &synthesizer(), &caps());
    let text = messages.iter().map(text_of).collect::<Vec<_>>().join("\n");
    assert!(!text.contains("old answer"), "{text}");
    assert!(text.contains("replacement answer"), "{text}");
}

// --- wire serialization -----------------------------------------------------

#[test]
fn projected_messages_serialize_into_both_vendors_wire_shapes() {
    let (_dir, log) = log(|log| {
        say_with_reasoning(log, &kimi(), "answer", Some("thought"));
        call(log, &kimi(), "call-1", "read_file", serde_json::json!({}));
        result(log, &kimi(), "call-1", "body");
        say(log, &deepseek(), "deepseek's reply");
    });

    let request = ChatRequest {
        model: "fake-model".to_owned(),
        messages: project(&log, &kimi(), &caps()),
        tools: Vec::new(),
        tool_choice: ToolChoice::Auto,
        params: GenerationParams {
            max_output_tokens: Some(100),
            ..GenerationParams::default()
        },
        cache_key: None,
    };

    for (vendor, caps) in [
        ("kimi", caps_for("kimi-k3").unwrap()),
        ("deepseek", caps_for("deepseek-v4-pro").unwrap()),
    ] {
        let (body, warnings) = build_body(&request, caps);
        assert!(warnings.is_empty(), "{vendor}: {warnings:?}");
        let messages = body["messages"].as_array().unwrap();

        let assistant = messages
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap_or_else(|| panic!("{vendor}: an assistant message"));
        assert_eq!(assistant["reasoning_content"], "thought", "{vendor}");
        assert_eq!(assistant["name"], "kimi", "{vendor}");

        let tool = messages
            .iter()
            .find(|message| message["role"] == "tool")
            .unwrap_or_else(|| panic!("{vendor}: a tool message"));
        assert!(
            tool.get("name").is_none(),
            "{vendor}: a tool message must not carry `name`: {tool}"
        );
        assert_eq!(tool["tool_call_id"], "call-1", "{vendor}");

        let user = messages
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap_or_else(|| panic!("{vendor}: the other-speaker message"));
        assert_eq!(user["name"], "deepseek", "{vendor}");
    }

    // The output-token field really is a per-vendor difference.
    let (kimi_body, _) = build_body(&request, caps_for("kimi-k3").unwrap());
    let (deepseek_body, _) = build_body(&request, caps_for("deepseek-v4-pro").unwrap());
    assert_eq!(kimi_body["max_completion_tokens"], 100);
    assert_eq!(deepseek_body["max_tokens"], 100);
}

#[test]
fn reasoning_replay_is_data_on_the_capability_table_not_a_vendor_branch() {
    let (_dir, log) = log(|log| {
        say_with_reasoning(log, &kimi(), "answer", Some("thought"));
    });

    let mut caps = caps();
    caps.requires_reasoning_replay = false;

    let messages = project(&log, &kimi(), &caps);
    match &messages[0] {
        Message::Assistant {
            reasoning_content, ..
        } => assert!(
            reasoning_content.is_none(),
            "a model that does not require replay gets none: {reasoning_content:?}"
        ),
        other => panic!("expected assistant, got {other:?}"),
    }
    // The same events with the on-table fact produce the replay.
    let messages = project(&log, &kimi(), &caps_for("deepseek-flash").unwrap());
    match &messages[0] {
        Message::Assistant {
            reasoning_content, ..
        } => assert_eq!(reasoning_content.as_deref(), Some("thought")),
        other => panic!("expected assistant, got {other:?}"),
    }
}
