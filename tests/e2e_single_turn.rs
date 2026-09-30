//! 仓库里的第一个端到端测试：用一个脚本化的假 provider，把一整个
//! 单一 agent 的回合穿过库的组装接缝跑一遍，然后断言 JSONL
//! 事件流与 headless 渲染器的那两个 sink。
//!
//! 没有网络、没有真 provider、没有环境：库需要的一切都是
//! 注入的。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::{ReasoningEffort, SessionConfig};
use fs_agent::events::{
    read_events, Event, EventPayload, Role, SessionId, SpeakerId, StopReason, Usage,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, Message, ProviderError, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    /// 会话的工作区：harness 的 `cwd`，也是测试摆下那些供脚本里的
    /// 工具调用去动的文件的地方。
    cwd: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, config: SessionConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    // 会话状态（日志、工具产物）与工作区是平级的，所以
    // 往工作区里写的工具撞不到会话文件。
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
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-1"),
            tools: fs_agent::tools::builtin(false),
            locks: fs_agent::tools::PathLocks::new(),
            // 一个交互式会话：默认的 `ask` 档，配一个每次都放行写的
            // 用户。专测权限的那些测试自己脚本化自己的。
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(Arc::new(AlwaysAllow)),
            questions: None,
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

    // stdout 上正好是最终产物；别的全都去了 stderr。
    assert_eq!(fixture.stdout.text(), "hello from fake\n");
    let diagnostics = fixture.stderr.text();
    assert!(diagnostics.contains("weighing"), "{diagnostics}");
    assert!(diagnostics.contains("hello from fake"), "{diagnostics}");
    assert!(
        diagnostics.contains(&fs_agent::render::wording::turn_ended(
            StopReason::Completed
        )),
        "这个回合的收尾被叙述了：{diagnostics}"
    );

    // 事件流就是那份可观察契约。
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
        other => panic!("期望 SessionStarted，实际得到 {other:?}"),
    }
    assert_eq!(events[1].speaker_id, SpeakerId::User);
    assert_eq!(events[2].speaker_id, kimi());
    match &events[3].payload {
        EventPayload::UsageRecorded { usage: recorded } => assert_eq!(*recorded, usage),
        other => panic!("期望 UsageRecorded，实际得到 {other:?}"),
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
        other => panic!("期望 MessageCompleted，实际得到 {other:?}"),
    }
    match &events[5].payload {
        EventPayload::TurnEnded { reason } => assert_eq!(*reason, StopReason::Completed),
        other => panic!("期望 TurnEnded，实际得到 {other:?}"),
    }

    // provider 看到的是投影、模型与那个会话缓存键。
    // 人是另一个发言者，所以投影点的是参与者这个名字；
    // 在讨论轮次之外，没有轮次标签可以给它做前缀。
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "fake-model");
    assert_eq!(requests[0].cache_key.as_deref(), Some("s-1"));
    assert_eq!(
        requests[0].messages,
        vec![
            Message::System {
                content: fs_agent::agent::agent_identity().to_owned(),
                name: None,
            },
            Message::User {
                content: "say hi".to_owned(),
                name: Some("user".to_owned()),
                injected: false,
            },
        ]
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
    assert_eq!(completed.len(), 1, "三条增量落成一条完成单元，而不是三条");
    match &completed[0].payload {
        EventPayload::MessageCompleted { text, .. } => assert_eq!(text, "alphabet"),
        other => panic!("期望 MessageCompleted，实际得到 {other:?}"),
    }

    fixture.harness.shutdown().await;
    assert_eq!(fixture.stdout.text(), "alphabet\n");
    assert!(
        fixture.stderr.text().contains("alphabet"),
        "增量文本被流式送到诊断 sink"
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
    assert_eq!(results.len(), 1, "每一次 tool_call 正好拿到一条结果");
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
        other => panic!("期望 ToolCallCompleted，实际得到 {other:?}"),
    }
    let turn_starts = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::TurnStarted { .. }))
        .count();
    assert_eq!(turn_starts, 2, "这次工具调用之后，回合继续了");

    // 第二次投影重放 assistant 那次工具调用与它那一条结果。
    let second = &fixture.provider.requests()[1];
    let tail = &second.messages[second.messages.len() - 2..];
    match &tail[0] {
        Message::Assistant { tool_calls, .. } => assert_eq!(tool_calls[0].id, "call-1"),
        other => panic!("期望带 tool_calls 的 Assistant，实际得到 {other:?}"),
    }
    match &tail[1] {
        Message::Tool {
            tool_call_id,
            content,
        } => {
            assert_eq!(tool_call_id, "call-1");
            assert!(content.contains("pub fn main() {}"), "{content}");
        }
        other => panic!("期望 Tool 结果，实际得到 {other:?}"),
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
    assert_eq!(turn_starts, 2, "循环停在迭代上限上");
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
        "一条没有 [DONE] 的流没有产出任何完成单元"
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
        .expect("一条 ToolCallStarted 事件");
    assert_eq!(args, serde_json::json!({}), "空参数意味着 {{}}");

    let second = &fixture.provider.requests()[1];
    match &second.messages[second.messages.len() - 2] {
        Message::Assistant { tool_calls, .. } => assert_eq!(tool_calls[0].arguments, "{}"),
        other => panic!("期望带 tool_calls 的 Assistant，实际得到 {other:?}"),
    }
}

