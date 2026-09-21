//! The repo's first end-to-end test: drive a whole single-agent turn through the
//! library assembly seam with a scripted fake provider, then assert the JSONL
//! event stream and the headless renderer's two sinks.
//!
//! No network, no real provider, no environment: everything the library needs is
//! injected.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::{ReasoningEffort, SessionConfig};
use fs_agent::events::{
    read_events, Event, EventPayload, Role, SessionId, SpeakerId, StopReason, Usage,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, Message, ProviderError, StreamEvent};
use fs_agent::render::RenderSinks;
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    /// The session workspace: `cwd` for the harness, and where tests put files
    /// the scripts tool calls against.
    cwd: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, config: SessionConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    // Session state (the log, tool artifacts) and the workspace are siblings, so
    // a tool writing into the workspace cannot collide with the session files.
    let session = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let log_path = session.join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        sinks: RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        },
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-1"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            // An interactive session: the default `ask` mode, with a user who
            // approves every write. Permission-specific tests script their own.
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(Arc::new(AlwaysAllow)),
            hook: None,
            home: None,
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
        cwd,
        _dir: dir,
    }
}

impl Fixture {
    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.cwd.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        std::fs::canonicalize(&path).unwrap()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.cwd.join(name)).unwrap()
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
    // The human is another speaker, so the projection names the participant;
    // outside a discussion round there is no round label to prefix it with.
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "fake-model");
    assert_eq!(requests[0].cache_key.as_deref(), Some("s-1"));
    assert_eq!(
        requests[0].messages,
        vec![Message::User {
            content: "say hi".to_owned(),
            name: Some("user".to_owned()),
            injected: false,
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

    let events = read_events(&fixture.log_path).unwrap();
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
                    arguments: "{\"file_path\":\"src/lib.rs\"}".into(),
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

    fixture.write("src/lib.rs", "pub fn main() {}\n");

    let outcome = fixture.harness.run_turn("read the file").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "all done");

    let events = read_events(&fixture.log_path).unwrap();
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
        EventPayload::ToolCallCompleted {
            ok,
            output,
            error,
            tool_call_id,
            ..
        } => {
            assert!(ok, "{error:?}");
            assert_eq!(tool_call_id.as_str(), "call-1");
            let output = output.as_deref().unwrap();
            assert!(output.contains("pub fn main() {}"), "{output}");
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }
    let turn_starts = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::TurnStarted { .. }))
        .count();
    assert_eq!(turn_starts, 2, "the turn continued after the tool call");

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
            assert!(content.contains("pub fn main() {}"), "{content}");
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

    let events = read_events(&fixture.log_path).unwrap();
    let turn_starts = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::TurnStarted { .. }))
        .count();
    assert_eq!(turn_starts, 2, "the loop stopped at the iteration cap");
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

    let events = read_events(&fixture.log_path).unwrap();
    assert!(matches!(
        events.last().unwrap().payload,
        EventPayload::TurnEnded {
            reason: StopReason::Error
        }
    ));

    fixture.harness.shutdown().await;
    assert!(fixture.stdout.text().is_empty());
}

#[tokio::test]
async fn a_stream_that_ends_without_done_is_an_error_and_logs_no_completed_message() {
    let mut fixture = fixture(
        vec![Reply::Raw(vec![Ok(StreamEvent::TextDelta(
            "partial".into(),
        ))])],
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("go").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Error);

    let events = read_events(&fixture.log_path).unwrap();
    assert!(
        !events.iter().any(|event| matches!(
            &event.payload,
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                ..
            }
        )),
        "a stream without [DONE] produced no completed unit"
    );
    assert!(matches!(
        events.last().unwrap().payload,
        EventPayload::TurnEnded {
            reason: StopReason::Error
        }
    ));
}

#[tokio::test]
async fn a_mid_stream_error_ends_the_turn_with_error() {
    let mut fixture = fixture(
        vec![Reply::Raw(vec![
            Ok(StreamEvent::TextDelta("partial".into())),
            Err(ProviderError::Transport {
                detail: "connection dropped".into(),
            }),
        ])],
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("go").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Error);

    let events = read_events(&fixture.log_path).unwrap();
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            ..
        }
    )));
    assert!(matches!(
        events.last().unwrap().payload,
        EventPayload::TurnEnded {
            reason: StopReason::Error
        }
    ));
}

