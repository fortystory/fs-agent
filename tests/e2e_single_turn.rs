//! The repo's first end-to-end test: drive a whole single-agent turn through the
//! library assembly seam with a scripted fake provider, then assert the JSONL
//! event stream and the headless renderer's two sinks.
//!
//! No network, no real provider, no environment: everything the library needs is
//! injected.

mod support;

use std::path::PathBuf;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, Event, EventPayload, Role, SessionId, SpeakerId, StopReason, Usage,
};
use fs_agent::provider::{FinishReason, Message, ProviderError, StreamEvent};
use fs_agent::render::RenderSinks;
use fs_agent::{assemble, AssemblyParts, Harness};
use support::{CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, config: SessionConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        cwd: dir.path().to_path_buf(),
        log_path: log_path.clone(),
        session_id: SessionId::new("s-1"),
        config,
        sinks: RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        provider,
        stdout,
        stderr,
        log_path,
        _dir: dir,
    }
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

#[tokio::test]
async fn one_turn_lands_completed_units_in_the_log_and_only_the_final_product_on_stdout() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 4,
        cached_tokens: 6,
        miss_tokens: 4,
        reasoning_tokens: Some(2),
    };
    let mut fixture = fixture(
        vec![Reply::Stream(vec![
            StreamEvent::ReasoningDelta("weighing".into()),
            StreamEvent::TextDelta("hello ".into()),
            StreamEvent::TextDelta("from fake".into()),
            StreamEvent::Usage(usage),
            StreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            },
        ])],
        SessionConfig::new("fake-model").with_max_iterations(4),
    )
    .await;

    let outcome = fixture.harness.run_turn("say hi").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "hello from fake");
    fixture.harness.shutdown().await;

    // stdout holds exactly the final product; everything else went to stderr.
    assert_eq!(fixture.stdout.text(), "hello from fake\n");
    let diagnostics = fixture.stderr.text();
    assert!(
        diagnostics.contains("[reasoning] weighing"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("hello from fake"), "{diagnostics}");
    assert!(
        diagnostics.contains("turn ended: Completed"),
        "{diagnostics}"
    );

    // The event stream is the observable contract.
    let events = read_events(&fixture.log_path).unwrap();
    let kinds: Vec<&str> = events.iter().map(|event| event.payload.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            "SessionStarted",
            "MessageCompleted",
            "TurnStarted",
            "UsageRecorded",
            "MessageCompleted",
            "TurnEnded",
        ]
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.seq, index as u64 + 1);
    }

    match &events[0].payload {
        EventPayload::SessionStarted {
            session_id,
            schema_version,
            ..
        } => {
            assert_eq!(session_id.as_str(), "s-1");
            assert_eq!(*schema_version, fs_agent::events::SCHEMA_VERSION);
        }
        other => panic!("expected SessionStarted, got {other:?}"),
    }
    assert_eq!(events[1].speaker_id, SpeakerId::User);
    assert_eq!(events[2].speaker_id, kimi());
    match &events[3].payload {
        EventPayload::UsageRecorded { usage: recorded } => assert_eq!(*recorded, usage),
        other => panic!("expected UsageRecorded, got {other:?}"),
    }
    match &events[4].payload {
        EventPayload::MessageCompleted {
            role,
            text,
            reasoning,
        } => {
            assert_eq!(*role, Role::Assistant);
            assert_eq!(text, "hello from fake");
            assert_eq!(reasoning.as_deref(), Some("weighing"));
        }
        other => panic!("expected MessageCompleted, got {other:?}"),
    }
    match &events[5].payload {
        EventPayload::TurnEnded { reason } => assert_eq!(*reason, StopReason::Completed),
        other => panic!("expected TurnEnded, got {other:?}"),
    }

    // The provider saw the projection, the model, and the session cache key.
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "fake-model");
    assert_eq!(requests[0].cache_key.as_deref(), Some("s-1"));
    assert_eq!(
        requests[0].messages,
        vec![Message::User {
            content: "say hi".to_owned(),
            name: None,
        }]
    );
}