#[tokio::test]
async fn the_reasoning_tier_is_pinned_for_the_whole_session() {
    // 会话中途切换 Kimi 的推理档位会把前缀缓存扔掉，
    // 所以档位是第一个回合之前设好的会话值，而不是循环
    // 可以随便改的一次调用参数。
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

/// 一条在一条流里完成的脚本化工具调用。
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
    // 工具循环的全部意义：模型开口，文件真的变了，
    // 流上记下开始与结束，而 `.before` 是被替换掉的那些字节
    // （`/undo` 就是从它恢复的）。
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

    // 工作区真的变了。
    assert_eq!(fixture.read("notes.txt"), "uno\ntwo\n");
    let outputs_dir = fixture.harness.outputs_dir().to_path_buf();
    fixture.harness.shutdown().await;

    // 流上每一次调用正好一个开始、一个结束，按顺序。
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
    assert_eq!(completed.len(), 2, "每一次 tool_call 正好拿到一条结果");

    let edit_result = completed
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventPayload::ToolCallCompleted { tool_call_id, .. }
                    if tool_call_id.as_str() == "call-edit"
            )
        })
        .expect("这次编辑调用有一条结果");
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
                "匹配层级是按约定的文本报出来的：{output}"
            );
        }
        other => panic!("期望 ToolCallCompleted，实际得到 {other:?}"),
    }

    // `.before` 是被替换掉的真实字节，不是调用者给的 `old_string`。它
    // 住在事件流旁边，所以一个会话始终是一个可搬运的目录。
    let snapshot = outputs_dir.join("call-edit.before");
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "one\n");

    // 投影把这次编辑的结果重放成模型看到的那个工具消息。
    let third = &fixture.provider.requests()[2];
    match third.messages.last().unwrap() {
        Message::Tool {
            tool_call_id,
            content,
        } => {
            assert_eq!(tool_call_id, "call-edit");
            assert!(content.contains("edit match"), "{content}");
        }
        other => panic!("期望最后一条消息是编辑的结果，实际得到 {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "uno\ntwo\n");
}

#[tokio::test]
async fn an_edit_before_a_read_is_refused_and_the_file_is_left_alone() {
    // 护栏在循环的路径上，不在模型的教养里：一次没有
    // 前置读的脚本化编辑被拒，并产出它那条必需的错误
    // 结果。
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
        .expect("被拒的那次调用照样拿到一条结果");
    assert!(!result.0);
    let message = result.1.unwrap();
    assert!(message.contains("read before write"), "{message}");
}

#[tokio::test]
async fn the_edit_ladder_reports_a_downgraded_match_in_the_event_stream() {
    // 文件里是制表符而模型发的是空格：这次编辑落在
    // line-trim 这一级上，而这一级在流上看得见，不是悄悄的。
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
        .expect("这次编辑的结果");
    assert!(output.contains("line-trim"), "{output}");

    // `.before` 扛的是真实被替换的字节（带那个制表符），
    // 这一级之所以被记下来，全部理由就在这儿。
    let snapshot = fixture.harness.outputs_dir().join("call-edit.before");
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), "\trun();");
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_turn_that_has_spent_the_session_allowance_ends_budget_exhausted() {
    // 硬停是累计的，不是按回合的（spec §17）：第一次调用花掉
    // 整个额度，它要的那次工具调用照样留着它那一条结果
    // —— 在飞的那个单元跑完 —— 随后这个回合就停了，
    // 而不是再开一次 provider 调用。
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
        "第二次模型调用从来没发出"
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
        "当时在飞的那次调用留住了它那一条结果"
    );

    fixture.harness.shutdown().await;
    let stderr = fixture.stderr.text();
    let budget = fs_agent::render::wording::turn_ended(StopReason::BudgetExhausted);
    assert!(stderr.contains(&budget), "那个理由被叙述了：{stderr}");
    // 与同一行可能扛的别的收尾原因要分得开：一个把自己的
    // 迭代数用光的回合，或者一个正常跑完的回合，读起来必须不一样。
    assert!(
        !stderr.contains(&fs_agent::render::wording::turn_ended(
            StopReason::MaxIterations
        )),
        "{stderr}"
    );
    assert!(
        !stderr.contains(&fs_agent::render::wording::turn_ended(
            StopReason::Completed
        )),
        "{stderr}"
    );
}

#[tokio::test]
async fn a_call_the_pre_flight_estimate_refuses_is_never_sent() {
    // 闸门起飞前那一半（spec §17）：每一类可丢的东西都装得进
    // 窗口，但这次调用显然装不进会话额度剩下的部分，
    // 所以它永远不会被发出去 —— 硬停便宜的那一半。
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
        "装不下的调用不会被发出去"
    );

    fixture.harness.shutdown().await;
    let stderr = fixture.stderr.text();
    assert!(stderr.contains("装不下"), "{stderr}");
}