#[tokio::test]
async fn a_tool_call_with_no_arguments_records_an_empty_object() {
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".into(),
                    name: "repo_map".into(),
                    arguments: String::new(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("done"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("map it").await.unwrap();

    let events = read_events(&fixture.log_path).unwrap();
    let args = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallStarted { args, .. } => Some(args.clone()),
            _ => None,
        })
        .expect("a ToolCallStarted event");
    assert_eq!(args, serde_json::json!({}), "empty arguments mean {{}}");

    let second = &fixture.provider.requests()[1];
    match &second.messages[second.messages.len() - 2] {
        Message::Assistant { tool_calls, .. } => assert_eq!(tool_calls[0].arguments, "{}"),
        other => panic!("expected Assistant with tool_calls, got {other:?}"),
    }
}

#[tokio::test]
async fn the_reasoning_tier_is_pinned_for_the_whole_session() {
    // Switching Kimi's reasoning tier mid-session throws away the prefix cache,
    // so the tier is a session value set before the first turn, not a per-call
    // parameter the loop may vary.
    let mut fixture = fixture(
        vec![Reply::text("first"), Reply::text("second")],
        SessionConfig::new("fake-model").with_reasoning_effort(ReasoningEffort::High),
    )
    .await;

    fixture.harness.run_turn("one").await.unwrap();
    fixture.harness.run_turn("two").await.unwrap();
    fixture.harness.shutdown().await;

    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.params.reasoning_effort, Some(ReasoningEffort::High));
    }
}

/// A scripted tool call that completes in one stream.
fn tool_reply(id: &str, name: &str, arguments: &str, finish_reason: FinishReason) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallStarted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
        },
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        },
        StreamEvent::Finished { finish_reason },
    ])
}

#[tokio::test]
async fn an_edit_file_call_changes_the_file_and_records_the_replaced_bytes() {
    // The whole point of the tool loop: the model asks, the file really changes,
    // the stream records start and finish, and `.before` is the bytes that were
    // replaced (the source `/undo` restores from).
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-read",
                "read_file",
                "{\"file_path\":\"notes.txt\"}",
                FinishReason::ToolCalls,
            ),
            tool_reply(
                "call-edit",
                "edit_file",
                "{\"file_path\":\"notes.txt\",\"old_string\":\"one\\n\",\"new_string\":\"uno\\n\"}",
                FinishReason::ToolCalls,
            ),
            Reply::text("renamed the first line"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;
    let file = fixture.write("notes.txt", "one\ntwo\n");

    let outcome = fixture.harness.run_turn("rename the line").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "renamed the first line");

    // The workspace really changed.
    assert_eq!(fixture.read("notes.txt"), "uno\ntwo\n");
    let outputs_dir = fixture.harness.outputs_dir().to_path_buf();
    fixture.harness.shutdown().await;

    // The stream holds exactly one start and one end for each call, in order.
    let events = read_events(&fixture.log_path).unwrap();
    let calls: Vec<(String, &str)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                ..
            } => Some((tool_call_id.as_str().to_owned(), tool_name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            ("call-read".to_owned(), "read_file"),
            ("call-edit".to_owned(), "edit_file"),
        ]
    );
    let completed: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(completed.len(), 2, "each tool_call gets exactly one result");

    let edit_result = completed
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventPayload::ToolCallCompleted { tool_call_id, .. }
                    if tool_call_id.as_str() == "call-edit"
            )
        })
        .expect("the edit call has a result");
    match &edit_result.payload {
        EventPayload::ToolCallCompleted { ok, output, .. } => {
            assert!(ok);
            let output = output.as_deref().unwrap();
            assert!(
                output.contains(&format!(
                    "{} {}",
                    fs_agent::tools::MATCH_LEVEL_PREFIX,
                    "exact"
                )),
                "the match level is reported as convention text: {output}"
            );
        }
        other => panic!("expected ToolCallCompleted, got {other:?}"),
    }

    // `.before` is the actual replaced bytes, not the caller's `old_string`. It
    // lives beside the event log, so a session stays one movable directory.
    let snapshot = outputs_dir.join("call-edit.before");
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "one\n");

    // The projection replays the edit result as the tool message the model sees.
    let third = &fixture.provider.requests()[2];
    match third.messages.last().unwrap() {
        Message::Tool {
            tool_call_id,
            content,
        } => {
            assert_eq!(tool_call_id, "call-edit");
            assert!(content.contains("edit match"), "{content}");
        }
        other => panic!("expected the edit result as the last message, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "uno\ntwo\n");
}

#[tokio::test]
async fn an_edit_before_a_read_is_refused_and_the_file_is_left_alone() {
    // The guardrail is in the loop's path, not in the model's manners: a scripted
    // edit with no preceding read is refused and produces its one required error
    // result.
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-edit",
                "edit_file",
                "{\"file_path\":\"notes.txt\",\"old_string\":\"one\",\"new_string\":\"uno\"}",
                FinishReason::ToolCalls,
            ),
            Reply::text("understood"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;
    fixture.write("notes.txt", "one\ntwo\n");

    fixture.harness.run_turn("just edit it").await.unwrap();

    assert_eq!(fixture.read("notes.txt"), "one\ntwo\n");
    let events = read_events(&fixture.log_path).unwrap();
    let result = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted { error, ok, .. } => Some((*ok, error.clone())),
            _ => None,
        })
        .expect("the refused call still gets a result");
    assert!(!result.0);
    let message = result.1.unwrap();
    assert!(message.contains("read before write"), "{message}");
}