#[tokio::test]
async fn incremental_text_reaches_the_renderer_but_not_the_event_log() {
    let mut fixture = fixture(
        vec![Reply::chunks(&["al", "pha", "bet"])],
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("spell it").await.unwrap();
    assert_eq!(outcome.text, "alphabet");

    let events = fixture.harness.events();
    let completed: Vec<&Event> = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventPayload::MessageCompleted {
                    role: Role::Assistant,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "the three deltas land as one completed unit, not three"
    );
    match &completed[0].payload {
        EventPayload::MessageCompleted { text, .. } => assert_eq!(text, "alphabet"),
        other => panic!("expected MessageCompleted, got {other:?}"),
    }

    fixture.harness.shutdown().await;
    assert_eq!(fixture.stdout.text(), "alphabet\n");
    assert!(
        fixture.stderr.text().contains("alphabet"),
        "the incremental deltas are streamed to the diagnostic sink"
    );
}

#[tokio::test]
async fn a_tool_call_gets_exactly_one_result_and_the_loop_continues() {
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::TextDelta("checking".into()),
                StreamEvent::ToolCallStarted {
                    index: 0,
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".into(),
                    name: "read_file".into(),
                    arguments: "{\"path\":\"src/lib.rs\"}".into(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("all done"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("read the file").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "all done");

    let events = fixture.harness.events();
    let starts = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallStarted { .. }))
        .count();
    let results: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(starts, 1);
    assert_eq!(results.len(), 1, "every tool_call gets exactly one result");
    match &results[0].payload {
        EventPayload::ToolCallCompleted { ok, error, .. } => {
            assert!(!ok);
            assert!(error.as_deref().unwrap().contains("no tool registered"));
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
    assert_eq!(
        fixture.provider.call_count(),
        2,
        "the turn continued after the tool call"
    );

    // The second projection replays the assistant tool call and its one result.
    let second = &fixture.provider.requests()[1];
    let tail = &second.messages[second.messages.len() - 2..];
    match &tail[0] {
        Message::Assistant { tool_calls, .. } => assert_eq!(tool_calls[0].id, "call-1"),
        other => panic!("expected Assistant with tool_calls, got {other:?}"),
    }
    match &tail[1] {
        Message::Tool {
            tool_call_id,
            content,
        } => {
            assert_eq!(tool_call_id, "call-1");
            assert!(content.contains("no tool registered"));
        }
        other => panic!("expected Tool result, got {other:?}"),
    }

    fixture.harness.shutdown().await;
    assert_eq!(fixture.stdout.text(), "all done\n");
}

#[tokio::test]
async fn a_turn_that_keeps_asking_for_tools_hits_max_iterations() {
    let tool_reply = || {
        Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ])
    };

    let mut fixture = fixture(
        vec![tool_reply(), tool_reply()],
        SessionConfig::new("fake-model").with_max_iterations(2),
    )
    .await;

    let outcome = fixture.harness.run_turn("go").await.unwrap();
    assert_eq!(outcome.reason, StopReason::MaxIterations);
    assert_eq!(fixture.provider.call_count(), 2);

    let events = fixture.harness.events();
    assert!(matches!(
        events.last().unwrap().payload,
        EventPayload::TurnEnded {
            reason: StopReason::MaxIterations
        }
    ));
}

#[tokio::test]
async fn a_provider_failure_ends_the_turn_with_error() {
    let mut fixture = fixture(
        vec![Reply::Fail(ProviderError::Transport {
            detail: "boom".into(),
        })],
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("go").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Error);

    let events = fixture.harness.events();
    assert!(matches!(
        events.last().unwrap().payload,
        EventPayload::TurnEnded {
            reason: StopReason::Error
        }
    ));

    fixture.harness.shutdown().await;
    assert!(fixture.stdout.text().is_empty());
}