#[tokio::test]
async fn the_edit_ladder_reports_a_downgraded_match_in_the_event_stream() {
    // The model sends spaces where the file has a tab: the edit lands at the
    // line-trim level, and the level is visible in the stream rather than silent.
    let mut fixture = fixture(
        vec![
            tool_reply(
                "call-read",
                "read_file",
                "{\"file_path\":\"code.rs\"}",
                FinishReason::ToolCalls,
            ),
            tool_reply(
                "call-edit",
                "edit_file",
                "{\"file_path\":\"code.rs\",\"old_string\":\"    run();\",\"new_string\":\"    run_twice();\"}",
                FinishReason::ToolCalls,
            ),
            Reply::text("done"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;
    fixture.write("code.rs", "fn main() {\n\trun();\n}\n");

    fixture.harness.run_turn("fix indentation").await.unwrap();

    assert_eq!(
        fixture.read("code.rs"),
        "fn main() {\n    run_twice();\n}\n"
    );
    let events = read_events(&fixture.log_path).unwrap();
    let output = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id,
                output: Some(output),
                ..
            } if tool_call_id.as_str() == "call-edit" => Some(output.clone()),
            _ => None,
        })
        .expect("the edit result");
    assert!(output.contains("line-trim"), "{output}");

    // `.before` carries the real replaced bytes (with the tab), which is the
    // whole reason the level is recorded at all.
    let snapshot = fixture.harness.outputs_dir().join("call-edit.before");
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "\trun();");
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_turn_that_has_spent_the_session_allowance_ends_budget_exhausted() {
    // The hard stop is cumulative, not per turn (spec §17): the first call lands
    // the whole allowance, the tool call it asked for still keeps its one result
    // — the unit in flight completes — and the turn then stops instead of
    // opening a second provider call.
    let limit = 1_000;
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".into(),
                    name: "read_file".into(),
                    arguments: "{\"file_path\":\"src/lib.rs\"}".into(),
                },
                StreamEvent::Usage(Usage {
                    input_tokens: limit,
                    output_tokens: 0,
                    cached_tokens: 0,
                    miss_tokens: limit,
                    reasoning_tokens: None,
                }),
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("a reply the budget refuses to ask for"),
        ],
        SessionConfig::new("fake-model").with_session_token_limit(limit),
    )
    .await;
    fixture.write("src/lib.rs", "pub fn main() {}\n");

    let outcome = fixture.harness.run_turn("read the file").await.unwrap();
    assert_eq!(outcome.reason, StopReason::BudgetExhausted);
    assert_eq!(
        fixture.provider.requests().len(),
        1,
        "the second model call is never made"
    );

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::TurnStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.payload,
                EventPayload::ToolCallCompleted { ok: true, .. }
            ))
            .count(),
        1,
        "the call that was in flight keeps its one result"
    );

    fixture.harness.shutdown().await;
    let stderr = fixture.stderr.text();
    assert!(
        stderr.contains("[turn ended: BudgetExhausted]"),
        "the reason is narrated: {stderr}"
    );
    // Distinct from the reasons the same line can carry: a turn that ran out of
    // its own iterations, or one that finished, must not read the same.
    assert!(!stderr.contains("[turn ended: MaxIterations]"), "{stderr}");
    assert!(!stderr.contains("[turn ended: Completed]"), "{stderr}");
}

#[tokio::test]
async fn a_call_the_pre_flight_estimate_refuses_is_never_sent() {
    // The pre-flight half of the gate (spec §17): every droppable class fits the
    // window, but the call plainly would not fit what is left of the session's
    // allowance, so it is never sent — the cheap half of the hard stop.
    let mut fixture = fixture(
        vec![Reply::text("a reply the budget refuses to ask for")],
        SessionConfig::new("fake-model").with_session_token_limit(10),
    )
    .await;

    let prompt = "x".repeat(400);
    let outcome = fixture.harness.run_turn(&prompt).await.unwrap();
    assert_eq!(outcome.reason, StopReason::BudgetExhausted);
    assert!(
        fixture.provider.requests().is_empty(),
        "a call that would not fit is not sent"
    );

    fixture.harness.shutdown().await;
    let stderr = fixture.stderr.text();
    assert!(stderr.contains("does not fit"), "{stderr}");
}
